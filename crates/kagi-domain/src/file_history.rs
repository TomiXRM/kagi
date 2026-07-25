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

impl FileHistory {
    /// The entry whose commit has `hash` as its full SHA, if any.
    ///
    /// Callers needing that commit's OWN historical path must read
    /// `entry.change.path_after` from the result — never assume it equals
    /// `self.current_path` (a rename between then and now means it won't).
    pub fn entry_by_hash(&self, hash: &str) -> Option<&FileHistoryEntry> {
        self.entries
            .iter()
            .find(|e| e.commit.as_ref().is_some_and(|c| c.full_hash == hash))
    }
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

/// A file's full text content as of a specific commit (read-only "snapshot"
/// view — not the write-path `RepoSnapshot`, which is unrelated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSnapshotContent {
    /// `None` when the blob is binary (no text to display).
    pub content: Option<String>,
    pub is_binary: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_entry(hash: &str, path_after: &str) -> FileHistoryEntry {
        FileHistoryEntry {
            kind: FileHistoryEntryKind::Commit,
            commit: Some(CommitSummary {
                full_hash: hash.to_string(),
                short_hash: hash[..7].to_string(),
                subject: "subject".to_string(),
                body: None,
                author_name: "Test".to_string(),
                author_email: "test@example.com".to_string(),
                author_date: "2026-01-01T00:00:00+09:00".to_string(),
                committer_name: "Test".to_string(),
                committer_date: "2026-01-01T00:00:00+09:00".to_string(),
            }),
            change: FileChangeSummary {
                change_type: FileChangeType::Modified,
                path_before: None,
                path_after: PathBuf::from(path_after),
                insertions: Some(1),
                deletions: Some(1),
                is_binary: false,
            },
        }
    }

    /// Regression test for the bug this method exists to prevent: a commit
    /// that predates a rename must resolve to ITS OWN path, not the file's
    /// current one — a caller that instead assumed `current_path` for every
    /// entry (as the Editor Workspace's History tab originally did) would
    /// look up the wrong tree path and silently find nothing.
    #[test]
    fn entry_by_hash_resolves_the_pre_rename_path() {
        let history = FileHistory {
            current_path: PathBuf::from("new.rs"),
            entries: vec![
                commit_entry("aaaa000post", "new.rs"),
                commit_entry("bbbb111pre", "old.rs"),
            ],
        };

        let post = history.entry_by_hash("aaaa000post").expect("post entry");
        assert_eq!(post.change.path_after, PathBuf::from("new.rs"));

        let pre = history.entry_by_hash("bbbb111pre").expect("pre entry");
        assert_eq!(pre.change.path_after, PathBuf::from("old.rs"));
        assert_ne!(pre.change.path_after, history.current_path);
    }

    #[test]
    fn entry_by_hash_unknown_hash_returns_none() {
        let history = FileHistory {
            current_path: PathBuf::from("new.rs"),
            entries: vec![commit_entry("aaaa000post", "new.rs")],
        };
        assert!(history.entry_by_hash("does-not-exist").is_none());
    }
}
