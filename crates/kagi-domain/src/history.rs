//! Operation history — the pure undo/redo stack for ref-moving operations.
//!
//! No `git2`, no `gpui`, no I/O. This module is the domain core of the
//! GitKraken-style Undo / Redo feature (ADR-0081, T-UNDOREDO-001).
//!
//! ## Model
//!
//! Each successful ref-moving operation (commit, merge, cherry-pick, revert,
//! amend, undo-commit…) is recorded as a [`HistoryEntry`] capturing the branch
//! that moved and the `before`/`after` commit SHAs (both remain reachable via
//! the reflog/ODB — nothing is ever destroyed).
//!
//! [`OperationHistory`] holds the entries plus a `cursor`. The cursor is the
//! count of entries that are currently "applied" (i.e. live in front of the
//! cursor):
//!
//! ```text
//!   entries: [ e0  e1  e2 ]   cursor = 3  → everything applied (top of stack)
//!                          ^cursor
//!   undo() → moves cursor to 2 (e2 is now undone, available to redo)
//!   redo() → moves cursor back to 3 (e2 re-applied)
//! ```
//!
//! Recording a NEW entry truncates any redo tail (entries past the cursor),
//! the standard undo-stack semantic.
//!
//! All methods here are PURE — they only move the cursor and read entries.
//! The actual ref move is performed by the git-backend layer.

use crate::commit::CommitId;

/// The kind of ref-moving operation that produced a [`HistoryEntry`].
///
/// Used for display (the undo/redo preview) and for tailoring messages; the
/// undo/redo mechanics are identical for every kind (a branch-ref move between
/// two reachable SHAs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
    /// A normal `commit`.
    Commit,
    /// A `merge` that produced a merge commit (or fast-forwarded).
    Merge,
    /// A `cherry-pick`.
    CherryPick,
    /// A `revert`.
    Revert,
    /// An `amend` (HEAD replaced by a new commit object).
    Amend,
    /// An `undo-commit` (the soft undo of the last commit, ADR-0041).
    UndoCommit,
}

impl OperationKind {
    /// A short, stable lowercase slug for the kind — used in oplog records and
    /// log messages (e.g. `"commit"`, `"merge"`).
    pub fn slug(&self) -> &'static str {
        match self {
            OperationKind::Commit => "commit",
            OperationKind::Merge => "merge",
            OperationKind::CherryPick => "cherry-pick",
            OperationKind::Revert => "revert",
            OperationKind::Amend => "amend",
            OperationKind::UndoCommit => "undo-commit",
        }
    }
}

/// One recorded ref-moving operation.
///
/// `before` is the branch tip *before* the operation; `after` is the tip
/// *after* it. Undo moves the branch from `after` back to `before`; redo moves
/// it from `before` forward to `after`. Both SHAs stay reachable via the reflog,
/// so neither undo nor redo can destroy a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// What kind of operation this was.
    pub kind: OperationKind,
    /// The short branch name whose ref moved (e.g. `"main"`).
    pub branch: String,
    /// The branch tip before the operation (undo target).
    pub before: CommitId,
    /// The branch tip after the operation (redo target).
    pub after: CommitId,
    /// Human-readable one-line summary for the preview, e.g.
    /// `"commit a1b2c3d4 'Fix the thing'"`.
    pub summary: String,
}

/// An in-session undo/redo stack of [`HistoryEntry`] with a cursor.
///
/// `cursor` is the number of applied entries (those in front of the cursor).
/// Invariant: `0 <= cursor <= entries.len()`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationHistory {
    entries: Vec<HistoryEntry>,
    cursor: usize,
}

