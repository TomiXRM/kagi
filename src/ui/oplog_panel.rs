//! Operation log panel — self-contained state (ADR-0111 / Phase C).
//!
//! Extracted from `KagiApp` so the op-log ring buffer + display logic lives in
//! one testable struct. Previously `op_entries: VecDeque<OpLogEntry>` was a flat
//! field on the god-struct, pushed to from `record_op` and read from
//! `render_bottom_panel`.
//!
//! Held as an `Entity<OpLogPanel>` on `KagiApp` (ADR-0110 Phase 5 Step 5.1, the
//! same shape as `ToastStack`): the panel renders via `impl Render for
//! OpLogPanel` (in `render.rs`) and a push / row-expand re-renders only this
//! subtree, not the whole app. The per-panel UI state (the expanded row and the
//! scroll handle) lives here too. The disk-loaded startup tail is carried in
//! `KagiApp::op_log_seed` until `open_main_window` can create the entity (the
//! pure constructors have no `cx`). The data methods stay `cx`-free for tests.

use std::collections::VecDeque;

use gpui::UniformListScrollHandle;
use kagi_git::oplog::OpLogEntry;

/// Maximum entries kept in the in-memory ring buffer.
const OP_ENTRIES_MAX: usize = 200;

/// Self-contained operation log ring buffer + panel UI state.
pub struct OpLogPanel {
    entries: VecDeque<OpLogEntry>,
    /// Which row index (0 = newest) is currently expanded; `None` = none.
    expanded: Option<usize>,
    /// Scroll handle for the `uniform_list` virtual scroll.
    scroll_handle: UniformListScrollHandle,
}

impl OpLogPanel {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            expanded: None,
            scroll_handle: UniformListScrollHandle::new(),
        }
    }

    /// Initialize from a pre-loaded tail (read from disk on tab open).
    pub fn from_entries(entries: VecDeque<OpLogEntry>) -> Self {
        Self {
            entries,
            expanded: None,
            scroll_handle: UniformListScrollHandle::new(),
        }
    }

    /// Push a new entry to the front; drop the oldest if over the cap.
    pub fn push(&mut self, entry: OpLogEntry) {
        self.entries.push_front(entry);
        if self.entries.len() > OP_ENTRIES_MAX {
            self.entries.pop_back();
        }
    }

    /// Read-only access to the entries (for rendering).
    pub fn entries(&self) -> &VecDeque<OpLogEntry> {
        &self.entries
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The currently-expanded row index, if any.
    pub fn expanded(&self) -> Option<usize> {
        self.expanded
    }

    /// Toggle expansion of row `i` (collapse if already expanded).
    pub fn toggle_expanded(&mut self, i: usize) {
        self.expanded = if self.expanded == Some(i) {
            None
        } else {
            Some(i)
        };
    }

    /// Collapse any expanded row (called when new entries arrive).
    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    /// A clone of the scroll handle for the `uniform_list` + scrollbar.
    pub fn scroll_handle(&self) -> UniformListScrollHandle {
        self.scroll_handle.clone()
    }
}

impl Default for OpLogPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::oplog::OpOutcome;

    fn dummy_entry(op: &str) -> OpLogEntry {
        OpLogEntry::new(
            op,
            "repo",
            kagi_git::ops::StateSummary {
                head: "HEAD → main".to_string(),
                dirty: "clean".to_string(),
            },
            OpOutcome::Success {
                after: kagi_git::ops::StateSummary {
                    head: "main".to_string(),
                    dirty: "clean".to_string(),
                },
            },
        )
    }

    #[test]
    fn push_and_cap() {
        let mut panel = OpLogPanel::new();
        for i in 0..(OP_ENTRIES_MAX + 5) {
            panel.push(dummy_entry(&format!("op-{i}")));
        }
        assert_eq!(panel.len(), OP_ENTRIES_MAX);
        // Oldest should have been dropped.
        assert!(!panel.entries().iter().any(|e| e.op == "op-0"));
    }

    #[test]
    fn push_front_ordering() {
        let mut panel = OpLogPanel::new();
        panel.push(dummy_entry("first"));
        panel.push(dummy_entry("second"));
        assert_eq!(panel.entries().front().unwrap().op, "second");
    }

    #[test]
    fn from_entries_preserves_order() {
        let mut vd = VecDeque::new();
        vd.push_back(dummy_entry("a"));
        vd.push_back(dummy_entry("b"));
        let panel = OpLogPanel::from_entries(vd);
        assert_eq!(panel.len(), 2);
        assert_eq!(panel.entries().front().unwrap().op, "a");
    }
}
