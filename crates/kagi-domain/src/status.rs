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

/// A stable fingerprint of a working tree's *classification*, for detecting
/// TOCTOU changes between an operation's plan and its execute (#295).
///
/// It hashes each file's path together with which group it is in
/// (staged / unstaged / untracked / conflicted) and its `ChangeKind`. So it
/// changes when a path is added to or removed from any group, or moves between
/// groups (e.g. `git rm --cached` moving a file from unstaged to untracked, or
/// a merge turning a path conflicted) — which is exactly the set of shifts that
/// silently invalidated a plan's blockers. It deliberately does NOT change when
/// a file's *contents* change while its classification stays the same: that is
/// not a classification shift, and hashing content would make every keystroke
/// in an open editor invalidate the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorktreeDigest(pub u64);

impl WorkingTreeStatus {
    /// Compute the [`WorktreeDigest`]. Order-independent: each group is sorted
    /// before hashing, so the same tree always yields the same value.
    pub fn digest(&self) -> WorktreeDigest {
        use std::hash::{Hash, Hasher};

        fn change_tag(c: &ChangeKind) -> String {
            match c {
                ChangeKind::Added => "A".into(),
                ChangeKind::Modified => "M".into(),
                ChangeKind::Deleted => "D".into(),
                ChangeKind::TypeChange => "T".into(),
                // The old path is part of the identity: a different rename
                // source is a different change.
                ChangeKind::Renamed { from } => format!("R:{}", from.to_string_lossy()),
            }
        }
        // One normalized line per entry, "<group> <tag> <path>", so a path that
        // moves between groups or changes kind produces a different line.
        let mut lines: Vec<String> = Vec::new();
        for f in &self.staged {
            lines.push(format!(
                "s {} {}",
                change_tag(&f.change),
                f.path.to_string_lossy()
            ));
        }
        for f in &self.unstaged {
            lines.push(format!(
                "u {} {}",
                change_tag(&f.change),
                f.path.to_string_lossy()
            ));
        }
        for p in &self.untracked {
            lines.push(format!("t {}", p.to_string_lossy()));
        }
        for p in &self.conflicted {
            lines.push(format!("c {}", p.to_string_lossy()));
        }
        lines.sort();

        let mut h = std::collections::hash_map::DefaultHasher::new();
        // DefaultHasher (SipHash, fixed keys) is deterministic within a build,
        // which is all a same-process plan→execute needs.
        lines.hash(&mut h);
        WorktreeDigest(h.finish())
    }

    /// Returns `true` if there are any changes (staged, unstaged, untracked,
    /// or conflicted). A clean working tree returns `false`.
    pub fn is_dirty(&self) -> bool {
        !self.staged.is_empty()
            || !self.unstaged.is_empty()
            || !self.untracked.is_empty()
            || !self.conflicted.is_empty()
    }
}

#[cfg(test)]
mod digest_tests {
    use super::*;
    use std::path::PathBuf;

    fn fs(p: &str, c: ChangeKind) -> FileStatus {
        FileStatus {
            path: PathBuf::from(p),
            change: c,
        }
    }

    /// The order files come back in must not change the digest.
    #[test]
    fn digest_is_order_independent() {
        let a = WorkingTreeStatus {
            unstaged: vec![
                fs("a.txt", ChangeKind::Modified),
                fs("b.txt", ChangeKind::Modified),
            ],
            ..Default::default()
        };
        let b = WorkingTreeStatus {
            unstaged: vec![
                fs("b.txt", ChangeKind::Modified),
                fs("a.txt", ChangeKind::Modified),
            ],
            ..Default::default()
        };
        assert_eq!(a.digest(), b.digest());
    }

    /// The three TOCTOU shifts #295 is about must each move the digest.
    #[test]
    fn every_classification_shift_changes_the_digest() {
        let base = WorkingTreeStatus {
            unstaged: vec![fs("f.txt", ChangeKind::Modified)],
            ..Default::default()
        };
        let d0 = base.digest();

        // f.txt turns conflicted (interrupting merge) — #295 impact 1.
        let conflicted = WorkingTreeStatus {
            conflicted: vec![PathBuf::from("f.txt")],
            ..Default::default()
        };
        assert_ne!(
            conflicted.digest(),
            d0,
            "unstaged → conflicted must be seen"
        );

        // f.txt moves unstaged → untracked (git rm --cached) — #295 impact 2.
        let untracked = WorkingTreeStatus {
            untracked: vec![PathBuf::from("f.txt")],
            ..Default::default()
        };
        assert_ne!(untracked.digest(), d0, "unstaged → untracked must be seen");

        // a previously-clean tree turns dirty (merge/stash TOCTOU) — 3 & 4.
        let extra_dirty = WorkingTreeStatus {
            unstaged: vec![
                fs("f.txt", ChangeKind::Modified),
                fs("g.txt", ChangeKind::Modified),
            ],
            ..Default::default()
        };
        assert_ne!(extra_dirty.digest(), d0, "a new dirty path must be seen");
    }

    /// Editing a file that stays in the same group must NOT move the digest —
    /// otherwise every keystroke in an open editor invalidates the plan.
    #[test]
    fn same_classification_different_content_is_stable() {
        // The digest has no content input, so two Modified states of the same
        // path are identical by construction; this pins that intent.
        let a = WorkingTreeStatus {
            unstaged: vec![fs("f.txt", ChangeKind::Modified)],
            ..Default::default()
        };
        let b = a.clone();
        assert_eq!(a.digest(), b.digest());
    }

    /// A rename's source path is part of the identity.
    #[test]
    fn a_different_rename_source_changes_the_digest() {
        let a = WorkingTreeStatus {
            staged: vec![fs(
                "new.txt",
                ChangeKind::Renamed {
                    from: PathBuf::from("old.txt"),
                },
            )],
            ..Default::default()
        };
        let b = WorkingTreeStatus {
            staged: vec![fs(
                "new.txt",
                ChangeKind::Renamed {
                    from: PathBuf::from("other.txt"),
                },
            )],
            ..Default::default()
        };
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn a_clean_tree_has_a_stable_digest() {
        assert_eq!(
            WorkingTreeStatus::default().digest(),
            WorkingTreeStatus::default().digest()
        );
    }
}
