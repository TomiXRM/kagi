use kagi_git::backend::recording::{self, RunReport};
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::{GitError, OperationOutcome, OperationPlan, PullOutcome, StashPopOutcome};

use super::{open_backend, verify_after_snapshot};
use crate::app::{PullPresentation, PullReport};
use crate::ui::i18n;

/// The restore-prediction notes a pull plan carries, in plan order.
///
/// The comparison that decides whether a confirmation is still honest (#625):
/// the dirty digest catches a path joining or leaving the set, and these notes
/// catch the prediction itself changing — a path that was mergeable becoming a
/// conflict. The UI's auto-stash rewrite replaces the two generic dirty
/// warnings and leaves these untouched, so a UI-owned plan and a freshly built
/// one are comparable here.
fn restore_notes(plan: &OperationPlan) -> Vec<&kagi_domain::plan_note::PullNote> {
    use kagi_domain::plan_note::{PlanNote, PullNote};
    plan.warnings
        .iter()
        .filter_map(|note| match note {
            PlanNote::Pull(
                inner @ (PullNote::RestoreConflict { .. }
                | PullNote::RestoreConflictPossible { .. }),
            ) => Some(inner),
            _ => None,
        })
        .collect()
}

/// A receipt for what did **not** run. The workflow can stop before any child
/// writes its own receipt — a stale confirmation, a plan that will not build —
/// and ADR-0196 says the core records that, never the UI (which would be
/// synthesizing an entry from a result it did not produce).
fn no_execute(
    op: &'static str,
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    outcome: OpOutcome,
    error: String,
) -> RunReport {
    RunReport {
        result: Err(GitError::Other(error)),
        recording: recording::finalize(OpLogEntry::new(
            op,
            repo_path.display().to_string(),
            plan.current.clone(),
            outcome,
        )),
        stash: None,
    }
}

/// May the workflow start its restore after a failed pull?
///
/// A pull whose termination is unconfirmed may be followed by **no** further
/// mutating step — least of all a pop "tidying up" the auto-stash while the
/// pull may still be applying (ADR-0196 決定 5, ADR-0177). Every other failure,
/// including an outcome kagi did not expect, is a stopped writer whose stash
/// must go back.
fn may_restore_after(result: &Result<OperationOutcome, GitError>) -> bool {
    !matches!(result, Err(GitError::TerminationUnknown(_)))
}

/// The workflow stopped before it started: one recorded step, and it decides.
fn not_started(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    outcome: OpOutcome,
    error: String,
) -> PullReport {
    let step = no_execute("pull", repo_path, plan, outcome, error.clone());
    PullReport::settled(vec![step], PullPresentation::Failed { error }, None)
}

fn failed_to_start(repo_path: &std::path::Path, plan: &OperationPlan, error: String) -> PullReport {
    let outcome = OpOutcome::Failed {
        error: error.clone(),
    };
    not_started(repo_path, plan, outcome, error)
}

