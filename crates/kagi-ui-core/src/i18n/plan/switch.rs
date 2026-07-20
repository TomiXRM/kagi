//! JA strings for `SwitchNote` (filled by the ops/switch fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{SwitchNote, SwitchRecovery, SwitchTitle};

/// Japanese rendering of one switch note.
pub fn note_ja(note: &SwitchNote) -> String {
    match *note {}
}

/// Japanese rendering of one switch title.
pub fn title_ja(title: &SwitchTitle) -> String {
    match *title {}
}

/// Japanese rendering of one switch recovery block.
pub fn recovery_ja(recovery: &SwitchRecovery) -> String {
    match *recovery {}
}
