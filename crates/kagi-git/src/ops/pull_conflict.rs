//! What a pull would collide with — the plan-time preview and the execute-time
//! guard, computed once (#625).
//!
//! Split out of `ops/pull.rs` (which was already over the 800-line target) on a
//! feature boundary: everything here answers "would this pull run into what the
//! user has locally?", and nothing here mutates the repository.
//!
//! The overlap that matters is *not* commit-to-commit. A fast-forward pull
//! cannot conflict between commits, so [`predict_merge_conflict`] correctly
//! reports nothing for one; what conflicts is the **working-tree content**
//! against the incoming content, which surfaces when the auto-stash is restored
//! after the pull. [`pull_dirty_overlap`] is that calculation, and both the
//! plan (before the user confirms) and execute (as a refusal) read it from
//! here, so the two can never drift apart.

use super::remote_common::resolve_upstream_oid;
use super::*;

/// Dirty paths that the tree change from `old_tree` to `new_tree` also touches.
///
/// Sorted and deduplicated, rendered for display. A rename contributes **both**
/// of its paths on the dirty side, so a pull that renames a file the user is
/// editing under either name is caught.
pub(super) fn pull_dirty_overlap(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<String>, GitError> {
    let status = working_tree_status(repo)?;
    if status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty() {
        return Ok(Vec::new());
    }

    let mut dirty_paths: std::collections::HashSet<PathBuf> =
        status.untracked.iter().cloned().collect();
    for file in status.staged.iter().chain(status.unstaged.iter()) {
        dirty_paths.insert(file.path.clone());
        if let ChangeKind::Renamed { from } = &file.change {
            dirty_paths.insert(from.clone());
        }
    }

    let mut overlapping: Vec<String> = pull_changed_paths_between_trees(repo, old_tree, new_tree)?
        .into_iter()
        .filter(|path| dirty_paths.contains(path))
        .map(|path| path.display().to_string())
        .collect();
    overlapping.sort();
    overlapping.dedup();
    Ok(overlapping)
}

/// Execute-time refusal: the pull must not overwrite what the user is editing.
pub(super) fn ensure_pull_does_not_touch_dirty_paths(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<(), GitError> {
    let overlapping = pull_dirty_overlap(repo, old_tree, new_tree)?;
    if overlapping.is_empty() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "pull would overwrite dirty path(s): {}. Stash or commit those paths, then pull again.",
            overlapping.join(", ")
        )))
    }
}

/// Plan-time preview (#625): dirty paths the **current** upstream tip also
/// changes, for the confirmation modal.
///
/// Local knowledge, like [`predict_merge_conflict`] beside it — `plan_pull`
/// does not fetch. The UI closes that freshness gap by fetching before it opens
/// the dirty-pull confirmation (ADR-0192), which is what makes this preview
/// trustworthy in the flow the user actually sees.
///
/// An unresolvable upstream, an unborn HEAD or an upstream equal to HEAD all
/// mean "nothing incoming", i.e. an empty list rather than an error: a preview
/// that cannot be computed must not fail the plan.
pub(super) fn plan_pull_restore_conflicts(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<Vec<String>, GitError> {
    let Some(head_oid) = repo.head().ok().and_then(|head| head.target()) else {
        return Ok(Vec::new());
    };
    let Ok(upstream_oid) = resolve_upstream_oid(repo, branch_name, remote_name) else {
        return Ok(Vec::new());
    };
    if head_oid == upstream_oid {
        return Ok(Vec::new());
    }
    let tree_of = |oid: git2::Oid| -> Result<git2::Tree<'_>, GitError> {
        repo.find_commit(oid)
            .and_then(|commit| commit.tree())
            .map_err(|e| {
                GitError::Other(format!(
                    "pull restore preview: cannot read tree of {oid}: {}",
                    e.message()
                ))
            })
    };
    let head_tree = tree_of(head_oid)?;
    let upstream_tree = tree_of(upstream_oid)?;
    pull_dirty_overlap(repo, &head_tree, &upstream_tree)
}

/// Every path either side of each delta between two trees.
pub(super) fn pull_changed_paths_between_trees(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<PathBuf>, GitError> {
    let diff = repo
        .diff_tree_to_tree(Some(old_tree), Some(new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for pull safety failed: {}",
                e.message()
            ))
        })?;

    let mut paths = Vec::new();
    for delta in diff.deltas() {
        if let Some(path) = delta.old_file().path() {
            paths.push(path.to_path_buf());
        }
        if let Some(path) = delta.new_file().path() {
            paths.push(path.to_path_buf());
        }
    }
    Ok(paths)
}

/// Attempt an in-memory merge with the current upstream tip to predict conflicts.
///
/// Returns `Ok(true)` if a conflict is predicted, `Ok(false)` if the merge
/// would be clean (or fast-forward), or `Err(...)` if the prediction itself
/// failed (non-fatal — caller ignores and treats as no warning).
pub(super) fn predict_merge_conflict(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<bool, GitError> {
    let head_oid = repo.head().ok().and_then(|r| r.target());
    let upstream_oid = resolve_upstream_oid(repo, branch_name, remote_name).ok();

    let (head_oid, upstream_oid) = match (head_oid, upstream_oid) {
        (Some(h), Some(u)) => (h, u),
        _ => return Ok(false),
    };

    // If already fast-forward or up-to-date, no conflict possible.
    if head_oid == upstream_oid {
        return Ok(false);
    }
    if repo
        .graph_descendant_of(head_oid, upstream_oid)
        .unwrap_or(false)
        || repo
            .graph_descendant_of(upstream_oid, head_oid)
            .unwrap_or(false)
    {
        return Ok(false);
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let upstream_commit = repo
        .find_commit(upstream_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let index = repo
        .merge_commits(&head_commit, &upstream_commit, None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    Ok(index.has_conflicts())
}
