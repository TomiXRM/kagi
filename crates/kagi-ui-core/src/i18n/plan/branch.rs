//! JA strings for `BranchNote` (filled by the ops/branch fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::BranchNote;

/// Japanese rendering of one branch note.
pub fn note_ja(note: &BranchNote) -> String {
    match *note {}
}
