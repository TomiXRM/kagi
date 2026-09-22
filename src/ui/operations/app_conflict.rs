//! Conflict-family receipt presentation; footer logs remain the English contract.
use crate::app;
use crate::ui::*;

impl KagiApp {
    pub(super) fn deliver_conflict_result(
        &mut self,
        attachment: app::Attachment,
        report: kagi_git::backend::conflict_ops::ConflictReport,
        cx: &mut Context<Self>,
    ) {
        // The abort's long-standing contract line (ADR-0056, documented in
        // T-ENTITY-CONFLICT-001). #704 moved the execution into the family, so
        // it is emitted from the receipt instead of from the UI's own run.
        if matches!(
            report.evidence.action,
            kagi_domain::conflict_family::ConflictAction::Abort
        ) && matches!(report.recording.entry().outcome, OpOutcome::Success { .. })
        {
            klog!("executed: {}", report.recording.entry().op);
        }
        self.present_conflict_action(
            attachment.session,
            report.evidence.action,
            report.recording,
            report.blocker.as_ref(),
            cx,
        );
    }

    /// Present one conflict-family receipt. The action picks the success
    /// wording: an abort saves nothing, so announcing it as "解決を保存しました"
    /// was simply false (#704 review).
    pub(crate) fn present_conflict_action(
        &mut self,
        owner: app::SessionId,
        action: kagi_domain::conflict_family::ConflictAction,
        recording: kagi_git::backend::recording::Recording,
        blocker: Option<&kagi_domain::plan_note::PlanNote>,
        cx: &mut Context<Self>,
    ) {
        let entry = recording.entry().clone();
        let success = matches!(entry.outcome, OpOutcome::Success { .. });
        let summary = match &entry.outcome {
            OpOutcome::Refused { blockers } => blocker
                .map(kagi_ui_core::i18n::plan_note_text)
                .unwrap_or_else(|| blockers.join("\n")),
            _ => oplog_panel::outcome_summary(&entry.outcome),
        };
        if let Some(panel) = &self.op_log {
            panel.update(cx, |panel, cx| {
                panel.push(entry.clone());
                cx.notify();
            });
        }
        if matches!(entry.outcome, OpOutcome::Refused { .. }) {
            self.app_notices
                .push_back(format!("{}: {}", entry.repo, summary).into());
        }
        self.push_toast(
            if success {
                ToastKind::Success
            } else {
                ToastKind::Error
            },
            match (success, action) {
                (true, kagi_domain::conflict_family::ConflictAction::Abort) => {
                    Msg::ConflictAborted.t().to_string()
                }
                (true, _) => Msg::EditorSavedResolved.t().to_string(),
                (false, _) => summary.clone(),
            },
            cx,
        );
        if self.active_session() == Some(owner) {
            let footer = match &entry.outcome {
                OpOutcome::Success { after } => {
                    format!("{}: {} → {}", entry.op, entry.before.head, after.head)
                }
                OpOutcome::Partial { error, .. }
                | OpOutcome::Unknown {
                    evidence: error, ..
                } => {
                    format!("{}: partially applied — {}", entry.op, error)
                }
                OpOutcome::Failed { error } => format!("{}: failed — {}", entry.op, error),
                OpOutcome::Refused { blockers } => format!(
                    "{}: refused ({} blocker{})",
                    entry.op,
                    blockers.len(),
                    if blockers.len() == 1 { "" } else { "s" }
                ),
            };
            klog!("footer: {}", footer);
            self.status_footer = if success {
                FooterStatus::Success(footer.into())
            } else {
                FooterStatus::Failed(footer.into())
            };
        }
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = recording {
            self.app_notices
                .push_back(format!("{}: recording failed: {}", entry.repo, error).into());
        }
    }
}
