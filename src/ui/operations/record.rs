//! UI-side operation recording (moved from `ui/mod.rs`) and #353's refusal
//! presentation: a refused operation says why, not how many.
//!
//! A plan-time refusal used to reach the footer and toast as
//! `"<op>: refused (N blockers)"`, which left the reason in a collapsed
//! Operation Log row — and a root Enter on a blocked plan closes the modal
//! that showed it. The footer and the bounded toast now carry the **first**
//! blocker (in the user's language when it is typed) plus how many more the
//! Operation Log holds; the durable entry still lists every blocker in
//! English, and the `[kagi] footer:` contract line keeps its count wording.
use crate::ui::i18n;
use crate::ui::KagiApp;
use gpui::Context;
use kagi_domain::plan_note::PlanNote;
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::ops::StateSummary;

impl KagiApp {
    /// UI-side operation recording (toast / footer / history panel).
    ///
    /// ADR-0149: for ops routed through `Backend::run`, `run` is now the sole
    /// oplog writer, so this path does NOT append the oplog for
    /// Success/Partial/Failed (that would double-record). It DOES still append
    /// `Refused` outcomes — those are rejected at plan time and never reach
    /// `run`, so the UI remains their recorder. A refusal with typed blockers
    /// goes through [`Self::record_refused`] instead.
    pub(in crate::ui) fn record_op(
        &mut self,
        op: &str,
        before: StateSummary,
        outcome: OpOutcome,
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let persist = matches!(outcome, OpOutcome::Refused { .. });
        let entry = OpLogEntry::new(op, repo_path.display().to_string(), before, outcome);
        self.record_op_impl(entry, cx, persist, None);
    }

    /// Like [`Self::record_op`] but ALSO persists entries for operations whose
    /// execution boundary does not yet record them (conflict resolution,
    /// terminal start, PR merge). Backend-owned non-run operations such as
    /// branch cleanup use presentation-only `record_op`. ADR-0149 §"non-run ops".
    pub(in crate::ui) fn record_op_persist(
        &mut self,
        op: &str,
        before: StateSummary,
        outcome: OpOutcome,
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let entry = OpLogEntry::new(op, repo_path.display().to_string(), before, outcome);
        self.record_op_impl(entry, cx, true, None);
    }

    /// Record a refusal whose blockers are typed plan notes: every blocker
    /// goes to the oplog, the first one (localized) to the footer and toast.
    pub(crate) fn record_refused(
        &mut self,
        op: &str,
        before: StateSummary,
        blockers: &[PlanNote],
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let shown = typed_refusal(op, blockers);
        let outcome = OpOutcome::Refused {
            blockers: blockers.iter().map(PlanNote::message_en).collect(),
        };
        let entry = OpLogEntry::new(op, repo_path.display().to_string(), before, outcome);
        self.record_op_impl(entry, cx, true, shown);
    }

    /// Present a no-execute receipt the core already wrote for a blocked plan
    /// (the local pull), naming its first typed blocker like
    /// [`Self::record_refused`] does. Nothing is appended here.
    pub(crate) fn present_refused_report(
        &mut self,
        op: &str,
        report: &kagi_git::backend::recording::RunReport,
        blockers: &[PlanNote],
        repo_path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        self.notice_recording_failure(op, &report.recording, repo_path);
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(&report.recording);
        self.record_op_impl(entry, cx, false, typed_refusal(op, blockers));
    }
}

/// Footer / toast text naming the first typed blocker in the user's language.
pub(crate) fn typed_refusal(op: &str, blockers: &[PlanNote]) -> Option<String> {
    refused_text(
        op,
        blockers.iter().map(i18n::plan_note_text),
        blockers.len(),
    )
}

/// The `[kagi] footer:` contract text for a refusal. Its wording is a test
/// contract; the user-facing text is [`refused_display`].
pub(crate) fn refused_contract(op: &str, count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{op}: refused ({count} blocker{plural})")
}

/// Footer / toast text for a refusal known only by its recorded blocker
/// strings (a receipt, a remote diagnostic, a runtime refusal). `None` for any
/// other outcome, or a refusal that recorded no blocker.
pub(crate) fn refused_display(op: &str, outcome: &OpOutcome) -> Option<String> {
    match outcome {
        OpOutcome::Refused { blockers } => {
            refused_text(op, blockers.iter().cloned(), blockers.len())
        }
        _ => None,
    }
}

fn refused_text(
    op: &str,
    mut reasons: impl Iterator<Item = String>,
    count: usize,
) -> Option<String> {
    let first = reasons.next()?;
    Some(i18n::op_refused(op, first, count - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_shows_its_first_blocker_and_counts_the_rest() {
        let outcome = OpOutcome::Refused {
            blockers: vec!["uncommitted changes".into(), "detached HEAD".into()],
        };
        let shown = refused_display("checkout", &outcome).unwrap();
        assert!(shown.contains("uncommitted changes"), "{shown}");
        assert!(!shown.contains("detached HEAD"), "{shown}");
        assert!(
            shown.contains('1'),
            "the second blocker is counted: {shown}"
        );
        assert_eq!(
            refused_contract("checkout", 2),
            "checkout: refused (2 blockers)"
        );
        assert_eq!(
            refused_contract("checkout", 1),
            "checkout: refused (1 blocker)"
        );
    }

    #[test]
    fn only_a_refusal_with_a_blocker_has_a_reason() {
        let empty = OpOutcome::Refused { blockers: vec![] };
        assert_eq!(refused_display("checkout", &empty), None);
        let failed = OpOutcome::Failed {
            error: "boom".into(),
        };
        assert_eq!(refused_display("checkout", &failed), None);
    }
}
