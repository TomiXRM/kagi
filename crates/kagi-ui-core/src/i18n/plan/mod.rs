//! Plan-text localization (ADR-0129 §3).
//!
//! `plan_note_text` / `plan_title_text` / `plan_recovery_text` are the ONLY
//! way the UI renders plan text. EN always delegates to the kagi-domain
//! `message_en()` renderer (no double-maintenance); JA strings live in the
//! per-category submodules (`plan/discard.rs`, …) added as Phase 2 structures
//! each producer. `Verbatim` notes render their payload untranslated in every
//! language — they disappear in Phase 3.

pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod cleanup;
pub mod commit;
pub mod common;
pub mod conflicts;
pub mod discard;
pub mod history;
pub mod merge;
pub mod pull;
pub mod push;
pub mod stash;
pub mod switch;
pub mod worktree;

use kagi_domain::plan_note::{PlanNote, PlanRecovery, PlanTitle, RecoveryKind};

use super::{lang, Lang};

/// Localized text for one plan note (blocker / warning).
pub fn plan_note_text(note: &PlanNote) -> String {
    match lang() {
        Lang::En => note.message_en(),
        Lang::Ja => match note {
            PlanNote::Common(n) => common::note_ja(n),
            PlanNote::Discard(n) => discard::note_ja(n),
            PlanNote::Branch(n) => branch::note_ja(n),
            PlanNote::Stash(n) => stash::note_ja(n),
            PlanNote::History(n) => history::note_ja(n),
            PlanNote::Pull(n) => pull::note_ja(n),
            PlanNote::Push(n) => push::note_ja(n),
            PlanNote::Switch(n) => switch::note_ja(n),
            PlanNote::Checkout(n) => checkout::note_ja(n),
            PlanNote::Merge(n) => merge::note_ja(n),
            PlanNote::Worktree(n) => worktree::note_ja(n),
            PlanNote::CherryRevert(n) => cherry_revert::note_ja(n),
            PlanNote::Cleanup(n) => cleanup::note_ja(n),
            PlanNote::Conflicts(n) => conflicts::note_ja(n),
            PlanNote::Commit(n) => commit::note_ja(n),
            PlanNote::Verbatim(s) => s.clone(),
        },
    }
}

/// Localized text for the plan title.
pub fn plan_title_text(title: &PlanTitle) -> String {
    match lang() {
        Lang::En => title.message_en(),
        Lang::Ja => match title {
            PlanTitle::Branch(t) => branch::title_ja(t),
            PlanTitle::Stash(t) => stash::title_ja(t),
            PlanTitle::History(t) => history::title_ja(t),
            PlanTitle::Pull(t) => pull::title_ja(t),
            PlanTitle::Push(t) => push::title_ja(t),
            PlanTitle::Switch(t) => switch::title_ja(t),
            PlanTitle::Checkout(t) => checkout::title_ja(t),
            PlanTitle::Merge(t) => merge::title_ja(t),
            PlanTitle::Worktree(t) => worktree::title_ja(t),
            PlanTitle::CherryRevert(t) => cherry_revert::title_ja(t),
            PlanTitle::Cleanup(t) => cleanup::title_ja(t),
            PlanTitle::Conflicts(t) => conflicts::title_ja(t),
            PlanTitle::Commit(t) => commit::title_ja(t),
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
            RecoveryKind::Branch(r) => branch::recovery_ja(r),
            RecoveryKind::Stash(r) => stash::recovery_ja(r),
            RecoveryKind::History(r) => history::recovery_ja(r),
            RecoveryKind::Pull(r) => pull::recovery_ja(r),
            RecoveryKind::Push(r) => push::recovery_ja(r),
            RecoveryKind::Switch(r) => switch::recovery_ja(r),
            RecoveryKind::Checkout(r) => checkout::recovery_ja(r),
            RecoveryKind::Merge(r) => merge::recovery_ja(r),
            RecoveryKind::Worktree(r) => worktree::recovery_ja(r),
            RecoveryKind::CherryRevert(r) => cherry_revert::recovery_ja(r),
            RecoveryKind::Cleanup(r) => cleanup::recovery_ja(r),
            RecoveryKind::Conflicts(r) => conflicts::recovery_ja(r),
            RecoveryKind::Commit(r) => commit::recovery_ja(r),
            RecoveryKind::Verbatim(s) => s.clone(),
            RecoveryKind::Discard => discard::recovery_ja(),
        },
    }
}
