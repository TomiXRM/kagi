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
        let entry = recording.entry();
        let success = matches!(entry.outcome, OpOutcome::Success { .. });
        let recording_failed = matches!(
            recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        let notice = (!recording_failed).then(|| {
            let message: SharedString = match &entry.outcome {
                OpOutcome::Success { .. } => match action {
                    kagi_domain::conflict_family::ConflictAction::Abort => {
                        Msg::ConflictAborted.t().into()
                    }
                    _ => i18n::op::user_text::operations::conflict_saved().into(),
                },
                OpOutcome::Refused { .. } => blocker
                    .map(kagi_ui_core::i18n::plan_note_text)
                    .unwrap_or_else(|| super::outcome_text(&entry.op, &entry.outcome))
                    .into(),
                _ => super::outcome_text(&entry.op, &entry.outcome).into(),
            };
            (
                if success {
                    ToastKind::Success
                } else {
                    ToastKind::Error
                },
                message,
            )
        });
        let shown = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(&recording);
        if action == kagi_domain::conflict_family::ConflictAction::Save {
            if let Some(conflict) = self.ui.get(&owner).and_then(|ui| ui.conflict.as_ref()) {
                conflict.update(cx, |view, cx| {
                    let attempted = view.save_attempt.take();
                    view.save_refused = if success { None } else { attempted };
                    cx.notify();
                });
            }
        }
        if self.active_session() == Some(owner) {
            let contract =
                Self::operation_footer_contract(&entry.op, &entry.before, &entry.outcome);
            let announce = self.operation_result_toast_needed(false);
            self.record_op_result(
                shown,
                cx,
                false,
                None,
                notice,
                announce,
                true,
                None,
                recording_failed,
                None,
                Some(contract),
                None,
                None,
            );
        } else {
            // The receipt remains inspectable without collapsing another
            // repository's current inspection or replacing its footer.
            if let Some(panel) = &self.op_log {
                panel.update(cx, |panel, cx| {
                    panel.push(shown);
                    cx.notify();
                });
            }
            if let Some((kind, message)) = notice {
                let _ = self.push_operation_result(
                    entry,
                    &entry.op,
                    crate::ui::toast_stack::ToastSpec::operation(entry, kind, message),
                    cx,
                );
            }
        }
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = &recording {
            self.app_notices.push_back(
                super::record::recording_failure_notice(
                    &entry.repo,
                    &entry.op,
                    &recording.entry().outcome,
                    &error.to_string(),
                    None,
                )
                .into(),
            );
        }
    }
}
