//! Apply a PR review suggestion to the working tree (#351, ADR-0209).
//!
//! "Apply suggestion…" on a review conversation entry only *plans*: the plan
//! card says whether the working-tree file is the PR head's version of it
//! (the review's line numbers are the head's) and how to get the old content
//! back. Only a confirm — button or Enter — runs it, through
//! `Backend::run_recorded` like every other write, so it lands in the oplog
//! with its backup ref. Nothing is staged: the change waits in the Commit
//! Panel.

use super::RunPresentation;
use crate::ui::blocking_ops::open_backend;
use crate::ui::modal_renderers::render_plan_modal_wrapper_styled;
use crate::ui::modals::apply_suggestion::ApplySuggestionModal;
use crate::ui::*;
use gpui_component::IconName;
use kagi_domain::suggestion::Suggestion;
use kagi_git::backend::recording::RunReport;
use kagi_git::Operation;

fn apply_suggestion_blocking(
    repo_path: &std::path::Path,
    op: &Operation,
    plan: &kagi_git::OperationPlan,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    Ok(repo.run_recorded(op, plan))
}

impl KagiApp {
    /// Plan applying `suggestion` against the open PR's head and show the
    /// plan card. Nothing is written here.
    pub fn open_apply_suggestion_modal(&mut self, suggestion: Suggestion, _cx: &mut Context<Self>) {
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let head = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|i| m.tabs.get(i)))
            .map(|tab| tab.head.clone())
            .unwrap_or_else(|| kagi_git::CommitId(String::new()));
        let Some(session) = self.ui().repo_session.as_ref() else {
            self.status_footer = FooterStatus::Failed(SharedString::from(i18n::op_plan_failed(
                i18n::Op::ApplySuggestion,
                "repo session unavailable",
            )));
            return;
        };
        let backend = session.backend();
        // A file that is gone or too short captures nothing; the plan then
        // carries the blocker that says why, instead of a bare failure.
        let expected = backend
            .capture_suggestion_context(&suggestion)
            .unwrap_or_default();
        let op = Operation::ApplySuggestion {
            suggestion,
            expected_original: expected,
            head,
        };
        match backend.plan(&op) {
            Ok(plan) => {
                klog!("plan: {} blockers={}", op.oplog_name(), plan.blockers.len());
                self.set_apply_suggestion_modal(ApplySuggestionModal {
                    op,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::ApplySuggestion, e),
                ));
            }
        }
    }

    pub fn cancel_apply_suggestion_modal(&mut self) {
        self.clear_apply_suggestion_modal();
    }

    /// Run the confirmed apply (the modal's button and root Enter both land
    /// here). A blocked plan is refused and recorded, never run.
    pub fn start_apply_suggestion(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.apply_suggestion_modal().cloned() else {
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
        if !modal.plan.blockers.is_empty() {
            klog!("refused: {} plan has blockers, not executing", name);
            self.record_refused(
                name,
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_apply_suggestion_modal();
            cx.notify();
            return;
        }
        self.clear_apply_suggestion_modal();
        klog!("async: {} started", name);
        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let op = modal.op;
        self.finish_run(
            cx,
            name,
            i18n::Op::ApplySuggestion,
            plan,
            repo_path,
            move || apply_suggestion_blocking(&bg_path, &op, &bg_plan),
            |_| None,
            |done| match done {
                Ok(_) => RunPresentation::none().reload(),
                Err(_) => RunPresentation::none(),
            },
        );
    }
}

/// The shared plan card for an apply: the file, the head-version check's
/// blockers, and how to read the old content back from its backup ref.
pub(crate) fn render_apply_suggestion_modal(
    modal: ApplySuggestionModal,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper_styled(
        modal.plan,
        modal.error,
        i18n::Op::ApplySuggestion.t(),
        None,
        Some((IconName::File.into(), theme::theme().color_branch)),
        |this, _cx| this.cancel_apply_suggestion_modal(),
        |this, cx| this.start_apply_suggestion(cx),
        overrides,
        cx,
    )
}
