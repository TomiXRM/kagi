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

use kagi_git::oplog::{OpLogEntry, OpOutcome};

/// Maximum entries kept in the in-memory ring buffer.
const OP_ENTRIES_MAX: usize = 200;

/// Self-contained operation log ring buffer + panel UI state.
pub struct OpLogPanel {
    entries: VecDeque<OpLogEntry>,
    /// Which row index (0 = newest) is currently expanded; `None` = none.
    expanded: Option<usize>,
    /// Scroll handle for the virtualized row list. Issue #468: a
    /// [`gpui::ListState`] (variable row height) rather than a
    /// `UniformListScrollHandle` — an expanded row is taller than a collapsed
    /// one, and `uniform_list` lays every row out at the FIRST row's height,
    /// so the overflow painted over the rows below. Same swap T-DIFF-WRAP-001
    /// made for the diff panes (`render_helpers::new_diff_list_state`).
    scroll_handle: gpui::ListState,
}

/// Issue #468: a fresh op-log [`gpui::ListState`] (item count 0 — the render
/// syncs it to the real entry count each frame, the lifecycle documented on
/// `render_helpers::render_diff_list`). `px(1000.)` overdraw matches the diff
/// list, the other variable-height list in the app.
fn new_oplog_list_state() -> gpui::ListState {
    gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(1000.))
}

impl OpLogPanel {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            expanded: None,
            scroll_handle: new_oplog_list_state(),
        }
    }

    /// Initialize from a pre-loaded tail (read from disk on tab open).
    pub fn from_entries(entries: VecDeque<OpLogEntry>) -> Self {
        Self {
            entries,
            expanded: None,
            scroll_handle: new_oplog_list_state(),
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

    /// A clone of the scroll handle for `gpui::list` + the scrollbar overlay.
    pub fn scroll_handle(&self) -> gpui::ListState {
        self.scroll_handle.clone()
    }

    /// Issue #468: copy row `i`'s whole entry (the truncated summary AND the
    /// detail block) to the clipboard. Same path as `branch_menu::copy_*` /
    /// `context_menu::copy_full_sha`. Called by the row's copy button; also the
    /// seam the GUI E2E scenario drives.
    pub fn copy_entry(&self, i: usize, cx: &mut gpui::App) {
        let Some(entry) = self.entries.get(i) else {
            return;
        };
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(entry_clipboard_text(entry)));
    }
}

/// One-line outcome summary shown on the (truncated) summary row. Pure so the
/// clipboard text and the rendered row cannot drift apart.
pub fn outcome_summary(outcome: &OpOutcome) -> String {
    match outcome {
        OpOutcome::Success { after } => format!("Success \u{2192} {}", after.head),
        OpOutcome::Partial { after, error } => {
            format!("Partial \u{2192} {}: {}", after.head, error)
        }
        OpOutcome::Failed { error } => format!("Failed: {}", error),
        OpOutcome::Refused { blockers } => format!(
            "Refused ({} blocker{})",
            blockers.len(),
            if blockers.len() == 1 { "" } else { "s" }
        ),
    }
}

/// The expanded-row detail lines (before/after state + error/blockers). Pure.
pub fn detail_lines(entry: &OpLogEntry) -> Vec<String> {
    let mut lines = vec![
        format!("  before:  {}", entry.before.head),
        format!("  dirty:   {}", entry.before.dirty),
    ];
    match &entry.outcome {
        OpOutcome::Success { after } => {
            lines.push(format!("  after:   {}", after.head));
            lines.push(format!("  dirty:   {}", after.dirty));
        }
        OpOutcome::Partial { after, error } => {
            lines.push(format!("  after:   {}", after.head));
            lines.push(format!("  dirty:   {}", after.dirty));
            lines.push(format!("  error:   {}", error));
        }
        OpOutcome::Failed { error } => lines.push(format!("  error:   {}", error)),
        OpOutcome::Refused { blockers } => {
            for b in blockers {
                lines.push(format!("  blocker: {}", b));
            }
        }
    }
    lines
}

/// Issue #468: the whole entry as one readable multi-line string — the summary
/// header (time / op / outcome) plus every detail line, i.e. exactly what the
/// expanded row shows, including the tail the summary row truncates away.
/// Pure (no gpui, no `cx`) so it is unit-testable.
pub fn entry_clipboard_text(entry: &OpLogEntry) -> String {
    let mut out = format!(
        "{}  {}  {}\n",
        super::format_hms(entry.timestamp),
        entry.op,
        outcome_summary(&entry.outcome)
    );
    for line in detail_lines(entry) {
        out.push_str(&line);
        out.push('\n');
    }
    out
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

    /// Issue #468: the clipboard text carries the whole entry — the header the
    /// summary row truncates AND every detail line, long `error:` included.
    #[test]
    fn clipboard_text_carries_the_whole_entry() {
        let long_error = "x".repeat(200);
        let entry = OpLogEntry::new(
            "checkout",
            "repo",
            kagi_git::ops::StateSummary {
                head: "HEAD → main".to_string(),
                dirty: "clean".to_string(),
            },
            OpOutcome::Failed {
                error: long_error.clone(),
            },
        );
        let text = entry_clipboard_text(&entry);
        let lines: Vec<&str> = text.lines().collect();
        // header + before + dirty + error
        assert_eq!(lines.len(), 4, "unexpected shape: {text:?}");
        assert!(lines[0].ends_with(&format!("  checkout  Failed: {long_error}")));
        assert_eq!(lines[1], "  before:  HEAD → main");
        assert_eq!(lines[2], "  dirty:   clean");
        assert_eq!(lines[3], format!("  error:   {long_error}"));
    }

    #[test]
    fn clipboard_text_lists_every_blocker() {
        let entry = OpLogEntry::new(
            "merge",
            "repo",
            kagi_git::ops::StateSummary {
                head: "main".to_string(),
                dirty: "dirty".to_string(),
            },
            OpOutcome::Refused {
                blockers: vec!["uncommitted changes".into(), "detached HEAD".into()],
            },
        );
        let text = entry_clipboard_text(&entry);
        assert!(text.contains("Refused (2 blockers)"), "{text}");
        assert!(text.contains("  blocker: uncommitted changes"), "{text}");
        assert!(text.contains("  blocker: detached HEAD"), "{text}");
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
