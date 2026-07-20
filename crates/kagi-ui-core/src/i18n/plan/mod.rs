//! Plan-text localization (ADR-0129 §3).
//!
//! `plan_note_text` / `plan_title_text` / `plan_recovery_text` are the ONLY
//! way the UI renders plan text. EN always delegates to the kagi-domain
//! `message_en()` renderer (no double-maintenance); JA strings live in the
//! per-category submodules (`plan/discard.rs`, …) added as Phase 2 structures
//! each producer. `Verbatim` notes render their payload untranslated in every
//! language — they disappear in Phase 3.

pub mod discard;

use kagi_domain::plan_note::{PlanNote, PlanRecovery, PlanTitle, RecoveryKind};

use super::{lang, Lang};

/// Localized text for one plan note (blocker / warning).
pub fn plan_note_text(note: &PlanNote) -> String {
    match lang() {
        Lang::En => note.message_en(),
        Lang::Ja => match note {
            PlanNote::Verbatim(s) => s.clone(),
            PlanNote::Discard(n) => discard::note_ja(n),
        },
    }
}

/// Localized text for the plan title.
pub fn plan_title_text(title: &PlanTitle) -> String {
    match lang() {
        Lang::En => title.message_en(),
        Lang::Ja => match title {
            PlanTitle::Verbatim(s) => s.clone(),
            PlanTitle::Discard { .. } => discard::title_ja(title),
        },
    }
}

/// Localized text for the recovery block. `None` renders empty (legacy plans
/// always carry `Some`; the Option exists for future no-recovery plans).
pub fn plan_recovery_text(recovery: Option<&PlanRecovery>) -> String {
    let Some(r) = recovery else {
        return String::new();
    };
    match lang() {
        Lang::En => r.message_en(),
        Lang::Ja => match &r.kind {
            RecoveryKind::Verbatim(s) => s.clone(),
            RecoveryKind::Discard => discard::recovery_ja(),
        },
    }
}
