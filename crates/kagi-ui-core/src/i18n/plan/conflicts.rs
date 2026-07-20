//! JA strings for `ConflictsNote` (filled by the ops/conflicts fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::ConflictsNote;

/// Japanese rendering of one conflicts note.
pub fn note_ja(note: &ConflictsNote) -> String {
    match *note {}
}
