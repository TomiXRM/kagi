//! Commit diff backend — T011
//!
//! Provides [`commit_changed_files`] which computes the file-level diff for a
//! single commit against its first parent (or the empty tree for root commits).
//!
//! # Design notes
//!
//! * **first-parent diff only** — matches `git show` semantics.  For merge
//!   commits the second+ parents are ignored; only the diff against
//!   `parents[0]` is reported.  This avoids the complexity of combined diffs
//!   and keeps the result predictable for users.
//! * **rename detection** — `find_similar` with `renames(true)` is called
//!   after the raw diff so that A+D pairs are collapsed into R entries.
//!   Without this call, renames appear as an unrelated Added + Deleted pair.
//! * **Copied and other exotic statuses** — mapped to `ChangeKind::Modified`
//!   as a safe default (noted in implementation memo).
//! * **Root commit** — compared against `None` old_tree; libgit2 treats this
//!   as an empty tree, so all files appear as Added.
//!
//! # Panics
//!
//! Never panics.  All path conversions use lossy UTF-8 or `OsString`.

use std::path::PathBuf;

use git2::{Diff, DiffFindOptions, Repository};

use super::{ChangeKind, CommitId, FileStatus, GitError};

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Return the list of files changed in `id` relative to its first parent.
///
/// * For a **root commit** (no parents) all files are returned as
///   [`ChangeKind::Added`].
/// * For a **merge commit** only the diff against `parents[0]` is returned
///   (first-parent diff, same as `git show`).
/// * Rename detection is enabled; a file that was renamed appears as a single
///   [`ChangeKind::Renamed`] entry rather than as a separate Deleted + Added
///   pair.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn commit_changed_files(
    repo: &Repository,
    id: &CommitId,
) -> Result<Vec<FileStatus>, GitError> {
    // 1. Resolve the commit object.
    let oid = git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 2. Resolve the commit's own tree.
    let new_tree = commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 3. Resolve the first parent's tree (None for root commits).
    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let tree = parent
            .tree()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Some(tree)
    } else {
        // Root commit: diff against empty tree.
        None
    };

    // 4. Compute raw diff (old=parent_tree, new=commit_tree).
    //    old_tree=None is equivalent to the empty tree in libgit2.
    let mut diff: Diff<'_> = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 5. Enable rename detection so Added+Deleted pairs collapse into Renamed.
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 6. Convert deltas to FileStatus entries.
    let mut result = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            // Copied and all other statuses are mapped to Modified.
            // Copied: the file is a new copy of another file; semantically
            //         "added but with history", treated as Added by some tools.
            //         We use Modified as a conservative fallback.
            _ => ChangeKind::Modified,
        };

        // For renames the canonical (new) path is in new_file().
        let path = delta
            .new_file()
            .path()
            .map(PathBuf::from)
            .or_else(|| delta.old_file().path().map(PathBuf::from))
            .unwrap_or_default();

        result.push(FileStatus { path, change });
    }

    Ok(result)
}
