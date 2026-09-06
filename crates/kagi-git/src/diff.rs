//! Commit diff backend — T011 / T012
//!
//! Provides:
//! - [`commit_changed_files`] — file-level diff (T011)
//! - [`commit_file_diff`] — unified diff for a single file (T012)
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

use std::path::{Path, PathBuf};

use git2::{Diff, DiffFindOptions, DiffOptions, Repository};
use kagi_domain::status::{ChangeKind, FileStatus};

use super::{CommitId, GitError};

// ────────────────────────────────────────────────────────────
// Diff models (architecture.md §3)
// ────────────────────────────────────────────────────────────
//
// `DiffLineKind`, `DiffLine`, `Hunk`, and `FileDiff` now live in the pure
// `kagi-domain` crate (ADR-0072). They are re-exported here so existing
// `kagi::git::*` paths keep resolving while the git2-backed diff functions
// below construct them.
pub use kagi_domain::diff::{DiffLine, DiffLineKind, FileDiff, Hunk};

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
pub fn commit_changed_files(repo: &Repository, id: &CommitId) -> Result<Vec<FileStatus>, GitError> {
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

/// Return the unified diff for a single file changed in commit `id`.
///
/// The `path` argument should be the **new-side path** of the file (i.e. the
/// path reported by [`commit_changed_files`]).  For deleted files, the old
/// path is used as a fallback by libgit2's pathspec matching.
///
/// The implementation:
/// 1. Recomputes the first-parent tree diff with a `pathspec` filter so only
///    the requested file is included.
/// 2. Iterates over `diff.num_deltas()` and picks the delta whose
///    `new_file().path()` (or `old_file().path()` for deletions) matches
///    `path`.  This is more robust than assuming `delta[0]` when the
///    pathspec might match multiple entries (e.g. directory prefixes).
/// 3. Calls `Patch::from_diff(&diff, idx)` on the matched delta.
/// 4. Extracts hunks and lines via `Patch::hunk()` / `Patch::line_in_hunk()`.
///
/// Binary files are returned with `is_binary = true` and `hunks` empty.
/// EOF-marker lines (`=`, `>`, `<`) are folded into `Context` / `Added` /
/// `Removed` so the display is clean.
///
/// Line content is decoded as lossy UTF-8 (never panics on arbitrary bytes).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn commit_file_diff(
    repo: &Repository,
    id: &CommitId,
    path: &Path,
) -> Result<FileDiff, GitError> {
    // 1. Resolve the commit.
    let oid = git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let new_tree = commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Some(
            parent
                .tree()
                .map_err(|e| GitError::Other(e.message().to_string()))?,
        )
    } else {
        None
    };

    // 2. Build diff with a pathspec so only the target file is included.
    //    pathspec uses the new-path of the file.
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(super::path_to_pathspec(path)?);
    // #292: literal pathspec match — no glob interpretation of [ ] * ? etc.
    diff_opts.disable_pathspec_match(true);

    let mut diff: Diff<'_> = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    diff_to_file_diff(&mut diff, path)
}

