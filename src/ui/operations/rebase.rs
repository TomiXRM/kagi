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
            .view()
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::Rebase, e),
                ));
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
        self.finish_recorded(
            cx,
            task,
            "rebase",
            i18n::Op::Rebase,
            plan.current.clone(),
            repo_path,
            |outcome| Some(format!(" — {}", rebase_summary(outcome))),
            move |app, done, cx| match done {
                Ok(_) => {
                    app.status_footer = FooterStatus::Success(SharedString::from(format!(
                        "rebase: onto '{}'",
                        onto
                    )));
                    // Re-runs conflict-mode detection unconditionally — a
                    // rebase paused at a conflict enters Conflict Mode here,
                    // exactly like a conflicting merge (see module doc).
                    app.reload(cx);
                }
                Err(failure) => {
                    // The typed code, not the prose (ADR-0195): a rebase that
                    // could not start because Kagi disabled repository settings
                    // gets its own guidance.
                    let error = if failure.code
                        == kagi_git::oplog::FailureCode::RebaseBlockedByRepoSettings
                    {
                        i18n::rebase_repository_settings_may_block_start().to_string()
                    } else {
                        failure.message
                    };
                    app.set_rebase_current_onto_modal(RebaseCurrentOntoModal {
                        onto: onto.clone(),
                        branch: modal.branch.clone(),
                        plan: plan.clone(),
                        error: Some(SharedString::from(error)),
                    });
                }
            },
        );
    }
}

/// Blocking `preflight → execute` for the background thread, mirroring
/// `blocking_ops.rs::merge_blocking`. Returns the backend's receipt — a
/// `Conflicted` rebase is a successful outcome, not an error (see module doc).
fn rebase_blocking(
    repo_path: &std::path::Path,
    plan: &kagi_git::ops::OperationPlan,
    onto: &str,
) -> Result<kagi_git::backend::recording::RunReport, String> {
    let mut repo = crate::ui::blocking_ops::open_backend(repo_path)
        .map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::RebaseCurrentOnto {
        onto: onto.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    match &report.result {
        Ok(kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Completed {
            head,
        })) => {
            klog!(
                "executed: rebase onto {} — completed at {}",
                onto,
                head.short()
            );
        }
        Ok(kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Conflicted)) => {
            klog!("executed: rebase onto {} — paused for conflicts", onto);
        }
        _ => {}
    }
    Ok(report)
}

/// The ` — <outcome>` suffix of the `async: rebase finished` contract line.
fn rebase_summary(outcome: &kagi_git::OperationOutcome) -> String {
    match outcome {
        kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Completed { head }) => {
            format!("completed at {}", head.short())
        }
        kagi_git::OperationOutcome::Rebase(kagi_git::ops::RebaseOutcome::Conflicted) => {
            "paused for conflicts".to_string()
        }
        _ => "done".to_string(),
    }
}