impl OperationHistory {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
        }
    }

    /// Record a freshly-executed operation at the cursor.
    ///
    /// Any redo tail (entries past the cursor, i.e. operations that were undone
    /// and not yet redone) is truncated first — performing a new operation
    /// invalidates the redo stack (standard undo-stack semantics). The cursor
    /// then advances past the new entry.
    pub fn record(&mut self, entry: HistoryEntry) {
        // Drop the redo tail.
        self.entries.truncate(self.cursor);
        self.entries.push(entry);
        self.cursor = self.entries.len();
    }

    /// Whether there is an applied entry that can be undone.
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    /// Whether there is an undone entry that can be redone.
    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    /// The entry that [`undo`](Self::undo) would act on, without moving the
    /// cursor. `None` when nothing can be undone.
    pub fn peek_undo(&self) -> Option<&HistoryEntry> {
        if self.can_undo() {
            self.entries.get(self.cursor - 1)
        } else {
            None
        }
    }

    /// The entry that [`redo`](Self::redo) would act on, without moving the
    /// cursor. `None` when nothing can be redone.
    pub fn peek_redo(&self) -> Option<&HistoryEntry> {
        if self.can_redo() {
            self.entries.get(self.cursor)
        } else {
            None
        }
    }

    /// Move the cursor back one step (undo) and return the entry that was
    /// undone. `None` when nothing can be undone (the cursor is unchanged).
    ///
    /// This only updates the cursor; the caller performs the actual ref move
    /// (branch from `after` back to `before`).
    pub fn undo(&mut self) -> Option<&HistoryEntry> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor)
    }

    /// Move the cursor forward one step (redo) and return the entry that was
    /// re-applied. `None` when nothing can be redone (the cursor is unchanged).
    ///
    /// This only updates the cursor; the caller performs the actual ref move
    /// (branch from `before` forward to `after`).
    pub fn redo(&mut self) -> Option<&HistoryEntry> {
        if !self.can_redo() {
            return None;
        }
        let entry = self.entries.get(self.cursor);
        self.cursor += 1;
        entry
    }

    /// The current cursor position (number of applied entries). Mainly for
    /// tests and diagnostics.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Total number of recorded entries (applied + redoable).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history holds no entries at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid(s: &str) -> CommitId {
        CommitId(s.to_string())
    }

    fn entry(branch: &str, before: &str, after: &str) -> HistoryEntry {
        HistoryEntry {
            kind: OperationKind::Commit,
            branch: branch.to_string(),
            before: cid(before),
            after: cid(after),
            summary: format!("commit {}", &after[..after.len().min(8)]),
        }
    }

    #[test]
    fn new_history_is_empty_and_idle() {
        let h = OperationHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.cursor(), 0);
        assert!(!h.can_undo());
        assert!(!h.can_redo());
        assert!(h.peek_undo().is_none());
        assert!(h.peek_redo().is_none());
    }

    #[test]
    fn record_advances_cursor_and_enables_undo() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "aaa", "bbb"));
        assert_eq!(h.len(), 1);
        assert_eq!(h.cursor(), 1);
        assert!(h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.peek_undo().unwrap().after, cid("bbb"));
    }

    #[test]
    fn undo_then_redo_round_trips_cursor() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "aaa", "bbb"));

        let undone = h.undo().cloned();
        assert_eq!(undone.unwrap().before, cid("aaa"));
        assert_eq!(h.cursor(), 0);
        assert!(!h.can_undo());
        assert!(h.can_redo());
        assert_eq!(h.peek_redo().unwrap().after, cid("bbb"));

        let redone = h.redo().cloned();
        assert_eq!(redone.unwrap().after, cid("bbb"));
        assert_eq!(h.cursor(), 1);
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_at_bottom_is_none_and_keeps_cursor() {
        let mut h = OperationHistory::new();
        assert!(h.undo().is_none());
        assert_eq!(h.cursor(), 0);
    }

    #[test]
    fn redo_at_top_is_none_and_keeps_cursor() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "aaa", "bbb"));
        assert!(h.redo().is_none());
        assert_eq!(h.cursor(), 1);
    }

    #[test]
    fn multiple_entries_undo_in_lifo_order() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "a0", "a1"));
        h.record(entry("main", "a1", "a2"));
        h.record(entry("main", "a2", "a3"));
        assert_eq!(h.cursor(), 3);

        assert_eq!(h.peek_undo().unwrap().after, cid("a3"));
        h.undo();
        assert_eq!(h.peek_undo().unwrap().after, cid("a2"));
        h.undo();
        assert_eq!(h.peek_undo().unwrap().after, cid("a1"));
        h.undo();
        assert!(!h.can_undo());
        assert_eq!(h.cursor(), 0);
    }

    #[test]
    fn record_truncates_redo_tail() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "a0", "a1"));
        h.record(entry("main", "a1", "a2"));
        h.record(entry("main", "a2", "a3"));

        // Undo twice → cursor at 1, two entries are redoable.
        h.undo();
        h.undo();
        assert_eq!(h.cursor(), 1);
        assert_eq!(h.len(), 3);
        assert!(h.can_redo());

        // A new operation truncates the redo tail (a2, a3) and appends.
        h.record(entry("main", "a1", "b2"));
        assert_eq!(h.cursor(), 2);
        assert_eq!(h.len(), 2);
        assert!(!h.can_redo());
        assert_eq!(h.peek_undo().unwrap().after, cid("b2"));

        // The truncated entries are gone — redo does nothing.
        assert!(h.redo().is_none());
    }

    #[test]
    fn peek_does_not_move_cursor() {
        let mut h = OperationHistory::new();
        h.record(entry("main", "aaa", "bbb"));
        let c = h.cursor();
        let _ = h.peek_undo();
        let _ = h.peek_redo();
        assert_eq!(h.cursor(), c);
    }

    #[test]
    fn full_undo_all_then_redo_all() {
        let mut h = OperationHistory::new();
        for i in 0..5 {
            h.record(entry("main", &format!("c{i}"), &format!("c{}", i + 1)));
        }
        assert_eq!(h.cursor(), 5);
        for _ in 0..5 {
            assert!(h.undo().is_some());
        }
        assert_eq!(h.cursor(), 0);
        assert!(h.undo().is_none());
        for _ in 0..5 {
            assert!(h.redo().is_some());
        }
        assert_eq!(h.cursor(), 5);
        assert!(h.redo().is_none());
    }

    #[test]
    fn operation_kind_slugs_are_stable() {
        assert_eq!(OperationKind::Commit.slug(), "commit");
        assert_eq!(OperationKind::Merge.slug(), "merge");
        assert_eq!(OperationKind::CherryPick.slug(), "cherry-pick");
        assert_eq!(OperationKind::Revert.slug(), "revert");
        assert_eq!(OperationKind::Amend.slug(), "amend");
        assert_eq!(OperationKind::UndoCommit.slug(), "undo-commit");
    }
}