/// Blocking part of Pull. A dirty plan uses the confirmed
/// stash → pull → pop sequence; every step remains a planned Backend operation
/// and contributes its own receipt to [`PullReport::steps`], in execution
/// order. `Err` means the repository would not open — the job records that.
pub(crate) fn pull_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    auto_stash: bool,
    promised_dirty: Option<kagi_domain::status::WorktreeDigest>,
) -> Result<PullReport, String> {
    let mut repo =
        open_backend(repo_path).map_err(|error| i18n::op_failed(i18n::Op::RepoOpen, error))?;

    let mut steps: Vec<RunReport> = Vec::new();
    let mut stashed_oid = None;
    if auto_stash {
        let dirty = match repo.working_tree_status() {
            Ok(status) => status.is_dirty(),
            Err(error) => {
                return Ok(failed_to_start(
                    repo_path,
                    plan,
                    i18n::op_failed(i18n::Op::Stash, error),
                ))
            }
        };
        // #625 / ADR-0192: the confirmation named what would be stashed and
        // what the restore would do to it. Between confirming and here the
        // working tree can move — an editor saves another path the update also
        // touches — and nothing downstream would notice: this stashes first, so
        // `Backend::run`'s preflight and the execute-time dirty-path guard both
        // see a clean tree and wave the pull through, leaving only the restore
        // to conflict with no warning ever shown (#626 review).
        //
        // So the promise is checked here, before anything is stashed: re-plan
        // and refuse if the dirty set or the restore prediction moved. Refusing
        // costs the user one more click on an accurate confirmation; stashing
        // blind costs them the surprise this whole change exists to remove.
        if dirty {
            let now = match repo.working_tree_status() {
                Ok(status) => status.digest(),
                Err(error) => {
                    return Ok(failed_to_start(
                        repo_path,
                        plan,
                        i18n::op_failed(i18n::Op::Stash, error),
                    ))
                }
            };
            let fresh = match repo.plan_pull() {
                Ok(fresh) => fresh,
                Err(error) => {
                    return Ok(failed_to_start(
                        repo_path,
                        plan,
                        i18n::op_plan_failed(i18n::Op::Pull, error),
                    ))
                }
            };
            if promised_dirty != Some(now) || restore_notes(&fresh) != restore_notes(plan) {
                let refusal = i18n::auto_stash_plan_stale().to_string();
                return Ok(not_started(
                    repo_path,
                    plan,
                    OpOutcome::Refused {
                        blockers: vec![refusal.clone()],
                    },
                    refusal,
                ));
            }
            let stash_op = kagi_git::Operation::StashPush {
                message: Some("kagi: auto-stash before pull".to_string()),
                include_untracked: true,
            };
            let stash_plan = match repo.plan(&stash_op) {
                Ok(plan) => plan,
                Err(error) => {
                    return Ok(failed_to_start(
                        repo_path,
                        plan,
                        i18n::op_plan_failed(i18n::Op::Stash, error),
                    ))
                }
            };
            let push = repo.run_recorded(&stash_op, &stash_plan);
            let mut stop = None;
            match &push.result {
                Ok(OperationOutcome::StashPush { oid }) => stashed_oid = Some(oid.clone()),
                // #623: the stash exists but a concurrent external push made it
                // indistinguishable, so no OID came back. Same user situation as
                // an unexpected outcome — work is saved, pull must not start,
                // and the Unknown receipt already asks for reconciliation.
                Ok(_) | Err(GitError::StashIdentityUnverified(_)) => {
                    stop = Some(PullPresentation::Partial {
                        error: i18n::auto_stash_identity_unverified().to_string(),
                    })
                }
                Err(error) => {
                    stop = Some(PullPresentation::Failed {
                        error: i18n::op_failed(i18n::Op::Stash, error),
                    })
                }
            }
            steps.push(push);
            if let Some(presentation) = stop {
                return Ok(PullReport::settled(steps, presentation, None));
            }
        }
    }

    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let pull = repo.run_recorded(&kagi_git::Operation::Pull, plan);
    let mut summary = None;
    let mut failure = None;
    match &pull.result {
        Ok(OperationOutcome::Pull(outcome)) => {
            summary = Some(match outcome {
                PullOutcome::UpToDate => "already up to date".to_string(),
                PullOutcome::FastForward { to } => format!("fast-forward to {}", to.short()),
                PullOutcome::Merged { commit } => format!("merge commit {}", commit.short()),
            })
        }
        Ok(_) => failure = Some("pull: unexpected outcome".to_string()),
        Err(error) => failure = Some(i18n::op_failed(i18n::Op::Pull, error)),
    }
    let decisive_pull = pull.clone();
    steps.push(pull);

    if let Some(error) = failure {
        // The stash stays exactly where it is and travels in the recovery
        // context instead.
        if !may_restore_after(&decisive_pull.result) {
            return Ok(PullReport::settled(
                steps,
                PullPresentation::Partial { error },
                stashed_oid,
            ));
        }
        let Some(stash_oid) = stashed_oid.clone() else {
            return Ok(PullReport::settled(
                steps,
                PullPresentation::Failed { error },
                None,
            ));
        };
        let (pop, restored) = pop_auto_stash(repo_path, &mut repo, plan, &stash_oid);
        steps.push(pop);
        return Ok(match restored {
            // The restore put the work back, so what decided this workflow is
            // the pull that failed — not the pop that succeeded after it.
            Ok(StashPopOutcome::Applied) => PullReport::new(
                steps,
                decisive_pull,
                PullPresentation::Failed {
                    error: i18n::pull_failed_stash_restored(&error),
                },
                None,
            ),
            Ok(StashPopOutcome::ConflictedStashKept { files }) => PullReport::settled(
                steps,
                PullPresentation::Partial {
                    error: i18n::auto_stash_restore_conflicted(Some(&error), &files.join(", ")),
                },
                Some(stash_oid),
            ),
            Err(pop_error) => PullReport::settled(
                steps,
                PullPresentation::Partial {
                    error: i18n::auto_stash_restore_failed(Some(&error), &pop_error),
                },
                Some(stash_oid),
            ),
        });
    }

    let summary = summary.expect("a pull that did not fail carries its outcome");
    klog!("executed: pull — {}", summary);

    if let Some(stash_oid) = stashed_oid.clone() {
        let (pop, restored) = pop_auto_stash(repo_path, &mut repo, plan, &stash_oid);
        steps.push(pop);
        match restored {
            Ok(StashPopOutcome::Applied) => {}
            Ok(StashPopOutcome::ConflictedStashKept { files }) => {
                let after = verify_after_snapshot(repo_path, plan);
                klog!("verified: pull after = {}", after.head);
                return Ok(PullReport::settled(
                    steps,
                    PullPresentation::Partial {
                        error: i18n::auto_stash_restore_conflicted(None, &files.join(", ")),
                    },
                    Some(stash_oid),
                ));
            }
            Err(error) => {
                let after = verify_after_snapshot(repo_path, plan);
                klog!("verified: pull after = {}", after.head);
                return Ok(PullReport::settled(
                    steps,
                    PullPresentation::Partial {
                        error: i18n::auto_stash_restore_failed(None, &error),
                    },
                    Some(stash_oid),
                ));
            }
        }
    }

    let after = verify_after_snapshot(repo_path, plan);
    klog!("verified: pull after = {}", after.head);
    Ok(PullReport::settled(
        steps,
        PullPresentation::Success {
            summary: if stashed_oid.is_some() {
                format!("{summary}; {}", i18n::Msg::AutoStashRestored.t())
            } else {
                summary
            },
        },
        None,
    ))
}

