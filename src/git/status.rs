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
use std::path::PathBuf;

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
                    .unwrap_or_default()
            } else {
                entry_path(&entry).unwrap_or_default()
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

            let path = entry_path(&entry).unwrap_or_default();
            result.unstaged.push(FileStatus { path, change: kind });
        }

        // ── Untracked ────────────────────────────────────────────────────
        if s.contains(git2::Status::WT_NEW) {
            if let Some(path) = entry_path(&entry) {
                result.untracked.push(path);
            }
        }
    }

    Ok(result)
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Extract the file path from a status entry.
///
/// `StatusEntry::path()` returns `Result<&str, Error>`. On success we use the
/// UTF-8 string directly; on failure (non-UTF-8 path) we fall back to the raw
/// bytes from `path_bytes()`.
fn entry_path(entry: &git2::StatusEntry<'_>) -> Option<PathBuf> {
    // path() returns Result<&str, Error>; use it when the path is valid UTF-8.
    if let Ok(p) = entry.path() {
        return Some(PathBuf::from(p));
    }
    // path_bytes() returns &[u8]; try to interpret as UTF-8.
    std::str::from_utf8(entry.path_bytes())
        .ok()
        .map(PathBuf::from)
}
