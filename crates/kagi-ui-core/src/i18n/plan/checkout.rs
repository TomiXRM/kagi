//! JA strings for `CheckoutNote` (filled by the ops/checkout fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::CheckoutNote;

/// Japanese rendering of one checkout note.
pub fn note_ja(note: &CheckoutNote) -> String {
    match *note {}
}
