//! Rebase-current-onto operation pipeline (branch-menu "Integrate" group,
//! "Rebase current branch onto <target>").
//!
//! Starts the rebase via the real `git` CLI (`run_git`), same as every other
//! CLI-driven op in this codebase (push/pull/fetch) — reimplementing git's
//! rebase step machine on top of libgit2's lower-level `Rebase` API would
//! duplicate real git's own sequencer for no benefit. A conflict is not a
//! failure: [`RebaseOutcome::Conflicted`] lets the existing conflict-mode
//! detection (`conflicts::detect_conflict_session`) and the fixed-in-this-PR
//! `execute_conflict_continue` sequencer-advance take over from there.

use super::*;
use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};

// ────────────────────────────────────────────────────────────
// plan_rebase_current_onto
// ────────────────────────────────────────────────────────────

/// Analyse whether rebasing the current branch onto `onto` is safe and
/// return an [`OperationPlan`]. Guarded-class (ADR-0004): dirty tree and
/// detached HEAD block; a predicted conflict is not knowable ahead of time
/// for a multi-commit rebase (unlike merge's single in-memory merge), so it
/// is always surfaced as an unconditional warning instead.
///
/// # Blocker conditions
///
/// - HEAD is detached or unborn.
/// - The working tree has uncommitted changes.
/// - `onto` does not resolve to a valid ref or commit.
/// - `onto` already equals the branch's current tip (nothing to replay).
pub fn plan_rebase_current_onto(repo: &Repository, onto: &str) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let head_display = head.display();
    let dirty_display = if status.is_dirty() {
        "dirty".to_string()
    } else {
        "clean".to_string()
    };
    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display.clone(),
    };
    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    let warnings: Vec<PlanNote> = vec![PlanNote::Rebase(RebaseNote::MayConflict)];

    let branch_name = match &head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    if branch_name.is_none() {
        blockers.push(PlanNote::Rebase(RebaseNote::DetachedHead));
    }
    if status.is_dirty() {
        blockers.push(PlanNote::Rebase(RebaseNote::DirtyWorkingTree));
    }

    let onto_oid = repo
        .revparse_single(onto)
        .ok()
        .map(|obj| obj.id())
        .and_then(|oid| repo.find_commit(oid).ok().map(|_| oid));
    if onto_oid.is_none() {
        blockers.push(PlanNote::Rebase(RebaseNote::InvalidOnto {
            onto: onto.to_string(),
        }));
    }

    let mut recovery = None;
    if let (Some(branch), Some(onto_oid)) = (branch_name.as_ref(), onto_oid) {
        if let Ok(branch_ref) = repo.find_branch(branch, BranchType::Local) {
            if let Some(head_oid) = branch_ref.get().target() {
                if head_oid == onto_oid {
                    blockers.push(PlanNote::Rebase(RebaseNote::AlreadyUpToDate {
                        branch: branch.clone(),
                        onto: onto.to_string(),
                    }));
                }
                recovery = Some(PlanRecovery {
                    kind: RecoveryKind::Rebase(RebaseRecovery::RebaseCurrentOnto {
                        branch: branch.clone(),
                        from: head_oid.to_string(),
                    }),
                    commands: vec![format!("git update-ref refs/heads/{} {}", branch, head_oid)],
                });
            }
        }
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Rebase(RebaseTitle::RebaseCurrentOnto {
            branch: branch_name.unwrap_or_default(),
            onto: onto.to_string(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_rebase_current_onto
// ────────────────────────────────────────────────────────────

/// Run `git rebase <onto>`. A conflict (non-zero exit, repo left in
/// `RepositoryState::Rebase*`) is reported as [`RebaseOutcome::Conflicted`],
/// not an `Err` — see the module doc.
pub fn execute_rebase_current_onto(
    repo: &Repository,
    repo_path: &Path,
    onto: &str,
) -> Result<RebaseOutcome, GitError> {
    let out = run_git(repo_path, &["rebase", onto])
        .map_err(|e| GitError::Other(format!("rebase failed to start: {}", e)))?;

    if !matches!(repo.state(), git2::RepositoryState::Clean) {
        return Ok(RebaseOutcome::Conflicted);
    }

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "rebase failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let head_oid = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| GitError::Other("HEAD has no target after rebase".to_string()))?;
    Ok(RebaseOutcome::Completed {
        head: CommitId(head_oid.to_string()),
    })
}