/// Return files changed between two commits (`a` → `b`) without touching the
/// working tree, index, refs, or repository state.
pub fn compare_commits(
    repo: &Repository,
    a: &CommitId,
    b: &CommitId,
) -> Result<Vec<FileStatus>, GitError> {
    let a_commit = find_commit(repo, a)?;
    let b_commit = find_commit(repo, b)?;
    let a_tree = a_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let b_tree = b_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut diff = repo
        .diff_tree_to_tree(Some(&a_tree), Some(&b_tree), None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_file_statuses(&mut diff)
}

/// Return the file diff for a single path between two commits (`a` → `b`).
pub fn compare_file_diff(
    repo: &Repository,
    a: &CommitId,
    b: &CommitId,
    path: &Path,
) -> Result<FileDiff, GitError> {
    let a_commit = find_commit(repo, a)?;
    let b_commit = find_commit(repo, b)?;
    let a_tree = a_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let b_tree = b_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(super::path_to_pathspec(path)?);
    diff_opts.disable_pathspec_match(true); // #292: literal match.
    let mut diff = repo
        .diff_tree_to_tree(Some(&a_tree), Some(&b_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_file_diff(&mut diff, path)
}

/// Return files changed between a commit and the current working tree,
/// including staged, unstaged, and untracked changes. Read-only.
pub fn compare_commit_to_workdir(
    repo: &Repository,
    a: &CommitId,
) -> Result<Vec<FileStatus>, GitError> {
    let a_commit = find_commit(repo, a)?;
    let a_tree = a_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut diff_opts = workdir_compare_options();
    let mut diff = repo
        .diff_tree_to_workdir_with_index(Some(&a_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_file_statuses(&mut diff)
}

/// Return a single-file diff between a commit and the current working tree,
/// including staged, unstaged, and untracked changes. Read-only.
pub fn compare_commit_to_workdir_file_diff(
    repo: &Repository,
    a: &CommitId,
    path: &Path,
) -> Result<FileDiff, GitError> {
    let a_commit = find_commit(repo, a)?;
    let a_tree = a_commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut diff_opts = workdir_compare_options();
    diff_opts.pathspec(super::path_to_pathspec(path)?);
    diff_opts.disable_pathspec_match(true); // #292: literal match.
    let mut diff = repo
        .diff_tree_to_workdir_with_index(Some(&a_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_file_diff(&mut diff, path)
}

fn find_commit<'repo>(
    repo: &'repo Repository,
    id: &CommitId,
) -> Result<git2::Commit<'repo>, GitError> {
    let oid = git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
    repo.find_commit(oid)
        .map_err(|e| GitError::Other(e.message().to_string()))
}

fn workdir_compare_options() -> DiffOptions {
    let mut opts = DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    opts
}

fn diff_to_file_statuses(diff: &mut Diff<'_>) -> Result<Vec<FileStatus>, GitError> {
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut result = Vec::new();
    for delta in diff.deltas() {
        result.push(file_status_from_delta(&delta));
    }
    Ok(result)
}

fn file_status_from_delta(delta: &git2::DiffDelta<'_>) -> FileStatus {
    use git2::Delta;
    let old_path = delta.old_file().path().map(PathBuf::from);
    let change = match delta.status() {
        Delta::Added | Delta::Untracked => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Modified => ChangeKind::Modified,
        Delta::Renamed => ChangeKind::Renamed {
            from: old_path.clone().unwrap_or_default(),
        },
        Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    };

    let path = delta
        .new_file()
        .path()
        .map(PathBuf::from)
        .or_else(|| delta.old_file().path().map(PathBuf::from))
        .unwrap_or_default();

    FileStatus { path, change }
}

fn diff_to_file_diff(diff: &mut Diff<'_>, path: &Path) -> Result<FileDiff, GitError> {
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    patch_to_file_diff(diff, path)
}

/// Convert the selected delta without changing the caller's rename/pathspec policy.
/// Commit, compare and staging diffs share this conversion; conflicted worktree
/// diffs use the same hunk decoder with their explicit ours-to-worktree patch.
pub(crate) fn patch_to_file_diff(diff: &git2::Diff<'_>, path: &Path) -> Result<FileDiff, GitError> {
    // Never fall back to delta 0: a missing exact match must not show another file.
    let Some(delta_idx) = diff.deltas().position(|delta| {
        delta.new_file().path() == Some(path) || delta.old_file().path() == Some(path)
    }) else {
        return Ok(FileDiff {
            old_path: None,
            new_path: Some(path.to_path_buf()),
            change: ChangeKind::Modified,
            hunks: vec![],
            is_binary: false,
        });
    };

    let patch = git2::Patch::from_diff(diff, delta_idx)
        .map_err(|e| GitError::Other(format!("Patch::from_diff failed: {}", e.message())))?;
    // libgit2 inspects content lazily. Re-read the delta after materializing the
    // patch rather than using flags copied before inspection or guessing from
    // zero hunks (which also occur for empty files, renames and mode-only changes).
    let delta = diff
        .get_delta(delta_idx)
        .ok_or_else(|| GitError::Other("diff delta disappeared".to_string()))?;
    let old_path = delta.old_file().path().map(PathBuf::from);
    let new_path = delta.new_file().path().map(PathBuf::from);
    let change = match delta.status() {
        git2::Delta::Added | git2::Delta::Untracked => ChangeKind::Added,
        git2::Delta::Deleted => ChangeKind::Deleted,
        git2::Delta::Renamed => ChangeKind::Renamed {
            from: old_path.clone().unwrap_or_default(),
        },
        git2::Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    };
    let is_binary = delta.old_file().is_binary() || delta.new_file().is_binary();
    let hunks = match patch {
        Some(patch) if !is_binary => patch_hunks(&patch)?,
        _ => Vec::new(),
    };
    Ok(FileDiff {
        old_path,
        new_path,
        change,
        hunks,
        is_binary,
    })
}

/// The hunk/line extraction shared by every `git2::Patch` → [`FileDiff`] path.
pub(crate) fn patch_hunks(patch: &git2::Patch<'_>) -> Result<Vec<Hunk>, GitError> {
    let num_hunks = patch.num_hunks();
    let mut hunks = Vec::with_capacity(num_hunks);

    for h_idx in 0..num_hunks {
        let (diff_hunk, line_count) = patch.hunk(h_idx).map_err(|e| {
            GitError::Other(format!("patch.hunk({}) failed: {}", h_idx, e.message()))
        })?;

        let old_range = (diff_hunk.old_start(), diff_hunk.old_lines());
        let new_range = (diff_hunk.new_start(), diff_hunk.new_lines());

        let mut lines = Vec::with_capacity(line_count);

        for l_idx in 0..line_count {
            let diff_line = patch.line_in_hunk(h_idx, l_idx).map_err(|e| {
                GitError::Other(format!(
                    "patch.line_in_hunk({},{}) failed: {}",
                    h_idx,
                    l_idx,
                    e.message()
                ))
            })?;

            let kind = match diff_line.origin() {
                '+' | '>' => DiffLineKind::Added,
                '-' | '<' => DiffLineKind::Removed,
                _ => DiffLineKind::Context,
            };

            let content = String::from_utf8_lossy(diff_line.content()).into_owned();

            lines.push(DiffLine {
                kind,
                content,
                old_lineno: diff_line.old_lineno(),
                new_lineno: diff_line.new_lineno(),
            });
        }

        hunks.push(Hunk {
            old_range,
            new_range,
            lines,
        });
    }

    Ok(hunks)
}
