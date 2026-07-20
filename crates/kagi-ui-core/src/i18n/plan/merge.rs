//! JA strings for `MergeNote` (filled by the ops/merge fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::MergeNote;

/// Japanese rendering of one merge note.
pub fn note_ja(note: &MergeNote) -> String {
    match *note {}
}
