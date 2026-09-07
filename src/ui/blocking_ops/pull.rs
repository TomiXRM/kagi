use kagi_git::{OperationPlan, PullOutcome, StashPopOutcome, StateSummary};

use super::{open_backend, verify_after_snapshot};
use crate::ui::i18n;

/// Result of the local Pull workflow. `Partial` means Pull or stash restoration
/// changed repository state but the complete confirmed workflow did not finish.
pub(crate) enum PullBlockingResult {
    Success {
        summary: String,
        after: StateSummary,
    },
    Failed {
        error: String,
    },
    Partial {
        error: String,
        after: StateSummary,
    },
}

/// Blocking part of Pull. A dirty plan uses the confirmed
/// stash → pull → pop sequence; every step remains a planned Backend operation.
pub(crate) fn pull_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    auto_stash: bool,
) -> PullBlockingResult {
    let mut repo = match open_backend(repo_path) {
        Ok(repo) => repo,
        Err(error) => {
            return PullBlockingResult::Failed {
                error: i18n::op_failed(i18n::Op::RepoOpen, error),
            };
        }
    };

    let mut stashed_oid = None;
    if auto_stash {
        let dirty = match repo.working_tree_status() {
            Ok(status) => status.is_dirty(),
            Err(error) => {
                return PullBlockingResult::Failed {
                    error: i18n::op_failed(i18n::Op::Stash, error),
                };
            }
        };
        if dirty {
            let stash_op = kagi_git::Operation::StashPush {
                message: Some("kagi: auto-stash before pull".to_string()),
                include_untracked: true,
            };
            let stash_plan = match repo.plan(&stash_op) {
                Ok(plan) => plan,
                Err(error) => {
                    return PullBlockingResult::Failed {
                        error: i18n::op_plan_failed(i18n::Op::Stash, error),
                    };
                }
            };
            match repo.run(&stash_op, &stash_plan) {
                Ok(kagi_git::OperationOutcome::Unit) => {
                    let oid = repo
                        .plan(&kagi_git::Operation::StashPop { index: 0 })
                        .ok()
                        .and_then(|plan| plan.stash_identity)
                        .and_then(|identity| identity.oids.first().cloned());
                    match oid {
                        Some(oid) => stashed_oid = Some(oid),
                        None => {
                            return PullBlockingResult::Partial {
                                error: i18n::auto_stash_identity_unverified().to_string(),
                                after: verify_after_snapshot(repo_path, plan),
                            };
                        }
                    }
                }
                Ok(_) => {
                    return PullBlockingResult::Failed {
                        error: "stash: unexpected outcome".to_string(),
                    };
                }
                Err(error) => {
                    return PullBlockingResult::Failed {
                        error: i18n::op_failed(i18n::Op::Stash, error),
                    };
                }
            }
        }
    }

    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let pull_outcome = match repo.run(&kagi_git::Operation::Pull, plan) {
        Ok(kagi_git::OperationOutcome::Pull(outcome)) => outcome,
        Ok(_) => {
            return finish_failed_pull_with_restore(
                repo_path,
                plan,
                &mut repo,
                stashed_oid.as_deref(),
                "pull: unexpected outcome".to_string(),
            );
        }
        Err(error) => {
            return finish_failed_pull_with_restore(
                repo_path,
                plan,
                &mut repo,
                stashed_oid.as_deref(),
                i18n::op_failed(i18n::Op::Pull, error),
            );
        }
    };
    let summary = match &pull_outcome {
        PullOutcome::UpToDate => "already up to date".to_string(),
        PullOutcome::FastForward { to } => format!("fast-forward to {}", to.short()),
        PullOutcome::Merged { commit } => format!("merge commit {}", commit.short()),
    };
    klog!("executed: pull — {}", summary);

    if let Some(stash_oid) = stashed_oid.as_deref() {
        match pop_auto_stash(repo_path, &mut repo, stash_oid) {
            Ok(StashPopOutcome::Applied) => {}
            Ok(StashPopOutcome::ConflictedStashKept { files }) => {
                let after = verify_after_snapshot(repo_path, plan);
                klog!("verified: pull after = {}", after.head);
                return PullBlockingResult::Partial {
                    error: i18n::auto_stash_restore_conflicted(None, &files.join(", ")),
                    after,
                };
            }
            Err(error) => {
                let after = verify_after_snapshot(repo_path, plan);
                klog!("verified: pull after = {}", after.head);
                return PullBlockingResult::Partial {
                    error: i18n::auto_stash_restore_failed(None, &error),
                    after,
                };
            }
        }
    }

    let after = verify_after_snapshot(repo_path, plan);
    klog!("verified: pull after = {}", after.head);
    PullBlockingResult::Success {
        summary: if stashed_oid.is_some() {
            format!("{summary}; {}", i18n::Msg::AutoStashRestored.t())
        } else {
            summary
        },
        after,
    }
}

fn finish_failed_pull_with_restore(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    repo: &mut kagi_git::Backend,
    stashed_oid: Option<&str>,
    pull_error: String,
) -> PullBlockingResult {
    let Some(stash_oid) = stashed_oid else {
        return PullBlockingResult::Failed { error: pull_error };
    };
    match pop_auto_stash(repo_path, repo, stash_oid) {
        Ok(StashPopOutcome::Applied) => PullBlockingResult::Failed {
            error: i18n::pull_failed_stash_restored(&pull_error),
        },
        Ok(StashPopOutcome::ConflictedStashKept { files }) => PullBlockingResult::Partial {
            error: i18n::auto_stash_restore_conflicted(Some(&pull_error), &files.join(", ")),
            after: verify_after_snapshot(repo_path, plan),
        },
        Err(error) => PullBlockingResult::Partial {
            error: i18n::auto_stash_restore_failed(Some(&pull_error), &error),
            after: verify_after_snapshot(repo_path, plan),
        },
    }
}

fn pop_auto_stash(
    repo_path: &std::path::Path,
    repo: &mut kagi_git::Backend,
    stash_oid: &str,
) -> Result<StashPopOutcome, String> {
    let index = kagi_git::Backend::unique_stash_index(repo_path, stash_oid)
        .map_err(|error| i18n::op_failed(i18n::Op::Stash, error))?
        .ok_or_else(|| i18n::auto_stash_missing().to_string())?;
    let pop_op = kagi_git::Operation::StashPop { index };
    let pop_plan = repo
        .plan(&pop_op)
        .map_err(|error| i18n::op_plan_failed(i18n::Op::Stash, error))?;
    match repo.run(&pop_op, &pop_plan) {
        Ok(kagi_git::OperationOutcome::StashPop(outcome)) => Ok(outcome),
        Ok(_) => Err("stash pop: unexpected outcome".to_string()),
        Err(error) => Err(i18n::op_failed(i18n::Op::Stash, error)),
    }
}
