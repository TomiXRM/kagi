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

fn recording_outcome(outcome: &OpOutcome) -> i18n::op::RecordingOutcome {
    use i18n::op::RecordingOutcome;
    match outcome {
        OpOutcome::Success { .. } => RecordingOutcome::Succeeded,
        OpOutcome::Failed { .. } => RecordingOutcome::Failed,
        OpOutcome::Refused { .. } => RecordingOutcome::Refused,
        OpOutcome::Partial { .. } => RecordingOutcome::Partial,
        OpOutcome::Unknown { .. } => RecordingOutcome::Unknown,
    }
}

/// The mandatory acknowledgement is the sole result when its receipt was
/// not saved. Keep the original diagnostic here too: there is no durable
/// receipt to fall back to, and a successful append must never be implied.
/// A borrowed, observed conflict stop changes only the displayed Success
/// classification; the receipt and all uncertain or failed outcomes stay intact.
pub(crate) fn recording_failure_notice(
    repo: impl std::fmt::Display,
    op: &str,
    outcome: &OpOutcome,
    error: &str,
    stopped_notice: Option<&str>,
) -> String {
    let stopped_notice = stopped_notice.filter(|_| matches!(outcome, OpOutcome::Success { .. }));
    let displayed_outcome = if stopped_notice.is_some() {
        i18n::op::RecordingOutcome::Partial
    } else {
        recording_outcome(outcome)
    };
    let mut notice =
        i18n::op::user_text::operations::recording_failed(repo, op, displayed_outcome, error);
    if let Some(stopped) = stopped_notice {
        notice.push_str("\n\n");
        notice.push_str(stopped);
    }
    let details: &[String] = match outcome {
        OpOutcome::Failed { error } | OpOutcome::Partial { error, .. } => {
            std::slice::from_ref(error)
        }
        OpOutcome::Unknown { evidence, .. } => std::slice::from_ref(evidence),
        OpOutcome::Refused { blockers } => blockers,
        OpOutcome::Success { .. } => &[],
    };
    for detail in details
        .iter()
        .map(String::as_str)
        .filter(|detail| !detail.trim().is_empty() && *detail != error)
    {
        notice.push_str("\n\n");
        notice.push_str(detail);
    }
    notice
}

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

    /// Present the mutation owner's attempted receipt without appending again.
    /// Quiet completions retain the live footer/toast presentation owner.
    /// `stopped` comes from the typed sequencer result for this request; it
    /// changes only the success presentation, never the recorded outcome.
    /// `display` carries a typed backend refusal's localized preview separately
    /// from the receipt's stable English blocker.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui) fn present_operation_completion(
        &mut self,
        recording: &kagi_git::backend::recording::Recording,
        current: bool,
        stopped: bool,
        display: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        let op = entry.op.clone();
        let repo_path = std::path::Path::new(&entry.repo);
        // A failed append makes the display row Partial, not the execution.
        let stopped = stopped && matches!(&recording.entry().outcome, OpOutcome::Success { .. });
        let recording_failed = matches!(
            recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = recording {
            klog!("oplog: write failed (non-fatal): {}", error);
            let stopped_notice = stopped.then(|| i18n::Msg::conflict_stopped(None));
            self.notice_recording_failure(&op, recording, repo_path, stopped_notice.as_deref());
        }
        if current {
            let announce = self.operation_result_toast_needed(false);
            return self
                .record_op_result(
                    entry,
                    cx,
                    false,
                    None,
                    display.map(|message| (crate::ui::ToastKind::Error, message.into())),
                    announce,
                    true,
                    None,
                    recording_failed,
                    stopped.then(|| i18n::Msg::conflict_stopped(None).into()),
                    None,
                )
                .recording_failed;
        }
        let (footer_msg, _) =
            Self::operation_footer_contract(&entry.op, &entry.before, &entry.outcome);
        let failed = !matches!(
            &entry.outcome,
            OpOutcome::Success { .. } | OpOutcome::Unknown { .. }
        );
        if let Some(panel) = self.op_log.clone() {
            let shown = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
            panel.update(cx, |panel, cx| {
                panel.push(shown);
                panel.collapse();
                cx.notify();
            });
        }
        // The footer contract is logged for the frozen owner. Important stops
        // and failures use the global result owner, never the foreign footer.
        klog!("footer: {}", footer_msg);
        if (stopped || failed) && !recording_failed {
            let entry = recording.entry();
            let (kind, message) = if stopped {
                (
                    crate::ui::ToastKind::Info,
                    i18n::Msg::conflict_stopped(None),
                )
            } else {
                (
                    crate::ui::ToastKind::Error,
                    display.unwrap_or_else(|| super::outcome_text(&op, &entry.outcome)),
                )
            };
            let _ = self.push_operation_result(entry, &op, kind, message.into(), cx);
        }
        recording_failed
    }

    /// [`Self::record_op_persist`] for a write that by construction moves no
    /// ref — worktree lock / unlock / prune / repair only touch worktree admin
    /// files (#907 review): recorded, nothing moved (`with_nothing_moved`).
    /// Not observed: a branch someone else moves meanwhile is not its move.
    pub(in crate::ui) fn record_op_persist_nothing_moved(
        &mut self,
        op: &str,
        before: StateSummary,
        outcome: OpOutcome,
        repo_path: &std::path::Path,
        success_display: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let notice = success_display
            .filter(|_| matches!(&outcome, OpOutcome::Success { .. }))
            .map(|text| (crate::ui::ToastKind::Success, text.into()));
        let entry = OpLogEntry::new(op, repo_path.display().to_string(), before, outcome)
            .with_nothing_moved();
        self.record_op_with_notice(entry, cx, true, None, notice, false);
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
        let entry = Self::refused_entry(op, before, blockers, repo_path);
        self.record_refused_entry(op, entry, blockers, cx);
    }

    /// The entry [`Self::record_refused`] records, for a caller that adds to
    /// it — an issue-create carries the fields it asked for (#904 review).
    pub(crate) fn refused_entry(
        op: &str,
        before: StateSummary,
        blockers: &[PlanNote],
        repo_path: &std::path::Path,
    ) -> OpLogEntry {
        let outcome = OpOutcome::Refused {
            blockers: blockers.iter().map(PlanNote::message_en).collect(),
        };
        OpLogEntry::new(op, repo_path.display().to_string(), before, outcome)
    }

    /// Record a refusal entry built by [`Self::refused_entry`].
    pub(crate) fn record_refused_entry(
        &mut self,
        op: &str,
        entry: OpLogEntry,
        blockers: &[PlanNote],
        cx: &mut Context<Self>,
    ) {
        self.record_op_impl(entry, cx, true, typed_refusal(op, blockers));
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
        self.notice_recording_failure(op, &report.recording, repo_path, None);
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(&report.recording);
        let announce = self.operation_result_toast_needed(false);
        self.record_op_result(
            entry,
            cx,
            false,
            typed_refusal(op, blockers),
            None,
            announce,
            true,
            None,
            matches!(
                report.recording,
                kagi_git::backend::recording::Recording::Failed { .. }
            ),
            None,
            None,
        );
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
        let first = i18n::op::user_text::diagnostic("uncommitted changes");
        let second = i18n::op::user_text::diagnostic("detached HEAD");
        assert!(shown.contains(first.as_ref()), "{shown}");
        assert!(!shown.contains(second.as_ref()), "{shown}");
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
