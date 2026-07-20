//! JA strings for `StashNote` (filled by the ops/stash fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{StashNote, StashRecovery, StashTitle};

/// Japanese rendering of one stash note.
pub fn note_ja(note: &StashNote) -> String {
    match *note {}
}

/// Japanese rendering of one stash title.
pub fn title_ja(title: &StashTitle) -> String {
    match *title {}
}

/// Japanese rendering of one stash recovery block.
pub fn recovery_ja(recovery: &StashRecovery) -> String {
    match *recovery {}
}
