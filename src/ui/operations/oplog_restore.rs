//! Operation Log restore plans and two-stage execution through run_recorded.

use super::RunPresentation;
use crate::ui::blocking_ops::open_backend;
use crate::ui::modals::oplog_restore::OplogRestoreModal;
use crate::ui::*;
use kagi_domain::ref_restore;
use kagi_git::backend::recording::RunReport;
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::Operation;

fn oplog_restore_blocking(
    repo_path: &std::path::Path,
    op: &Operation,
    plan: &kagi_git::OperationPlan,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    Ok(repo.run_recorded(op, plan))
}

/// The Operation Log panel entity, wired to the app: a selected row's
/// "Revert this operation…" / "Restore to this point…" (`OpLogPanelEvent`)
/// open this card. Built once with the app (`build_kagi_entity`), which the
/// real window and the offscreen mount share.
pub(crate) fn op_log_panel(
    seed: std::collections::VecDeque<kagi_git::oplog::OpLogEntry>,
    cx: &mut Context<KagiApp>,
) -> Entity<crate::ui::oplog_panel::OpLogPanel> {
    use crate::ui::oplog_panel::{OpLogPanel, OpLogPanelEvent};
    let panel = cx.new(|_| OpLogPanel::from_entries(seed));
    cx.subscribe(
        &panel,
        |app, _panel, event: &OpLogPanelEvent, cx| match event {
            OpLogPanelEvent::Restore(op) => app.open_oplog_restore_modal(op.clone(), cx),
        },
    )
    .detach();
    panel
}

impl KagiApp {
    /// Plan `op` (`OpRevert` / `RestoreToPoint`) for the active repository and
    /// show the card. Nothing is written here.
    pub fn open_oplog_restore_modal(&mut self, op: Operation, cx: &mut Context<Self>) {
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let i18n_op = match op {
            Operation::OpRevert { .. } => i18n::Op::OpRevert,
            _ => i18n::Op::RestoreToPoint,
        };
        let Some(session) = self.ui().repo_session.as_ref() else {
            self.status_footer = FooterStatus::Failed(SharedString::from(i18n::op_plan_failed(
                i18n_op,
                "repo session unavailable",
            )));
            return;
        };
        match session.backend().plan(&op) {
            Ok(plan) => self.admit_oplog_restore_plan(op, plan, cx),
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(i18n::op_plan_failed(i18n_op, e)));
            }
        }
    }
    /// Admission boundary shared by the real backend plan and the malformed
    /// canonical-row regression scenario. No restore runs while opening it.
    fn admit_oplog_restore_plan(
        &mut self,
        op: Operation,
        plan: kagi_git::OperationPlan,
        cx: &mut Context<Self>,
    ) {
        klog!(
            "plan: {} blockers={} warnings={}",
            op.oplog_name(),
            plan.blockers.len(),
            plan.warnings.len()
        );
        let restores = match ref_restore::from_lines(&plan.preview_commits) {
            Ok(restores) => restores,
            Err(error) => {
                let reason = format!("Restore plan ref rows could not be decoded: {error}");
                if let Some(path) = self.repo_path.clone() {
                    let entry = OpLogEntry::new(
                        op.oplog_name(),
                        path.display().to_string(),
                        plan.current.clone(),
                        OpOutcome::Failed { error: reason },
                    )
                    .with_nothing_moved();
                    self.record_op_impl(
                        entry,
                        cx,
                        true,
                        Some("Restore plan ref rows are invalid; see the Operation Log".into()),
                    );
                } else {
                    self.push_toast(
                        ToastKind::Error,
                        "Restore plan ref rows are invalid; no repository is open",
                        cx,
                    );
                }
                return;
            }
        };
        // A blocked restore cannot be run; keep the safety reasons ahead of
        // speculative graph context in a height-capped confirmation card.
        let preview = plan
            .blockers
            .is_empty()
            .then(|| {
                super::oplog_restore_preview::build(self.view(), &restores, &plan.head_at_plan)
            })
            .flatten();
        self.set_oplog_restore_modal(OplogRestoreModal {
            op,
            plan: std::sync::Arc::new(plan),
            restores,
            error: None,
            confirm_armed: false,
            preview,
        });
        self.focus_root_for_modal();
        cx.notify();
    }

    #[cfg(feature = "gui-e2e")]
    pub fn admit_oplog_restore_plan_for_test(
        &mut self,
        op: Operation,
        plan: kagi_git::OperationPlan,
        cx: &mut Context<Self>,
    ) {
        self.admit_oplog_restore_plan(op, plan, cx);
    }

    pub fn cancel_oplog_restore_modal(&mut self) {
        self.clear_oplog_restore_modal();
    }

    /// The card's confirm and root Enter both land here: refuse a blocked
    /// plan (recorded), arm on the first confirm, run on the second.
    pub fn start_oplog_restore(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.oplog_restore_modal().cloned() else {
            return;
        };
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let name = modal.op.oplog_name();
        if !modal.plan.blockers.is_empty() || modal.restores.is_empty() {
            klog!("refused: {} plan has blockers, not executing", name);
            self.record_refused(
                name,
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_oplog_restore_modal();
            cx.notify();
            return;
        }
        if !modal.confirm_armed {
            self.set_oplog_restore_modal(OplogRestoreModal {
                confirm_armed: true,
                ..modal
            });
            klog!("{}: armed (second confirm required — destructive)", name);
            cx.notify();
            return;
        }
        self.clear_oplog_restore_modal();
        klog!("async: {} started", name);
        let i18n_op = modal.i18n_op();
        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let op = modal.op;
        self.finish_run(
            cx,
            name,
            i18n_op,
            plan,
            repo_path,
            move || oplog_restore_blocking(&bg_path, &op, &bg_plan),
            |_| None,
            |done| match done {
                Ok(_) => RunPresentation::none().reload(),
                Err(_) => RunPresentation::none(),
            },
        );
    }
}

/// REFS-first restore card; render from canonical typed ref moves.
pub(crate) fn render_oplog_restore_modal(
    modal: OplogRestoreModal,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    super::oplog_restore_card::render(modal, overrides, cx)
}
