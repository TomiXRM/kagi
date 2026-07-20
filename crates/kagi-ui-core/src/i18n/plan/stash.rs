//! JA strings for `StashNote` (filled by the ops/stash fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::StashNote;

/// Japanese rendering of one stash note.
pub fn note_ja(note: &StashNote) -> String {
    match *note {}
}
