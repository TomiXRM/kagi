//! JA strings for `CheckoutNote` (filled by the ops/checkout fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{CheckoutNote, CheckoutRecovery, CheckoutTitle};

/// Japanese rendering of one checkout note.
pub fn note_ja(note: &CheckoutNote) -> String {
    match *note {}
}

/// Japanese rendering of one checkout title.
pub fn title_ja(title: &CheckoutTitle) -> String {
    match *title {}
}

/// Japanese rendering of one checkout recovery block.
pub fn recovery_ja(recovery: &CheckoutRecovery) -> String {
    match *recovery {}
}
