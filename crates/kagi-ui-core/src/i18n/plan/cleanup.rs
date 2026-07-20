//! JA strings for `CleanupNote` (filled by the ops/cleanup fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{CleanupNote, CleanupRecovery, CleanupTitle};

/// Japanese rendering of one cleanup note.
pub fn note_ja(note: &CleanupNote) -> String {
    match *note {}
}

/// Japanese rendering of one cleanup title.
pub fn title_ja(title: &CleanupTitle) -> String {
    match *title {}
}

/// Japanese rendering of one cleanup recovery block.
pub fn recovery_ja(recovery: &CleanupRecovery) -> String {
    match *recovery {}
}
