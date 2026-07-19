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

/// One row of a side-by-side (split) diff: indices into the source line
/// sequence for the left (old) and right (new) cells (ADR-0124).
///
/// `None` is a filler cell — an unbalanced replace / pure add / pure remove
/// leaves one side empty for that visual row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPair {
    /// Index of the line shown in the left (old) column.
    pub left: Option<usize>,
    /// Index of the line shown in the right (new) column.
    pub right: Option<usize>,
}

/// Pair a diff-line sequence for side-by-side display (ADR-0124).
///
/// Walks `kinds` (the [`DiffLineKind`] of each line, in hunk order) and
/// produces one [`SplitPair`] per visual row:
///
/// - a `Context` line occupies both columns (same index left and right);
/// - a run of `Removed` lines immediately followed by a run of `Added` lines
///   is a *replace block*: the two runs are paired index-wise, the longer
///   run's tail getting filler cells on the other side;
/// - a run with no counterpart (pure add / pure remove) pairs against filler.
///
/// Pure and allocation-only — unit-tested here so every renderer (native,
/// remote) shares one pairing behaviour.
pub fn split_pairs(kinds: &[DiffLineKind]) -> Vec<SplitPair> {
    let mut out = Vec::with_capacity(kinds.len());
    let mut i = 0usize;
    while i < kinds.len() {
        match kinds[i] {
            DiffLineKind::Context => {
                out.push(SplitPair {
                    left: Some(i),
                    right: Some(i),
                });
                i += 1;
            }
            DiffLineKind::Removed => {
                // Collect the removed run, then any directly-following added
                // run, and pair them index-wise.
                let removed_start = i;
                while i < kinds.len() && kinds[i] == DiffLineKind::Removed {
                    i += 1;
                }
                let added_start = i;
                while i < kinds.len() && kinds[i] == DiffLineKind::Added {
                    i += 1;
                }
                let removed = removed_start..added_start;
                let added = added_start..i;
                let rows = removed.len().max(added.len());
                for k in 0..rows {
                    out.push(SplitPair {
                        left: removed.clone().nth(k),
                        right: added.clone().nth(k),
                    });
                }
            }
            DiffLineKind::Added => {
                // Added run with no preceding removed run: right column only.
                out.push(SplitPair {
                    left: None,
                    right: Some(i),
                });
                i += 1;
            }
        }
    }
    out
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

#[cfg(test)]
mod split_pair_tests {
    use super::DiffLineKind::{Added as A, Context as C, Removed as R};
    use super::{split_pairs, SplitPair};

    fn p(left: Option<usize>, right: Option<usize>) -> SplitPair {
        SplitPair { left, right }
    }

    #[test]
    fn context_occupies_both_columns() {
        assert_eq!(
            split_pairs(&[C, C]),
            vec![p(Some(0), Some(0)), p(Some(1), Some(1))]
        );
    }

    #[test]
    fn balanced_replace_pairs_index_wise() {
        // R R A A → two rows, old/new side by side.
        assert_eq!(
            split_pairs(&[R, R, A, A]),
            vec![p(Some(0), Some(2)), p(Some(1), Some(3))]
        );
    }

    #[test]
    fn unbalanced_replace_fills_the_shorter_side() {
        // R A A → row 1 pairs, row 2 has a left filler.
        assert_eq!(
            split_pairs(&[R, A, A]),
            vec![p(Some(0), Some(1)), p(None, Some(2))]
        );
        // R R A → row 2 has a right filler.
        assert_eq!(
            split_pairs(&[R, R, A]),
            vec![p(Some(0), Some(2)), p(Some(1), None)]
        );
    }

    #[test]
    fn pure_add_and_pure_remove_pair_against_filler() {
        assert_eq!(
            split_pairs(&[A, A]),
            vec![p(None, Some(0)), p(None, Some(1))]
        );
        assert_eq!(
            split_pairs(&[R, R]),
            vec![p(Some(0), None), p(Some(1), None)]
        );
    }

    #[test]
    fn context_separates_replace_blocks() {
        // R A C R A → two independent replace blocks around a context row.
        assert_eq!(
            split_pairs(&[R, A, C, R, A]),
            vec![
                p(Some(0), Some(1)),
                p(Some(2), Some(2)),
                p(Some(3), Some(4)),
            ]
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(split_pairs(&[]), Vec::<SplitPair>::new());
    }
}
