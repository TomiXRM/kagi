//! Rebase-current-onto and replay-onto operations (branch-menu "Integrate"
//! group). One modal, one confirm path; the `Operation` in the modal decides
//! which backend op runs.
//!
//! Rebase: single confirm (mirrors `start_merge`) — rebase is Guarded, not
//! Destructive: it only ever rewrites the *local* branch, and a mid-rebase
//! conflict routes into the existing conflict editor exactly the way a
//! conflicting merge does. `reload()` re-runs conflict-mode detection
//! unconditionally after execute, so `RebaseOutcome::Conflicted` needs no
//! special routing here — the same call that picks up a completed rebase
//! also picks up a paused one.
//!
//! Replay (#344, ADR-0211): the plan is `destructive` (history rewrite,
//! ADR-0023), so the first confirm only arms the card and the second runs
//! it. Nothing is checked out: `git replay` prints the ref moves, the
//! backend applies them with `git update-ref --stdin`, and a conflict is a
//! plan blocker rather than a paused state.

use super::RunPresentation;
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
        let repo = match self.ui().repo_session.as_ref() {
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
                    op: kagi_git::Operation::RebaseCurrentOnto { onto: onto.clone() },
                    onto,
                    branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::Rebase, e),
                ));
            }
        }
    }

    /// Open the replay-onto modal (#344): move `branch` (the right-clicked
    /// row, checked out anywhere or nowhere) onto the current branch by ref
    /// update only. Same card and confirm path as rebase; the plan is
    /// destructive, so the card arms before it runs.
    pub fn open_replay_modal(&mut self, branch: String, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let Some(onto) = self
            .view()
            .branches
            .iter()
            .find(|(_, current)| *current)
            .map(|(name, _)| name.clone())
        else {
            self.status_footer =
                FooterStatus::Failed(SharedString::from("replay: no current branch"));
            return;
        };
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from("replay: repo session unavailable"));
                return;
            }
        };
        let op = kagi_git::Operation::ReplayOnto {
            branch: branch.clone(),
            onto: onto.clone(),
        };
        match repo.plan(&op) {
            Ok(plan) => {
                klog!(
                    "plan: replay-onto '{}' onto '{}' blockers={} warnings={}",
                    branch,
                    onto,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_rebase_current_onto_modal(RebaseCurrentOntoModal {
                    op,
                    onto,
                    branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::Replay, e),
                ));
            }
        }
    }

    pub fn cancel_rebase_modal(&mut self) {
        self.clear_rebase_current_onto_modal();
    }

    /// Confirm + execute the rebase on a background thread (write latch), then
    /// reload — a conflict pause and a clean completion both flow through
    /// the same `reload()` call (see module doc).
    pub fn start_rebase(&mut self, cx: &mut Context<Self>) {
        if self.op_latched() {
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
        let op_name = modal.op.oplog_name();
        if !modal.plan.blockers.is_empty() {
            klog!("refused: {} plan has blockers, not executing", op_name);
            self.record_refused(
                op_name,
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_rebase_current_onto_modal();
            cx.notify();
            return;
        }
        // ADR-0023: a history rewrite needs a second, explicit confirm.
        if modal.plan.destructive && !modal.confirm_armed {
            self.set_rebase_current_onto_modal(RebaseCurrentOntoModal {
                confirm_armed: true,
                ..modal
            });
            klog!("{}: armed (second confirm required — destructive)", op_name);
            cx.notify();
            return;
        }

        self.clear_rebase_current_onto_modal();
        let (busy, i18n_op) = match &modal.op {
            kagi_git::Operation::ReplayOnto { .. } => (
                format!("Replaying '{}' onto '{}'…", modal.branch, modal.onto),
                i18n::Op::Replay,
            ),
            _ => (
                format!("Rebasing '{}' onto '{}'…", modal.branch, modal.onto),
                i18n::Op::Rebase,
            ),
        };
        self.status_footer = FooterStatus::Busy(SharedString::from(busy));
        klog!("async: {} started", op_name);

        let plan = modal.plan.clone();
        let onto = modal.onto.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_op = modal.op.clone();
        self.finish_run(
            cx,
            op_name,
            i18n_op,
            plan.clone(),
            repo_path,
            move || rebase_blocking(&bg_path, &bg_plan, &bg_op),
            |outcome| Some(format!("finished — {}", rebase_summary(outcome))),
            move |done| match done {
                Ok(_) => RunPresentation::status(FooterStatus::Success(SharedString::from(
                    format!("{}: onto '{}'", op_name, onto),
                ))),
                Err(failure)
                    if failure.code
                        == kagi_git::oplog::FailureCode::RebaseBlockedByRepoSettings =>
                {
                    RunPresentation::none().outcome_notice(
                        i18n::rebase_repository_settings_may_block_start().to_string(),
                    )
                }
                Err(_) => RunPresentation::none(),
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
    op: &kagi_git::Operation,
) -> Result<kagi_git::backend::recording::RunReport, String> {
    let mut repo = crate::ui::blocking_ops::open_backend(repo_path)
        .map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let onto = match op {
        kagi_git::Operation::RebaseCurrentOnto { onto }
        | kagi_git::Operation::ReplayOnto { onto, .. } => onto.as_str(),
        _ => "",
    };
    let report = repo.run_recorded(op, plan);
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
        Ok(kagi_git::OperationOutcome::ReplayOnto {
            branch, from, to, ..
        }) => {
            klog!(
                "executed: replay-onto {} onto {} — {} -> {}",
                branch,
                onto,
                &from[..7.min(from.len())],
                &to[..7.min(to.len())]
            );
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
        kagi_git::OperationOutcome::ReplayOnto { backups, to, .. } => {
            format!(
                "{} ref(s) updated, now at {}",
                backups.len(),
                &to[..7.min(to.len())]
            )
        }
        _ => "done".to_string(),
    }
}
