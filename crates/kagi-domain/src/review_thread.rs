//! Pull-request review threads placed on the PR diff (#351, ADR-0209).
//!
//! A thread (GraphQL `PullRequestReviewThread`) anchors to one line of one
//! side of the PR's head diff: `RIGHT` is the head (new) line number, `LEFT`
//! the base (old) one. An outdated thread no longer has a current `line`;
//! it is placed at its `originalLine` and marked outdated, so it can be shown
//! dimmed where it was written rather than dropped. Pure: rows are given as
//! each diff row's `(old line, new line)`.

use std::collections::BTreeMap;

use crate::github::ReviewComment;

/// Which side of the diff a thread's line number counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffSide {
    /// The base (old) side: a removed or context line's old number.
    Left,
    /// The head (new) side: an added or context line's new number.
    #[default]
    Right,
}

/// One review thread as read from GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewThread {
    pub path: String,
    /// The current anchor line; `None` once the thread is outdated.
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    /// The line the thread was written on, kept when it goes outdated.
    pub original_line: Option<u32>,
    pub diff_side: DiffSide,
    pub is_outdated: bool,
    pub is_resolved: bool,
    /// Read but not acted on: resolving is a write (a later slice).
    pub viewer_can_resolve: bool,
    /// The thread's comments, oldest first.
    pub comments: Vec<ReviewComment>,
}

impl ReviewThread {
    /// The line this thread is placed on and whether that placement is
    /// outdated. Outdated means GitHub says so, or there is no current line.
    pub fn anchor(&self) -> Option<(u32, bool)> {
        match (self.line, self.original_line) {
            (Some(line), _) if !self.is_outdated => Some((line, false)),
            (line, original) => original.or(line).map(|line| (line, true)),
        }
    }
}

/// Where `threads` sit in one file's diff: row index → indices into `threads`
/// (in thread order). `rows[i]` is diff row `i`'s `(old line, new line)`; a
/// thread lands on the first row carrying its line on its side. Threads for
/// other paths, or whose line the diff does not show, are not placed.
pub fn anchor_rows(
    threads: &[ReviewThread],
    path: &str,
    rows: &[(Option<u32>, Option<u32>)],
) -> BTreeMap<usize, Vec<usize>> {
    let mut placed: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (ix, thread) in threads.iter().enumerate() {
        if thread.path != path {
            continue;
        }
        let Some((line, _)) = thread.anchor() else {
            continue;
        };
        let row = rows.iter().position(|(old, new)| match thread.diff_side {
            DiffSide::Left => *old == Some(line),
            DiffSide::Right => *new == Some(line),
        });
        if let Some(row) = row {
            placed.entry(row).or_default().push(ix);
        }
    }
    placed
}

/// Every thread's comments as the conversation feed's flat list, keeping the
/// feed's existing anchor: the current line, else the original one.
pub fn feed_comments(threads: &[ReviewThread]) -> Vec<ReviewComment> {
    threads
        .iter()
        .flat_map(|thread| {
            let line = thread.line.or(thread.original_line).unwrap_or(0);
            thread.comments.iter().map(move |comment| ReviewComment {
                path: thread.path.clone(),
                line,
                start_line: thread.start_line,
                ..comment.clone()
            })
        })
        .filter(|comment| !comment.body.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(path: &str, side: DiffSide, line: Option<u32>, original: u32) -> ReviewThread {
        ReviewThread {
            path: path.into(),
            line,
            original_line: Some(original),
            diff_side: side,
            is_outdated: line.is_none(),
            ..Default::default()
        }
    }

    /// context c1 (1,1); removed c2 (2,-); added C2 (-,2); added c3 (-,3).
    const ROWS: [(Option<u32>, Option<u32>); 5] = [
        (None, None), // hunk header
        (Some(1), Some(1)),
        (Some(2), None),
        (None, Some(2)),
        (None, Some(3)),
    ];

    #[test]
    fn a_right_thread_lands_on_the_new_line_and_a_left_one_on_the_old_line() {
        let threads = [
            thread("c.txt", DiffSide::Right, Some(2), 2),
            thread("c.txt", DiffSide::Left, Some(2), 2),
            thread("c.txt", DiffSide::Right, Some(1), 1),
        ];
        let placed = anchor_rows(&threads, "c.txt", &ROWS);
        assert_eq!(placed.get(&3), Some(&vec![0]), "RIGHT 2 is the added row");
        assert_eq!(placed.get(&2), Some(&vec![1]), "LEFT 2 is the removed row");
        assert_eq!(placed.get(&1), Some(&vec![2]), "a context row carries both");
        assert_eq!(placed.len(), 3);
    }

    #[test]
    fn several_threads_on_one_row_keep_their_order() {
        let threads = [
            thread("c.txt", DiffSide::Right, Some(3), 3),
            thread("c.txt", DiffSide::Right, Some(3), 3),
        ];
        assert_eq!(
            anchor_rows(&threads, "c.txt", &ROWS).get(&4),
            Some(&vec![0, 1])
        );
    }

    #[test]
    fn other_paths_and_lines_the_diff_does_not_show_are_not_placed() {
        let threads = [
            thread("other.txt", DiffSide::Right, Some(2), 2),
            thread("c.txt", DiffSide::Right, Some(40), 40),
            thread("c.txt", DiffSide::Left, Some(3), 3),
        ];
        assert!(anchor_rows(&threads, "c.txt", &ROWS).is_empty());
    }

    #[test]
    fn an_outdated_thread_is_placed_at_its_original_line_and_marked() {
        let outdated = thread("c.txt", DiffSide::Right, None, 3);
        assert_eq!(outdated.anchor(), Some((3, true)));
        assert_eq!(
            anchor_rows(&[outdated], "c.txt", &ROWS).get(&4),
            Some(&vec![0])
        );

        let current = thread("c.txt", DiffSide::Right, Some(2), 1);
        assert_eq!(current.anchor(), Some((2, false)), "the current line wins");

        let flagged = ReviewThread {
            is_outdated: true,
            ..thread("c.txt", DiffSide::Right, Some(2), 1)
        };
        assert_eq!(flagged.anchor(), Some((1, true)), "GitHub's flag wins");

        let nowhere = ReviewThread {
            original_line: None,
            ..thread("c.txt", DiffSide::Right, None, 0)
        };
        assert_eq!(nowhere.anchor(), None);
    }

    #[test]
    fn the_feed_keeps_every_comment_with_the_thread_s_anchor() {
        let comment = |body: &str| ReviewComment {
            author: "a".into(),
            body: body.into(),
            ..Default::default()
        };
        let threads = [
            ReviewThread {
                comments: vec![comment("first"), comment("  ")],
                ..thread("c.txt", DiffSide::Right, Some(2), 2)
            },
            ReviewThread {
                comments: vec![comment("old")],
                ..thread("c.txt", DiffSide::Right, None, 7)
            },
        ];
        let feed = feed_comments(&threads);
        let got: Vec<_> = feed
            .iter()
            .map(|c| (c.path.as_str(), c.line, c.body.as_str()))
            .collect();
        assert_eq!(got, vec![("c.txt", 2, "first"), ("c.txt", 7, "old")]);
    }
}
