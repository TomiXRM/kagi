//! Delete-remote-branch operation (branch-menu "Advanced / Dangerous" group).
//!
//! Two-stage confirm (mirrors `operations/discard.rs`'s `confirm_armed`
//! pattern): the first click only arms the button; the second click runs the
//! delete on a background thread and reloads.

use crate::ui::blocking_ops::*;
use crate::ui::*;

impl KagiApp {
    /// Open the delete-remote-branch modal for `remote_branch` (e.g.
    /// `"origin/feature/x"`).
    pub fn open_delete_remote_branch_modal(&mut self, remote_branch: impl Into<String>) {
        let remote_branch = remote_branch.into();
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "delete-remote-branch: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_delete_remote_branch(&remote_branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: delete-remote-branch {} blockers={}",
                    remote_branch,
                    plan.blockers.len()
                );
                self.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                    remote_branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "delete-remote-branch plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_delete_remote_branch_modal(&mut self) {
        self.clear_delete_remote_branch_modal();
    }

    /// Two-stage confirm (mirrors `start_discard`): the first click only
    /// arms the button; the second click runs the delete on a background
    /// thread (busy_op) and reloads.
    pub fn start_delete_remote_branch(&mut self, cx: &mut Context<Self>) {
        let modal = match self.delete_remote_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };

        if !modal.confirm_armed {
            self.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                confirm_armed: true,
                ..modal
            });
            klog!("delete-remote-branch: armed (second confirm required — destructive)");
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
                "[kagi] refused: delete-remote-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "delete-remote-branch",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_delete_remote_branch_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("delete-remote-branch");
        self.clear_delete_remote_branch_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Deleting remote branch '{}'…",
            modal.remote_branch
        )));
        klog!("async: delete-remote-branch started");

        let plan = modal.plan.clone();
        let remote_branch = modal.remote_branch.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_remote_branch = remote_branch.clone();
        let task = cx.background_spawn(async move {
            delete_remote_branch_blocking(&bg_path, &bg_plan, &bg_remote_branch)
        });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(after) => {
                klog!("async: delete-remote-branch finished");
                app.record_op(
                    "delete-remote-branch",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "delete-remote-branch: '{}' deleted",
                    remote_branch
                )));
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: delete-remote-branch failed — {}", err_msg);
                app.record_op(
                    "delete-remote-branch",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                app.set_delete_remote_branch_modal(DeleteRemoteBranchModal {
                    remote_branch: remote_branch.clone(),
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    confirm_armed: false,
                });
            }
        });
    }
}
