//! Diff domain models — pure data, no git2.
//!
//! The git2-backed functions that compute these models live in the git-backend
//! layer (`kagi::git::diff`).

use std::path::PathBuf;

use crate::status::ChangeKind;

/// The kind of a diff line: unchanged context, a newly-added line, or a
/// removed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Unchanged context line (present in both old and new).
    Context,
    /// Line that was added in the new version.
    Added,
    /// Line that was removed from the old version.
    Removed,
}

/// One line in a diff hunk.
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// The role of this line (context, added, or removed).
    pub kind: DiffLineKind,
    /// The line content, including any trailing newline, as lossy UTF-8.
    pub content: String,
    /// 1-based line number in the old file, if applicable.
    pub old_lineno: Option<u32>,
    /// 1-based line number in the new file, if applicable.
    pub new_lineno: Option<u32>,
}

/// A contiguous block of changes in a file diff.
#[derive(Debug, Clone)]
pub struct Hunk {
    /// `(start, count)` range in the old file.
    pub old_range: (u32, u32),
    /// `(start, count)` range in the new file.
    pub new_range: (u32, u32),
    /// The lines belonging to this hunk (context + added + removed).
    pub lines: Vec<DiffLine>,
}

/// The complete diff for one file in a commit.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Path in the old tree (populated for Deleted / Renamed files).
    pub old_path: Option<PathBuf>,
    /// Path in the new tree (populated for Added / Modified / Renamed files).
    pub new_path: Option<PathBuf>,
    /// The type of change that produced this diff.
    pub change: ChangeKind,
    /// The diff hunks.  Empty for binary files.
    pub hunks: Vec<Hunk>,
    /// `true` if git detected the file as binary (no text diff available).
    pub is_binary: bool,
}
