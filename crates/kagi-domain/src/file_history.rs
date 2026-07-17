//! File History models (ADR-0089, ADR-0108, ADR-0121 C3).
//!
//! Pure data describing the per-commit change history of a single file: the
//! history list, its entries (a synthetic WIP row or a real commit), and the
//! per-entry change summary. Moved here from `kagi-git` (which re-exports them
//! as a shim, per the repo naming convention) so the Git-free
//! `kagi-ui-file-history` crate can render them without touching the backend.
//!
//! **Name note (ADR-0108):** `kagi_domain::history` is the *operation* history
//! (undo/redo stack); this module is the *file* history — the per-path commit
//! log. Different concepts, deliberately different module names.
//!
//! The collection itself (the `git log` orchestration and parsing) stays in
//! `kagi-git::file_history` — it shells out to git and is not pure.

use std::path::PathBuf;

/// The collected history of a single file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistory {
    /// The file's current (most recent) path, repo-relative.
    pub current_path: PathBuf,
    /// Entries, WIP first (if any), then commits newest-first.
    pub entries: Vec<FileHistoryEntry>,
}

/// A single row in the history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHistoryEntry {
    /// Whether this is the synthetic WIP row or a real commit.
    pub kind: FileHistoryEntryKind,
    /// Commit metadata; `None` for the WIP entry.
    pub commit: Option<CommitSummary>,
    /// The change this entry made to the file.
    pub change: FileChangeSummary,
}

/// Discriminates a real commit row from the synthetic working-tree row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileHistoryEntryKind {
    /// Uncommitted working-tree / index / untracked change.
    Wip,
    /// A committed change.
    Commit,
}

/// Commit metadata for a history row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub full_hash: String,
    pub short_hash: String,
    pub subject: String,
    pub body: Option<String>,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
    pub committer_name: String,
    pub committer_date: String,
}

/// How the file changed in a given entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChangeSummary {
    pub change_type: FileChangeType,
    /// Previous path for renames/copies; `None` otherwise.
    pub path_before: Option<PathBuf>,
    /// The file's path at this entry.
    pub path_after: PathBuf,
    /// Added lines; `None` when unknown (binary).
    pub insertions: Option<u32>,
    /// Removed lines; `None` when unknown (binary).
    pub deletions: Option<u32>,
    /// Whether git reported this as a binary change.
    pub is_binary: bool,
}

/// The kind of change recorded for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Unknown,
}
