//! JA strings for `PushNote` (filled by the ops/push fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{PushNote, PushRecovery, PushTitle};

/// Japanese rendering of one push note.
pub fn note_ja(note: &PushNote) -> String {
    match *note {}
}

/// Japanese rendering of one push title.
pub fn title_ja(title: &PushTitle) -> String {
    match *title {}
}

/// Japanese rendering of one push recovery block.
pub fn recovery_ja(recovery: &PushRecovery) -> String {
    match *recovery {}
}
