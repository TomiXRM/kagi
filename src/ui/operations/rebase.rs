//! Rebase-current-onto operation (branch-menu "Integrate" group).
//!
//! Single confirm (mirrors `start_merge`) — rebase is Guarded, not
//! Destructive: it only ever rewrites the *local* branch, and a mid-rebase
//! conflict routes into the existing conflict editor exactly the way a
//! conflicting merge does. `reload()` re-runs conflict-mode detection
//! unconditionally after execute, so `RebaseOutcome::Conflicted` needs no
//! special routing here — the same call that picks up a completed rebase
//! also picks up a paused one.

use crate::ui::*;

impl KagiApp {
    /// Open the rebase-current-onto modal, rebasing the checked-out branch
    /// onto `onto` (the right-clicked row's branch).
    pub fn open_rebase_modal(&mut self, onto: String, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let branch = self
            .active_view
            .branches
            .iter()
            .find(|(_, current)| *current)
            .map(|(name, _)| name.clone())
            .unwrap_or_default();
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from("rebase: repo session unavailable"));
                return;
            }
        };
        match repo.plan_rebase_current_onto(&onto) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: rebase '{}' onto '{}' blockers={}",
                    branch,
                    onto,
                    plan.blockers.len()
                );
                self.set_rebase_current_onto_modal(RebaseCurrentOntoModal {
                    onto,
                    branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("rebase plan error: {}", e)));
            }
        }
    }

    pub fn cancel_rebase_modal(&mut self) {
        self.clear_rebase_current_onto_modal();
    }

    /// Confirm + execute the rebase on a background thread (busy_op), then
    /// reload — a conflict pause and a clean completion both flow through
    /// the same `reload()` call (see module doc).
    pub fn start_rebase(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.rebase_current_onto_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: rebase plan has blockers, not executing");
            self.record_op(
                "rebase",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_rebase_current_onto_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("rebase");
        self.clear_rebase_current_onto_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(format!(
            "Rebasing '{}' onto '{}'…",
            modal.branch, modal.onto
        )));
        klog!("async: rebase started");

        let plan = modal.plan.clone();
        let onto = modal.onto.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_onto = onto.clone();
        let task =
            cx.background_spawn(async move { rebase_blocking(&bg_path, &bg_plan, &bg_onto) });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(summary) => {
                klog!("async: rebase finished — {}", summary);
                app.record_op(
                    "rebase",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success {
                        after: kagi_git::ops::StateSummary {
                            head: plan.current.head.clone(),
                            dirty: summary,
                        },
                    },
                    &repo_path,
                    cx,
                );
                app.status_footer =
                    FooterStatus::Success(SharedString::from(format!("rebase: onto '{}'", onto)));
                // Re-runs conflict-mode detection unconditionally — a
                // rebase paused at a conflict enters Conflict Mode here,
                // exactly like a conflicting merge (see module doc).
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: rebase failed — {}", err_msg);
                app.record_op(
                    "rebase",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                app.set_rebase_current_onto_modal(RebaseCurrentOntoModal {
                    onto: onto.clone(),
                    branch: modal.branch.clone(),
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        });
    }
}

/// Blocking `preflight → execute` for the background thread, mirroring
/// `blocking_ops.rs::merge_blocking`. `run()` enforces preflight (refuses if
/// HEAD moved since `plan` was captured) as its first step. Returns a short
/// human string describing the outcome for the oplog/footer — `Conflicted`
/// is not an `Err` (see module doc).
fn rebase_blocking(
    repo_path: &std::path::Path,
    plan: &kagi_git::ops::OperationPlan,
    onto: &str,
) -> Result<String, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    let op = kagi_git::Operation::RebaseCurrentOnto {
        onto: onto.to_string(),
    };
    let outcome = repo
        .run(&op, plan)
        .map_err(|e| format!("Rebase failed: {}", e))?;
    match outcome {
        kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Completed { head }) => {
            klog!(
                "executed: rebase onto {} — completed at {}",
                onto,
                head.short()
            );
            Ok(format!("completed at {}", head.short()))
        }
        kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Conflicted) => {
            klog!("executed: rebase onto {} — paused for conflicts", onto);
            Ok("paused for conflicts".to_string())
        }
        _ => Ok("done".to_string()),
    }
}
