//! Working tree status domain models — pure data, no git2.
//!
//! The git2-backed `working_tree_status` function that populates these models
//! lives in the git-backend layer (`kagi::git::status`).

use std::path::PathBuf;

/// The type of change recorded for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// File was added (did not exist in the previous tree/index).
    Added,
    /// File content was modified.
    Modified,
    /// File was deleted.
    Deleted,
    /// File was renamed; `from` is the original path.
    Renamed {
        /// Previous path of the file.
        from: PathBuf,
    },
    /// File type changed (e.g. regular file → symlink).
    TypeChange,
}

impl ChangeKind {
    /// Short label used in the UI (e.g. "Modified", "Added").
    pub fn label(&self) -> &'static str {
        match self {
            ChangeKind::Added => "Added",
            ChangeKind::Modified => "Modified",
            ChangeKind::Deleted => "Deleted",
            ChangeKind::Renamed { .. } => "Renamed",
            ChangeKind::TypeChange => "TypeChange",
        }
    }
}

/// Status of a single file within the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// Path of the file relative to the repository root.
    pub path: PathBuf,
    /// The kind of change.
    pub change: ChangeKind,
}

/// Snapshot of the working tree status.
///
/// Untracked files and conflicted files are stored as bare `PathBuf` values
/// because they have no meaningful "change kind".
///
/// A file that has both staged and unstaged changes will appear in **both**
/// `staged` and `unstaged` (e.g. partially-staged modifications).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkingTreeStatus {
    /// Files staged in the index (ready to be committed).
    pub staged: Vec<FileStatus>,
    /// Files modified in the work directory but not yet staged.
    pub unstaged: Vec<FileStatus>,
    /// New files that are not tracked by Git.
    pub untracked: Vec<PathBuf>,
    /// Files with unresolved merge conflicts.
    pub conflicted: Vec<PathBuf>,
}

impl WorkingTreeStatus {
    /// Returns `true` if there are any changes (staged, unstaged, untracked,
    /// or conflicted). A clean working tree returns `false`.
    pub fn is_dirty(&self) -> bool {
        !self.staged.is_empty()
            || !self.unstaged.is_empty()
            || !self.untracked.is_empty()
            || !self.conflicted.is_empty()
    }
}
