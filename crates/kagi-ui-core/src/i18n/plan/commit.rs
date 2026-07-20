//! JA strings for `CommitNote` (filled by the ops/commit fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::CommitNote;

/// Japanese rendering of one commit note.
pub fn note_ja(note: &CommitNote) -> String {
    match *note {}
}
