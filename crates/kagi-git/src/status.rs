//! Working tree status — T003
//!
//! This module provides the domain model for working tree status and the
//! backend function that populates it using `git2`.
//!
//! # Domain model (architecture.md §3)
//!
//! ```text
//! WorkingTreeStatus
//!   staged:     Vec<FileStatus>  – changes staged in the index (INDEX_*)
//!   unstaged:   Vec<FileStatus>  – changes in the workdir (WT_*)
//!   untracked:  Vec<PathBuf>     – new files not yet tracked (WT_NEW)
//!   conflicted: Vec<PathBuf>     – files with merge conflicts (CONFLICTED)
//! ```
//!
//! Files that have both an index and a workdir change appear in **both**
//! `staged` and `unstaged`.

use git2::{Repository, StatusOptions};
use std::path::{Path, PathBuf};

use super::GitError;

// ────────────────────────────────────────────────────────────
// Domain model
// ────────────────────────────────────────────────────────────
//
// `ChangeKind`, `FileStatus`, and `WorkingTreeStatus` now live in the pure
// `kagi-domain` crate (ADR-0072). They are re-exported here so existing
// `kagi::git::{ChangeKind, FileStatus, WorkingTreeStatus}` paths keep
// resolving while the git2-backed `working_tree_status` below constructs them.
pub use kagi_domain::status::{ChangeKind, FileStatus, WorkingTreeStatus};

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Query the working tree status of `repo` and return a [`WorkingTreeStatus`].
///
/// # Behaviour
///
/// * Untracked files are included and untracked directories are traversed
///   recursively (`recurse_untracked_dirs`).
/// * Ignored files are **excluded**.
/// * Staged renames are detected via `renames_head_to_index`.
/// * Files that appear both staged and unstaged are listed in both groups.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any `git2` failure.
pub fn working_tree_status(repo: &Repository) -> Result<WorkingTreeStatus, GitError> {
    let mut opts = StatusOptions::new();
    opts.include_ignored(false)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut result = WorkingTreeStatus::default();
    let workdir = repo.workdir().map(|p| p.to_path_buf());

    for entry in statuses.iter() {
        let s = entry.status();

        // ── Conflicted ──────────────────────────────────────────────────
        if s.contains(git2::Status::CONFLICTED) {
            if let Some(path) = entry_path(&entry) {
                result.conflicted.push(path);
            }
            // Skip further classification for this entry.
            continue;
        }

        // ── Staged (index side) ─────────────────────────────────────────
        if s.contains(git2::Status::INDEX_NEW)
            || s.contains(git2::Status::INDEX_MODIFIED)
            || s.contains(git2::Status::INDEX_DELETED)
            || s.contains(git2::Status::INDEX_RENAMED)
            || s.contains(git2::Status::INDEX_TYPECHANGE)
        {
            let kind = if s.contains(git2::Status::INDEX_RENAMED) {
                // For a rename, `head_to_index()` holds both old and new paths.
                let from = entry
                    .head_to_index()
                    .and_then(|d| d.old_file().path())
                    .map(PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            } else if s.contains(git2::Status::INDEX_NEW) {
                ChangeKind::Added
            } else if s.contains(git2::Status::INDEX_DELETED) {
                ChangeKind::Deleted
            } else if s.contains(git2::Status::INDEX_TYPECHANGE) {
                ChangeKind::TypeChange
            } else {
                ChangeKind::Modified
            };

            // For renames, use the *new* path (new_file of head_to_index).
            let path = if s.contains(git2::Status::INDEX_RENAMED) {
                entry
                    .head_to_index()
                    .and_then(|d| d.new_file().path())
                    .map(PathBuf::from)
                    .or_else(|| entry_path(&entry))
            } else {
                entry_path(&entry)
            };
            // #293: never fabricate an empty PathBuf — skip an entry whose path
            // can't be built (non-UTF-8 on non-Unix) so it can't vanish from
            // the overwrite guards under a bogus "" key.
            let Some(path) = path else {
                continue;
            };

            result.staged.push(FileStatus { path, change: kind });
        }

        // ── Unstaged (workdir side) ──────────────────────────────────────
        // WT_NEW is handled separately as "untracked".
        if s.contains(git2::Status::WT_MODIFIED)
            || s.contains(git2::Status::WT_DELETED)
            || s.contains(git2::Status::WT_RENAMED)
            || s.contains(git2::Status::WT_TYPECHANGE)
        {
            let kind = if s.contains(git2::Status::WT_RENAMED) {
                let from = entry
                    .index_to_workdir()
                    .and_then(|d| d.old_file().path())
                    .map(PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            } else if s.contains(git2::Status::WT_DELETED) {
                ChangeKind::Deleted
            } else if s.contains(git2::Status::WT_TYPECHANGE) {
                ChangeKind::TypeChange
            } else {
                ChangeKind::Modified
            };

            let Some(path) = entry_path(&entry) else {
                continue;
            };
            result.unstaged.push(FileStatus { path, change: kind });
        }

        // ── Untracked ────────────────────────────────────────────────────
        if s.contains(git2::Status::WT_NEW) {
            if let Some(path) = entry_path(&entry) {
                // Skip nested git repositories / linked worktrees (a directory
                // containing a `.git`). git surfaces such a dir as a single
                // untracked entry, but it is a whole separate checkout (often
                // thousands of files) — showing it as "untracked" is noise, and
                // tools like Claude Code create worktrees under
                // `.claude/worktrees/`. Treat them as not part of this repo.
                if is_nested_git_dir(workdir.as_deref(), &path) {
                    continue;
                }
                result.untracked.push(path);
            }
        }
    }

    Ok(result)
}

/// Every tracked file (including unmodified) plus every untracked-but-not-
/// ignored file, sorted and repo-relative (T-WS-EDITOR-004: Editor Workspace
/// "All files" tree source, ADR-0120 §4).
///
/// ponytail: one index walk + one `statuses` call (`include_unmodified`
/// reuses the exact same machinery as `working_tree_status` above — no
/// hand-rolled `.gitignore` parsing). Eager and full, no lazy per-directory
/// expansion; if that's too slow on huge repos, lazy expansion is
/// T-WS-EDITOR-003's remaining scope.
pub fn worktree_files(repo: &Repository) -> Result<Vec<PathBuf>, GitError> {
    let mut opts = StatusOptions::new();
    opts.include_ignored(false)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_unmodified(true);

    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let workdir = repo.workdir().map(|p| p.to_path_buf());
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in statuses.iter() {
        let Some(path) = entry_path(&entry) else {
            continue;
        };
        if is_nested_git_dir(workdir.as_deref(), &path) {
            continue;
        }
        files.push(path);
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Whether `rel` (relative to `workdir`) is a nested git repository or linked
/// worktree — i.e. a directory that itself contains a `.git` (a real `.git`
/// directory for a nested clone, or a `.git` *file* for a linked worktree).
fn is_nested_git_dir(workdir: Option<&Path>, rel: &Path) -> bool {
    match workdir {
        Some(wd) => wd.join(rel).join(".git").exists(),
        None => false,
    }
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Extract the file path from a status entry, byte-faithfully (#293).
///
/// `StatusEntry::path()` returns `Result<&str, Error>` and fails for a
/// non-UTF-8 name. The old fallback re-ran the *same* UTF-8 check on
/// `path_bytes()`, so a non-UTF-8 name still collapsed to `None` and the
/// caller's `unwrap_or_default()` turned it into an **empty** `PathBuf` — which
/// silently dropped the file from every overwrite guard.
///
/// On Unix the path is now built from the raw bytes via `OsStrExt::from_bytes`,
/// which never loses information. On non-Unix targets there is no lossless
/// bytes→path bridge, so a non-UTF-8 name yields `None` and the caller skips
/// the entry (never a bogus empty path). Non-Unix non-UTF-8 handling is out of
/// scope (#293).
fn entry_path(entry: &git2::StatusEntry<'_>) -> Option<PathBuf> {
    // path() returns Result<&str, Error>; use it when the path is valid UTF-8.
    if let Ok(p) = entry.path() {
        return Some(PathBuf::from(p));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(
            entry.path_bytes(),
        )))
    }
    #[cfg(not(unix))]
    {
        eprintln!(
            "[kagi-git] status: skipping entry with non-UTF-8 path (unsupported on this platform)"
        );
        None
    }
}
