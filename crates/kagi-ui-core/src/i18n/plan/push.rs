//! JA strings for `PushNote` (filled by the ops/push fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::PushNote;

/// Japanese rendering of one push note.
pub fn note_ja(note: &PushNote) -> String {
    match *note {}
}
