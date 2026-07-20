//! JA strings for `PullNote` (filled by the ops/pull fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::PullNote;

/// Japanese rendering of one pull note.
pub fn note_ja(note: &PullNote) -> String {
    match *note {}
}