/// Restore the auto-stash. Always yields a receipt: the steps that stop before
/// `StashPop` runs (the entry is gone, the plan will not build) are recorded
/// here rather than reconstructed by the caller.
fn pop_auto_stash(
    repo_path: &std::path::Path,
    repo: &mut kagi_git::Backend,
    plan: &OperationPlan,
    stash_oid: &str,
) -> (RunReport, Result<StashPopOutcome, String>) {
    let refused = |error: String| {
        (
            no_execute(
                "stash-pop",
                repo_path,
                plan,
                OpOutcome::Failed {
                    error: error.clone(),
                },
                error.clone(),
            ),
            Err(error),
        )
    };
    let index = match kagi_git::Backend::unique_stash_index(repo_path, stash_oid) {
        Ok(Some(index)) => index,
        Ok(None) => return refused(i18n::auto_stash_missing().to_string()),
        Err(error) => return refused(i18n::op_failed(i18n::Op::Stash, error)),
    };
    let pop_op = kagi_git::Operation::StashPop { index };
    let pop_plan = match repo.plan(&pop_op) {
        Ok(pop_plan) => pop_plan,
        Err(error) => return refused(i18n::op_plan_failed(i18n::Op::Stash, error)),
    };
    let report = repo.run_recorded(&pop_op, &pop_plan);
    let restored = match &report.result {
        Ok(OperationOutcome::StashPop(outcome)) => Ok(outcome.clone()),
        Ok(_) => Err("stash pop: unexpected outcome".to_string()),
        Err(error) => Err(i18n::op_failed(i18n::Op::Stash, error)),
    };
    (report, restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unconfirmed_pull_never_starts_the_restore() {
        assert!(
            !may_restore_after(&Err(GitError::TerminationUnknown("deadline".into()))),
            "an unconfirmed pull must not be followed by a pop"
        );
        assert!(
            may_restore_after(&Err(GitError::Other("fetch failed".into()))),
            "a stopped writer's stash must go back"
        );
        assert!(
            may_restore_after(&Ok(OperationOutcome::Unit)),
            "an unexpected outcome is still a stopped writer"
        );
    }
}
