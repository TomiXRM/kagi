//! Operation log panel — self-contained state (ADR-0111 / Phase C).
//!
//! Extracted from `KagiApp` so the op-log ring buffer + display logic lives in
//! one testable struct. Previously `op_entries: VecDeque<OpLogEntry>` was a flat
//! field on the 105-field god-struct, pushed to from `record_op` and read from
//! `render_bottom_panel`.
//!
//! Today held as `Rc<RefCell<OpLogPanel>>` (same pattern as ToastStack,
//! ADR-0110) because `record_op` has ~30 callers that don't all have `cx`.
//! Entity migration is a follow-up once those callers thread `cx`.

use std::collections::VecDeque;

use kagi_git::oplog::OpLogEntry;

/// Maximum entries kept in the in-memory ring buffer.
const OP_ENTRIES_MAX: usize = 200;

/// Self-contained operation log ring buffer.
pub struct OpLogPanel {
    entries: VecDeque<OpLogEntry>,
}

impl OpLogPanel {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    /// Initialize from a pre-loaded tail (read from disk on tab open).
    pub fn from_entries(entries: VecDeque<OpLogEntry>) -> Self {
        Self { entries }
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
