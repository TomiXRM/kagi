//! Repository-health fixes offered by Analyze (#358, ADR-0205).
//!
//! The Health axis only *asks*: its "Enable…" button plans the fix here and
//! opens the shared plan card (title, what changes, the equivalent git
//! command and how to undo it). Only a confirm — button or Enter — runs it,
//! through `Backend::run_recorded` like every other write.

use super::RunPresentation;
use crate::ui::blocking_ops::open_backend;
use crate::ui::modal_renderers::render_plan_modal_wrapper_styled;
use crate::ui::modals::repo_health::RepoHealthModal;
use crate::ui::*;
use gpui_component::IconName;
use kagi_domain::plan_note::MaintenanceTitle;
use kagi_domain::repo_health::HealthFix;
use kagi_git::backend::recording::RunReport;
use kagi_git::Operation;

fn operation(fix: HealthFix) -> Operation {
    match fix {
        HealthFix::WriteCommitGraph => Operation::WriteCommitGraph,
        HealthFix::EnableFsmonitor => Operation::EnableFsmonitor,
    }
}

fn op_label(fix: HealthFix) -> i18n::Op {
    match fix {
        HealthFix::WriteCommitGraph => i18n::Op::WriteCommitGraph,
        HealthFix::EnableFsmonitor => i18n::Op::EnableFsmonitor,
    }
}

fn repo_health_blocking(
    repo_path: &std::path::Path,
    op: &Operation,
    plan: &kagi_git::OperationPlan,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    Ok(repo.run_recorded(op, plan))
}

impl KagiApp {
    /// Plan `fix` and open its confirmation. Nothing is written here.
    pub fn open_repo_health_modal(&mut self, fix: HealthFix) {
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let op = operation(fix);
        let Some(session) = self.ui().repo_session.as_ref() else {
            self.status_footer = FooterStatus::Failed(SharedString::from(i18n::op_plan_failed(
                op_label(fix),
                "repo session unavailable",
            )));
            return;
        };
        match session.backend().plan(&op) {
            Ok(plan) => {
                klog!("plan: {} blockers={}", op.oplog_name(), plan.blockers.len());
                self.set_repo_health_modal(RepoHealthModal {
                    fix,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(op_label(fix), e),
                ));
            }
        }
    }

    pub fn cancel_repo_health_modal(&mut self) {
        self.clear_repo_health_modal();
    }

    /// Run the confirmed fix (the modal's button and root Enter both land
    /// here). A blocked plan is refused and recorded, never run.
    pub fn start_repo_health(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.repo_health_modal().cloned() else {
            return;
        };
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let op = operation(modal.fix);
        let name = op.oplog_name();
        if !modal.plan.blockers.is_empty() {
            klog!("refused: {} plan has blockers, not executing", name);
            self.record_refused(
                name,
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_repo_health_modal();
            cx.notify();
            return;
        }
        self.clear_repo_health_modal();
        klog!("async: {} started", name);
        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        self.finish_run(
            cx,
            name,
            op_label(modal.fix),
            plan,
            repo_path,
            move || repo_health_blocking(&bg_path, &op, &bg_plan),
            |_| None,
            |done| match done {
                Ok(_) => RunPresentation::none().refresh_repo_health(),
                Err(_) => RunPresentation::none(),
            },
        );
    }
}

/// The shared plan card for a health fix: what changes, the equivalent git
/// command, and how to undo it.
pub(crate) fn render_repo_health_modal(
    modal: RepoHealthModal,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let confirm_title = match modal.fix {
        HealthFix::WriteCommitGraph => MaintenanceTitle::WriteCommitGraph,
        HealthFix::EnableFsmonitor => MaintenanceTitle::EnableFsmonitor,
    };
    let confirm_label = kagi_ui_core::i18n::plan::maintenance::confirm_label(&confirm_title);
    render_plan_modal_wrapper_styled(
        modal.plan,
        modal.error,
        confirm_label,
        None,
        Some((IconName::Settings.into(), theme::theme().color_branch)),
        |this, _cx| this.cancel_repo_health_modal(),
        |this, cx| this.start_repo_health(cx),
        overrides,
        cx,
    )
}
