//! Reset-current-to-HEAD operation (branch-menu "Advanced / Dangerous" group).
//!
//! Two-stage confirm (mirrors `operations/remote_branch.rs`'s
//! delete-remote-branch `confirm_armed` pattern): the first click only arms
//! the button; the second click runs the reset on a background thread and
//! reloads.

use crate::ui::*;

impl KagiApp {
    /// Open the reset-current-to-HEAD modal for the commit at `target`.
    pub fn open_reset_current_modal(&mut self, target: CommitId, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "reset-current: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_reset_current_to_head(&target) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: reset-current-to-head {} blockers={}",
                    target.short(),
                    plan.blockers.len()
                );
                self.set_reset_current_modal(ResetCurrentModal {
                    target,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "reset-current plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_reset_current_modal(&mut self) {
        self.clear_reset_current_modal();
    }

    /// Two-stage confirm (mirrors `start_delete_remote_branch`): the first
    /// click only arms the button; the second click runs the reset on a
    /// background thread (busy_op) and reloads.
    pub fn start_reset_current(&mut self, cx: &mut Context<Self>) {
        let modal = match self.reset_current_modal().cloned() {
            Some(m) => m,
            None => return,
        };

        if !modal.confirm_armed {
            self.set_reset_current_modal(ResetCurrentModal {
                confirm_armed: true,
                ..modal
            });
            klog!("reset-current: armed (second confirm required — destructive)");
            cx.notify();
            return;
        }

        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: reset-current plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "reset-current",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_reset_current_modal();
            cx.notify();
            return;
        }

        self.clear_reset_current_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Resetting current branch to {}…",
            modal.target.short()
        )));
        klog!("async: reset-current started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_target = target.clone();
        self.finish_run(
            cx,
            "reset-current",
            i18n::Op::Reset,
            plan.clone(),
            repo_path,
            move || reset_current_blocking(&bg_path, &bg_plan, &bg_target),
            |_| None,
            move |app, done, _cx| match done {
                Ok(_) => {
                    app.status_footer = FooterStatus::Success(SharedString::from(format!(
                        "reset-current: now at {}",
                        target.short()
                    )));
                }
                Err(failure) => {
                    app.set_reset_current_modal(ResetCurrentModal {
                        target: target.clone(),
                        plan: plan.clone(),
                        error: Some(SharedString::from(failure.message)),
                        confirm_armed: false,
                    });
                }
            },
        );
    }
}

/// Blocking `plan → preflight → execute` for the background thread, mirroring
/// `blocking_ops.rs::delete_remote_branch_blocking`. Returns the backend's receipt.
fn reset_current_blocking(
    repo_path: &std::path::Path,
    plan: &kagi_git::ops::OperationPlan,
    target: &CommitId,
) -> Result<kagi_git::backend::recording::RunReport, String> {
    let mut repo = crate::ui::blocking_ops::open_backend(repo_path)
        .map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::ResetCurrentToHead {
        target: target.clone(),
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_ok() {
        klog!("executed: reset-current-to-head {}", target.short());
    }
    Ok(report)
}
