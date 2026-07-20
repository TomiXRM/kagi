//! Tracking-branch checkout and switch-to-latest pipelines (T-BCM-061 / ADR-0101).
//!
//! Split out of the monolithic `ops/pull_push.rs` (Wave 3, ADR-0116 /
//! T-SPLIT-PULLPUSH-001). Behaviour-preserving move only.
//!
//! These live in their own module (rather than `pull.rs`) because they are
//! branch-*switching* flows: they create/move a local branch to track a remote
//! ref and check out its tree, sharing the pull pipeline's upstream-resolution
//! helpers but not its merge-into-current-branch semantics. Keeping them here
//! keeps `pull.rs` focused on the pull triple and both files under the LOC target.

use super::remote_common::{local_branch_oid, short_oid_string};
use super::*;
use kagi_domain::plan_note::{
    CommonNote, DirtyParts, OpPhrase, SwitchNote, SwitchRecovery, SwitchTitle, UntrackedCtx,
};

// ────────────────────────────────────────────────────────────
// plan_checkout_tracking_branch / execute_checkout_tracking_branch (T-BCM-061)
// ────────────────────────────────────────────────────────────

/// Default local branch name for a remote-tracking branch display name.
pub fn default_tracking_branch_name(remote_branch: &str) -> String {
    remote_branch
        .split_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| remote_branch.to_string())
}

/// Plan creation of a local tracking branch from `remote_branch`, followed by
/// checking it out as one confirmed operation.
pub fn plan_checkout_tracking_branch(
    repo: &Repository,
    remote_branch: &str,
    local_branch: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if local_branch.trim().is_empty() {
        blockers.push(PlanNote::Switch(SwitchNote::LocalNameEmpty));
    }
    if repo.find_branch(local_branch, BranchType::Local).is_ok() {
        blockers.push(PlanNote::Switch(SwitchNote::LocalExists {
            name: local_branch.to_string(),
        }));
    }
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::Checkout,
        }));
    }
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let parts = DirtyParts {
            staged: status.staged.len(),
            modified: status.unstaged.len(),
        };
        blockers.push(PlanNote::Common(CommonNote::DirtyBlocksOp {
            parts,
            before: OpPhrase::Checkout,
        }));
        warnings.push(PlanNote::Common(CommonNote::SuggestStashPush));
    }
    if !status.untracked.is_empty() {
        warnings.push(PlanNote::Common(CommonNote::UntrackedRemain {
            count: status.untracked.len(),
            ctx: UntrackedCtx::AfterCheckout,
        }));
    }

    let remote_commit = resolve_branch_commit(repo, remote_branch)?;
    let predicted = StateSummary {
        head: format!("branch: {} (tracks {})", local_branch, remote_branch),
        dirty: current.dirty.clone(),
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Switch(SwitchTitle::CheckoutTracking {
            remote: remote_branch.to_string(),
            local: local_branch.to_string(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Switch(SwitchRecovery::CheckoutTracking {
                local: local_branch.to_string(),
            }),
            commands: vec![
                "git checkout -".to_string(),
                format!("git branch -d {}", local_branch),
            ],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: vec![format!(
            "{}  {}",
            short_oid(remote_commit.id()),
            remote_branch
        )],
        destructive: false,
    })
}

/// Create a local branch tracking `remote_branch` and check it out.
pub fn execute_checkout_tracking_branch(
    repo: &Repository,
    remote_branch: &str,
    local_branch: &str,
) -> Result<(), GitError> {
    if repo.find_branch(local_branch, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "Local branch '{}' already exists.",
            local_branch
        )));
    }
    let remote_commit = resolve_branch_commit(repo, remote_branch)?;
    let mut branch = repo
        .branch(local_branch, &remote_commit, false)
        .map_err(|e| GitError::Other(format!("branch create failed: {}", e.message())))?;
    branch
        .set_upstream(Some(remote_branch))
        .map_err(|e| GitError::Other(format!("set upstream failed: {}", e.message())))?;

    let obj = remote_commit.as_object();
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;
    let refname = format!("refs/heads/{}", local_branch);
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head failed: {}", e.message())))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_switch_to_latest / execute_switch_to_latest  (ADR-0101)
// ────────────────────────────────────────────────────────────

