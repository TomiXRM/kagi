//! JA strings for `ConflictsNote` (filled by the ops/conflicts fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{ConflictsNote, ConflictsRecovery, ConflictsTitle};

/// Japanese rendering of one conflicts note.
pub fn note_ja(note: &ConflictsNote) -> String {
    match *note {}
}

/// Japanese rendering of one conflicts title.
pub fn title_ja(title: &ConflictsTitle) -> String {
    match *title {}
}

/// Japanese rendering of one conflicts recovery block.
pub fn recovery_ja(recovery: &ConflictsRecovery) -> String {
    match *recovery {}
}
