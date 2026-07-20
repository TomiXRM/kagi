//! JA strings for `CleanupNote` (filled by the ops/cleanup fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::CleanupNote;

/// Japanese rendering of one cleanup note.
pub fn note_ja(note: &CleanupNote) -> String {
    match *note {}
}
