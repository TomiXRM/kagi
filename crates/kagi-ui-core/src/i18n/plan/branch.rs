//! JA strings for `BranchNote` (filled by the ops/branch fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{BranchNote, BranchRecovery, BranchTitle};

/// Japanese rendering of one branch note.
pub fn note_ja(note: &BranchNote) -> String {
    match *note {}
}

/// Japanese rendering of one branch title.
pub fn title_ja(title: &BranchTitle) -> String {
    match *title {}
}

/// Japanese rendering of one branch recovery block.
pub fn recovery_ja(recovery: &BranchRecovery) -> String {
    match *recovery {}
}
