//! JA strings for `HistoryNote` (filled by the ops/history fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{HistoryNote, HistoryRecovery, HistoryTitle};

/// Japanese rendering of one history note.
pub fn note_ja(note: &HistoryNote) -> String {
    match *note {}
}

/// Japanese rendering of one history title.
pub fn title_ja(title: &HistoryTitle) -> String {
    match *title {}
}

/// Japanese rendering of one history recovery block.
pub fn recovery_ja(recovery: &HistoryRecovery) -> String {
    match *recovery {}
}
