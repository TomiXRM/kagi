//! Force-with-lease push operation (branch-menu "Advanced / Dangerous" group).
//!
//! Two-stage confirm (mirrors `operations/reset.rs`'s
//! reset-current-to-head `confirm_armed` pattern): the first click only arms
//! the button; the second click runs the push on a background thread and
//! reloads.

use crate::ui::*;

impl KagiApp {
    /// Open the force-with-lease push modal for the current branch.
    pub fn open_force_lease_push_modal(&mut self, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "force-with-lease-push: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_force_with_lease_push() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: force-with-lease-push blockers={}",
                    plan.blockers.len()
                );
                self.set_force_lease_push_modal(ForceLeasePushModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "force-with-lease-push plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_force_lease_push_modal(&mut self) {
        self.clear_force_lease_push_modal();
    }

    /// Two-stage confirm (mirrors `start_reset_current`): the first click
    /// only arms the button; the second click runs the push on a background
    /// thread (busy_op) and reloads.
    pub fn start_force_lease_push(&mut self, cx: &mut Context<Self>) {
        let modal = match self.force_lease_push_modal().cloned() {
            Some(m) => m,
            None => return,
        };

        if !modal.confirm_armed {
            self.set_force_lease_push_modal(ForceLeasePushModal {
                confirm_armed: true,
                ..modal
            });
            klog!("force-with-lease-push: armed (second confirm required — destructive)");
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
                "[kagi] refused: force-with-lease-push plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "force-with-lease-push",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_force_lease_push_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("force-with-lease-push");
        self.clear_force_lease_push_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from("Force-with-lease pushing…"));
        klog!("async: force-with-lease-push started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task =
            cx.background_spawn(async move { force_lease_push_blocking(&bg_path, &bg_plan) });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(after) => {
                klog!("async: force-with-lease-push finished");
                app.record_op(
                    "force-with-lease-push",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.status_footer =
                    FooterStatus::Success(SharedString::from("force-with-lease-push: done"));
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: force-with-lease-push failed — {}", err_msg);
                app.record_op(
                    "force-with-lease-push",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                app.set_force_lease_push_modal(ForceLeasePushModal {
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    confirm_armed: false,
                });
            }
        });
    }
}

/// Blocking `preflight → execute` for the background thread, mirroring
/// `operations/reset.rs::reset_current_blocking`. `run()` enforces preflight
/// (refuses if HEAD moved since `plan` was captured) as its first step.
fn force_lease_push_blocking(
    repo_path: &std::path::Path,
    plan: &kagi_git::ops::OperationPlan,
) -> Result<kagi_git::ops::StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    let op = kagi_git::Operation::ForceWithLeasePush;
    repo.run(&op, plan)
        .map_err(|e| format!("Push failed: {}", e))?;
    klog!("executed: force-with-lease-push");

    Ok(kagi_git::ops::StateSummary {
        head: plan.current.head.clone(),
        dirty: "force-with-lease pushed".to_string(),
    })
}
