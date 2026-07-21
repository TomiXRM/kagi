//! Delete-remote-branch operation pipeline (branch-menu "Advanced / Dangerous"
//! group). Deletes a ref on the remote via `git push <remote> --delete
//! <branch>` (shelled out through `run_git`, mirroring `execute_push`) —
//! never touches HEAD, the index, or the local branch of the same name.

use super::*;
use kagi_domain::plan_note::{RemoteBranchNote, RemoteBranchRecovery, RemoteBranchTitle};

/// Split `"origin/feature/x"` into `("origin", "feature/x")`.
fn split_remote_branch(remote_branch: &str) -> Option<(&str, &str)> {
    remote_branch.split_once('/')
}

// ────────────────────────────────────────────────────────────
// plan_delete_remote_branch
// ────────────────────────────────────────────────────────────

/// Analyse whether deleting the remote branch `remote_branch` (e.g.
/// `"origin/feature/x"`) is safe and return an [`OperationPlan`].
///
/// This is a **Destructive-class** operation (ADR-0009): the remote ref is
/// removed and, unlike a local branch delete, kagi cannot verify "merged"
/// status against the remote's other branches before doing so. `destructive:
/// true` — the UI must require an armed two-stage confirm (mirrors
/// `discard`), not a single click.
///
/// # Blocker conditions
///
/// - `remote_branch` has no `/` (can't split remote from branch name).
/// - The remote-tracking ref `refs/remotes/<remote>/<branch>` does not exist
///   locally (nothing to delete from kagi's point of view).
///
/// # Warnings
///
/// - Always: deleting the remote branch does not touch a same-named local
///   branch (it becomes upstream-less).
pub fn plan_delete_remote_branch(
    repo: &Repository,
    remote_branch: &str,
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
    let predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    let (remote, branch) = match split_remote_branch(remote_branch) {
        Some(parts) => parts,
        None => {
            blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                message: format!("'{}' is not a <remote>/<branch> name", remote_branch),
            }));
            ("", "")
        }
    };

    if !remote.is_empty()
        && repo
            .find_reference(&format!("refs/remotes/{}/{}", remote, branch))
            .is_err()
    {
        blockers.push(PlanNote::RemoteBranch(RemoteBranchNote::NotFound {
            remote: remote.to_string(),
            branch: branch.to_string(),
        }));
    }

    let mut warnings = Vec::new();
    if !branch.is_empty() {
        warnings.push(PlanNote::RemoteBranch(
            RemoteBranchNote::LocalBranchUntouched {
                local_name: branch.to_string(),
            },
        ));
    }

    let sha = repo
        .find_reference(&format!("refs/remotes/{}/{}", remote, branch))
        .ok()
        .and_then(|r| r.target())
        .map(|oid| oid.to_string()[..8].to_string())
        .unwrap_or_default();

    let recovery = PlanRecovery {
        kind: RecoveryKind::RemoteBranch(RemoteBranchRecovery::DeleteRemoteBranch {
            remote: remote.to_string(),
            branch: branch.to_string(),
            sha: sha.clone(),
        }),
        commands: vec![format!("git push {} {}:refs/heads/{}", remote, sha, branch)],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::RemoteBranch(RemoteBranchTitle::DeleteRemoteBranch {
            remote: remote.to_string(),
            branch: branch.to_string(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
    })
}

// ────────────────────────────────────────────────────────────
// execute_delete_remote_branch
// ────────────────────────────────────────────────────────────

/// Delete the remote branch `remote_branch` (e.g. `"origin/feature/x"`) via
/// `git push <remote> --delete <branch>`.
///
/// **force is never used** — a delete refspec push is not a force-push and
/// is rejected by the remote like any other non-fast-forward push would be
/// only if branch protection denies deletion, which surfaces as a normal
/// `GitError`.
pub fn execute_delete_remote_branch(repo_path: &Path, remote_branch: &str) -> Result<(), GitError> {
    let (remote, branch) = split_remote_branch(remote_branch).ok_or_else(|| {
        GitError::Other(format!(
            "'{}' is not a <remote>/<branch> name",
            remote_branch
        ))
    })?;

    let out = run_git(repo_path, &["push", remote, "--delete", branch])
        .map_err(|e| GitError::Other(format!("push --delete failed: {}", e)))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "push --delete failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    Ok(())
}