/// Remote name portion of a tracking ref like `origin/master` → `origin`.
fn remote_of_ref(remote_branch: &str) -> &str {
    remote_branch
        .split_once('/')
        .map(|(remote, _)| remote)
        .unwrap_or(remote_branch)
}

/// Plan "switch to the latest `<branch>`" (ADR-0101): fetch + switch + ff-only.
///
/// `branch_name` is the local branch (may not exist yet); `remote_branch` is the
/// tracking ref to sync to (e.g. `origin/master`). A dirty/conflicted working
/// tree is a blocker because switching rewrites the working tree. behind/ahead
/// shown here is local knowledge; execute re-checks after fetch.
pub fn plan_switch_to_latest(
    repo: &Repository,
    branch_name: &str,
    remote_branch: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if branch_name.trim().is_empty() {
        blockers.push(PlanNote::Switch(SwitchNote::NameEmpty));
    }
    if remote_branch.trim().is_empty() {
        blockers.push(PlanNote::Switch(SwitchNote::NoUpstreamToSwitch));
    }
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::Switching,
        }));
    }
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let parts = DirtyParts {
            staged: status.staged.len(),
            modified: status.unstaged.len(),
        };
        blockers.push(PlanNote::Common(CommonNote::DirtyBlocksOp {
            parts,
            before: OpPhrase::Switching,
        }));
        warnings.push(PlanNote::Common(CommonNote::SuggestStashPush));
    }
    if !status.untracked.is_empty() {
        warnings.push(PlanNote::Common(CommonNote::UntrackedRemain {
            count: status.untracked.len(),
            ctx: UntrackedCtx::AfterSwitching,
        }));
    }

    // Resolve the remote tip from local knowledge (pre-fetch).
    let remote_commit = match resolve_branch_commit(repo, remote_branch) {
        Ok(c) => Some(c),
        Err(e) => {
            blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                message: e.to_string(),
            }));
            None
        }
    };

    let local_exists = repo.find_branch(branch_name, BranchType::Local).is_ok();

    let predicted_head = if let Some(remote_commit) = remote_commit.as_ref() {
        let remote_oid = remote_commit.id();
        if !local_exists {
            warnings.push(PlanNote::Switch(SwitchNote::WillCreateTracking {
                name: branch_name.to_string(),
                remote: remote_branch.to_string(),
            }));
            format!("branch: {} (new, tracks {})", branch_name, remote_branch)
        } else {
            match local_branch_oid(repo, branch_name) {
                Ok(local_oid) if local_oid == remote_oid => {
                    format!("branch: {} (already up to date)", branch_name)
                }
                Ok(local_oid)
                    if repo
                        .graph_descendant_of(remote_oid, local_oid)
                        .unwrap_or(false) =>
                {
                    let (_, behind) = repo
                        .graph_ahead_behind(local_oid, remote_oid)
                        .unwrap_or((0, 0));
                    warnings.push(PlanNote::Switch(SwitchNote::FfLocalKnowledge { behind }));
                    format!(
                        "branch: {} -> {}",
                        branch_name,
                        short_oid_string(remote_oid)
                    )
                }
                Ok(local_oid)
                    if repo
                        .graph_descendant_of(local_oid, remote_oid)
                        .unwrap_or(false) =>
                {
                    let (ahead, _) = repo
                        .graph_ahead_behind(local_oid, remote_oid)
                        .unwrap_or((0, 0));
                    warnings.push(PlanNote::Switch(SwitchNote::AheadSwitchOnly {
                        name: branch_name.to_string(),
                        ahead,
                        remote: remote_branch.to_string(),
                    }));
                    format!("branch: {} (switch only, ahead)", branch_name)
                }
                Ok(local_oid) => {
                    let (ahead, behind) = repo
                        .graph_ahead_behind(local_oid, remote_oid)
                        .unwrap_or((0, 0));
                    warnings.push(PlanNote::Switch(SwitchNote::DivergedSwitchOnly {
                        name: branch_name.to_string(),
                        remote: remote_branch.to_string(),
                        ahead,
                        behind,
                    }));
                    format!("branch: {} (switch only, diverged)", branch_name)
                }
                Err(e) => {
                    blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                        message: e.to_string(),
                    }));
                    current.head.clone()
                }
            }
        }
    } else {
        current.head.clone()
    };

    let preview_commits = remote_commit
        .as_ref()
        .map(|c| vec![format!("{}  {}", short_oid(c.id()), remote_branch)])
        .unwrap_or_default();

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Switch(SwitchTitle::SwitchToLatest {
            branch: branch_name.to_string(),
            remote: remote_branch.to_string(),
        }),
        current,
        predicted: StateSummary {
            head: predicted_head,
            dirty: "switched (ff-only when safe)".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Switch(SwitchRecovery::SwitchToLatest {
                remote: remote_of_ref(remote_branch).to_string(),
                branch: branch_name.to_string(),
            }),
            commands: vec!["git checkout -".to_string()],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits,
        destructive: false,
    })
}

