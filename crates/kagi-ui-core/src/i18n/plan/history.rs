//! JA strings for `HistoryNote` (filled by the ops/history fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::HistoryNote;

/// Japanese rendering of one history note.
pub fn note_ja(note: &HistoryNote) -> String {
    match *note {}
}
