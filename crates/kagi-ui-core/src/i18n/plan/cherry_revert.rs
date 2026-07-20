//! JA strings for `CherryRevertNote` (filled by the ops/cherry_revert fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{CherryRevertNote, CherryRevertRecovery, CherryRevertTitle};

/// Japanese rendering of one cherry_revert note.
pub fn note_ja(note: &CherryRevertNote) -> String {
    match *note {}
}

/// Japanese rendering of one cherry_revert title.
pub fn title_ja(title: &CherryRevertTitle) -> String {
    match *title {}
}

/// Japanese rendering of one cherry_revert recovery block.
pub fn recovery_ja(recovery: &CherryRevertRecovery) -> String {
    match *recovery {}
}