/// Execute "switch to latest": fetch, then switch to `branch_name`,
/// fast-forwarding it to `remote_branch` when the move is a fast-forward.
pub fn execute_switch_to_latest(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
    remote_branch: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;

    // 1. Fetch the remote so the tracking ref reflects the true latest tip.
    let remote_name = remote_of_ref(remote_branch);
    let fetch_out = run_git(repo_path, &["fetch", remote_name])
        .map_err(|e| GitError::Other(format!("fetch failed: {}", e)))?;
    if fetch_out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            fetch_out.status,
            fetch_out.stderr.trim()
        )));
    }

    // 2. Re-resolve the (now-updated) remote tip.
    let remote_commit = resolve_branch_commit(repo, remote_branch)?;
    let remote_oid = remote_commit.id();
    let local_exists = repo.find_branch(branch_name, BranchType::Local).is_ok();

    if !local_exists {
        // Create a tracking branch at the remote tip and switch to it.
        let mut branch = repo
            .branch(branch_name, &remote_commit, false)
            .map_err(|e| GitError::Other(format!("branch create failed: {}", e.message())))?;
        branch.set_upstream(Some(remote_branch)).ok();
        checkout_branch_tree(repo, branch_name, remote_commit.as_object())?;
        return Ok(());
    }

    // Local branch exists: fast-forward only when safe, otherwise switch as-is.
    let local_oid = local_branch_oid(repo, branch_name)?;
    let can_ff = local_oid != remote_oid
        && repo
            .graph_descendant_of(remote_oid, local_oid)
            .unwrap_or(false);

    if can_ff {
        let refname = format!("refs/heads/{}", branch_name);
        repo.reference(
            &refname,
            remote_oid,
            true,
            &format!(
                "switch-to-latest: fast-forward {} to {}",
                branch_name,
                short_oid_string(remote_oid)
            ),
        )
        .map_err(|e| GitError::Other(format!("branch ref update failed: {}", e.message())))?;
        checkout_branch_tree(repo, branch_name, remote_commit.as_object())?;
    } else {
        // Diverged or ahead — switch to the branch at its current tip, no move.
        let local_commit = repo
            .find_commit(local_oid)
            .map_err(|e| GitError::Other(format!("find commit failed: {}", e.message())))?;
        checkout_branch_tree(repo, branch_name, local_commit.as_object())?;
    }

    // Best-effort: set upstream if the branch has none configured.
    if let Ok(mut branch) = repo.find_branch(branch_name, BranchType::Local) {
        if branch.upstream().is_err() {
            branch.set_upstream(Some(remote_branch)).ok();
        }
    }
    Ok(())
}

/// Check out `obj`'s tree (safe) and move HEAD onto `refs/heads/<branch_name>`.
fn checkout_branch_tree(
    repo: &Repository,
    branch_name: &str,
    obj: &git2::Object,
) -> Result<(), GitError> {
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;
    let refname = format!("refs/heads/{}", branch_name);
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head failed: {}", e.message())))?;
    Ok(())
}
