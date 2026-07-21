//! Reset-current-to-HEAD operation pipeline (branch-menu "Advanced /
//! Dangerous" group, "Reset current to this HEAD...").
//!
//! Ref-only, exactly like `history::execute_undo_commit`: moves the current
//! branch's ref via `repo.reference(..., force=true)`. Never touches the
//! index or working tree — no `reset --hard` anywhere (AGENTS.md invariant
//! #3). Semantically a `git reset --soft` restricted to the current branch.

use super::*;
use kagi_domain::plan_note::{ResetNote, ResetRecovery, ResetTitle};

// ────────────────────────────────────────────────────────────
// plan_reset_current_to_head
// ────────────────────────────────────────────────────────────

/// Analyse whether moving the current branch's ref to `target` is safe and
/// return an [`OperationPlan`]. `destructive: true` — the UI must require an
/// armed two-stage confirm (mirrors `discard` / `delete-remote-branch`), not
/// a single click.
///
/// # Blocker conditions
///
/// - HEAD is detached (no current branch to move).
/// - `target` does not exist.
///
/// # Warnings
///
/// - Always: ref-only move — working tree/index are untouched (soft-reset
///   semantics).
/// - If `target` is an ancestor of the current tip: the count of commits
///   that become unreachable from the branch.
/// - If `target` is NOT an ancestor of the current tip (and not equal to
///   it): this reassigns the branch to unrelated history.
pub fn plan_reset_current_to_head(
    repo: &Repository,
    target: &CommitId,
) -> Result<OperationPlan, GitError> {
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

    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = vec![PlanNote::Reset(ResetNote::RefOnlySoftReset)];

    let head_ref = repo.head().ok();
    let branch_name = head_ref
        .as_ref()
        .filter(|r| r.is_branch())
        .and_then(|r| r.shorthand().ok())
        .map(str::to_string);
    if branch_name.is_none() {
        blockers.push(PlanNote::Reset(ResetNote::DetachedHead));
    }

    let target_oid = git2::Oid::from_str(&target.0).ok();
    let target_exists = target_oid.is_some_and(|oid| repo.find_commit(oid).is_ok());
    if !target_exists {
        blockers.push(PlanNote::Reset(ResetNote::CommitMissing {
            sha: target.short().to_string(),
        }));
    }

    let mut to_display = target.short().to_string();
    if let (Some(branch), Some(target_oid), Some(head_ref)) =
        (branch_name.as_ref(), target_oid, head_ref.as_ref())
    {
        if let Some(head_oid) = head_ref.target() {
            to_display = short_oid(target_oid);
            if head_oid != target_oid {
                if repo
                    .graph_descendant_of(head_oid, target_oid)
                    .unwrap_or(false)
                {
                    let count = repo
                        .graph_ahead_behind(head_oid, target_oid)
                        .map(|(ahead, _)| ahead)
                        .unwrap_or(0);
                    if count > 0 {
                        warnings.push(PlanNote::Reset(ResetNote::AbandonsCommits {
                            branch: branch.clone(),
                            count,
                        }));
                    }
                } else {
                    warnings.push(PlanNote::Reset(ResetNote::TargetNotAncestor {
                        branch: branch.clone(),
                    }));
                }
            }
        }
    }

    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let recovery = branch_name.as_ref().and_then(|branch| {
        head_ref
            .as_ref()
            .and_then(|r| r.target())
            .map(|from_oid| PlanRecovery {
                kind: RecoveryKind::Reset(ResetRecovery::ResetCurrentToHead {
                    branch: branch.clone(),
                    from: from_oid.to_string(),
                }),
                commands: vec![format!("git update-ref refs/heads/{} {}", branch, from_oid)],
            })
    });

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Reset(ResetTitle::ResetCurrentToHead {
            branch: branch_name.unwrap_or_default(),
            to: to_display,
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
        destructive: true,
    })
}

fn short_oid(oid: git2::Oid) -> String {
    oid.to_string()[..8.min(oid.to_string().len())].to_string()
}

// ────────────────────────────────────────────────────────────
// execute_reset_current_to_head
// ────────────────────────────────────────────────────────────

/// Move the current branch's ref to point at `target`. Ref-only —
/// `force=true` on the ref update only, never on the index or working tree
/// (there is no `reset --hard` path in this codebase).
pub fn execute_reset_current_to_head(repo: &Repository, target: &CommitId) -> Result<(), GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is not on a branch. Reset current-to-HEAD requires an attached HEAD.".to_string(),
        ));
    }
    let branch_refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD ref name failed: {}", e.message())))?
        .to_string();

    let target_oid = git2::Oid::from_str(&target.0).map_err(|e| {
        GitError::Other(format!("invalid commit id '{}': {}", target.0, e.message()))
    })?;
    repo.find_commit(target_oid)
        .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;

    let log_msg = format!(
        "reset-current-to-head: move {} to {}",
        branch_refname, target_oid
    );
    repo.reference(&branch_refname, target_oid, true, &log_msg)
        .map_err(|e| {
            GitError::Other(format!("branch ref update (reset) failed: {}", e.message()))
        })?;

    Ok(())
}
