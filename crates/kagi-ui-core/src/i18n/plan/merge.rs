//! JA strings for `MergeNote` (filled by the ops/merge fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{MergeNote, MergeRecovery, MergeTitle};

/// Japanese rendering of one merge note.
pub fn note_ja(note: &MergeNote) -> String {
    match *note {}
}

/// Japanese rendering of one merge title.
pub fn title_ja(title: &MergeTitle) -> String {
    match *title {}
}

/// Japanese rendering of one merge recovery block.
pub fn recovery_ja(recovery: &MergeRecovery) -> String {
    match *recovery {}
}
