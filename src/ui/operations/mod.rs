//! Per-operation orchestration for `KagiApp`, split out of `ui/mod.rs`
//! (issue #13 Phase 4, P1). Each submodule holds the `open_/cancel_/replan_/
//! confirm_/start_` methods (plus async/finish helpers) for one family of Git
//! operations as additional `impl KagiApp` blocks. Pure physical split —
//! behaviour and signatures are unchanged.

mod app_bridge;
pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod commit;
pub mod conflict;
pub mod conflict_detect;
mod conflict_skip;
pub mod discard;
pub mod editor_fs;
pub mod force_lease;
pub mod history;
pub mod merge;
pub mod modal_state;
pub mod pull_push;
pub mod rebase;
pub mod remote_branch;
pub mod reset;
mod staging_failure;
pub mod stash;
pub mod tag;
pub(crate) mod transport_hold;
pub mod worktree;

use crate::ui::i18n::Msg;
use crate::ui::types::FooterStatus;

/// A failure handed to a family's `on_done` by [`KagiApp::finish_recorded`]:
/// the localized text plus the receipt's typed code (ADR-0195), so a family
/// can special-case a code without matching on prose.
pub(crate) struct OpFailure {
    pub message: String,
    pub code: FailureCode,
}

/// A Pull confirmation that could not be delivered when its fetch finished,
/// waiting for the tab that asked for it (#625, ADR-0192).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PullConfirmDelivery {
    /// Plan and open the confirmation.
    Confirm,
    /// The pre-Pull fetch failed; show the notice (the oplog entry was already
    /// written when the fetch completed).
    FetchFailed(String),
}
use crate::ui::KagiApp;
use gpui::{AppContext, Context, SharedString, Task};
use kagi_git::backend::recording::{Recording, RunReport};
use kagi_git::oplog::FailureCode;
use kagi_git::OperationOutcome;
use kagi_git::OperationPlan;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Whether a state-changing op may start right now. A held **lease** is the
/// truth about an in-flight write, `write_busy` its name mirror (and the sole
/// latch of the one lease-less writer left, remote pull over SSH), `planning`
/// the in-flight *plan* latch (ADR-0196 Wave 3). A mutation started while any
/// of the three holds is exactly the concurrent-mutation hazard #283 is about,
/// so every entry point that begins one consults this — through
/// [`KagiApp::op_latched`]. Pure so the gate is testable without a Context.
pub(crate) fn op_may_start(
    has_leases: bool,
    write_busy: Option<&'static str>,
    planning: Option<&'static str>,
) -> bool {
    !has_leases && write_busy.is_none() && planning.is_none()
}

impl KagiApp {
    /// Reject a state-changing op if another is in flight (#283 stage 1).
    /// Returns true (and sets the footer) when the caller must bail out.
    pub(crate) fn reject_if_busy(&mut self, cx: &mut Context<Self>) -> bool {
        self.refresh_write_busy();
        if !self.op_latched() {
            return false;
        }
        self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
        cx.notify();
        true
    }

    /// Complete a **planning** task: the last shape of background work that is
    /// not a write (ADR-0196 Wave 3). It holds no lease, so all it releases is
    /// its own `planning` tag — and only while that tag is still the one in
    /// place, since a later plan owns whatever it latched.
    ///
    /// Its result belongs to the tab that launched it, identified by the owner
    /// stamp frozen here — same session, same visit (決定 3) — not by the
    /// `repo_path + switch_generation` string comparison it replaces.
    pub(crate) fn finish_planning<R, F>(
        &mut self,
        cx: &mut Context<Self>,
        task: Task<R>,
        tag: &'static str,
        on_done: F,
    ) where
        R: 'static,
        F: FnOnce(&mut Self, R, &mut Context<Self>) + 'static,
    {
        let owner = self.active_session();
        let visit = owner.and_then(|session| self.app_sessions.visit(session));
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            let _ = this.update(acx, move |app, cx| {
                if app.planning == Some(tag) {
                    app.planning = None;
                }
                let current = app.active_session() == owner
                    && owner.and_then(|session| app.app_sessions.visit(session)) == visit;
                match result {
                    Some(result) if current => on_done(app, result, cx),
                    Some(_) => klog!("op result dropped: tab switched during op"),
                    // #289: gpui does not propagate a background panic, so the
                    // task can end without a result. The latch is released
                    // above whatever happened, so nothing wedges.
                    None => {
                        klog!("op panicked: {} — busy_op cleared", tag);
                        // ponytail: raw string, not a new i18n Msg — this is an
                        // edge-case recovery footer, matching the raw-format!
                        // footers already used across the op modules.
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "{tag}: operation failed unexpectedly"
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// ADR-0196 Wave 3: one legacy run-pipeline write, admitted through the
    /// application layer. Replaces the legacy latch, `background_spawn`
    /// and [`finish_recorded`](Self::finish_recorded) trio: admission
    /// (`approve_run`, then `begin_write`) reserves the lease and freezes the
    /// owner stamp; the family's blocking core runs as the job; `apply`
    /// settles (lease, reconcile, invalidation) and the completion is
    /// presented to the tab the stamp names — same tab, same visit — or
    /// dropped exactly as the legacy `switch_generation` guard dropped it.
    /// The `Invalidate` delivery reloads the owner, so `on_done` no longer
    /// calls `reload`. `finished_note` is the tail of the `async: <op> …`
    /// contract line for a successful result (`None` = `finished`; a family
    /// with a historical ` — <summary>` suffix or a "partially applied" verb
    /// returns the whole tail). Returns `false` when admission refused; the
    /// refusal is already presented.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn finish_run<X, N, F>(
        &mut self,
        cx: &mut Context<Self>,
        op_name: &'static str,
        op: crate::ui::i18n::Op,
        plan: Arc<OperationPlan>,
        repo_path: PathBuf,
        execute: X,
        finished_note: N,
        on_done: F,
    ) -> bool
    where
        X: FnOnce() -> Result<RunReport, String> + Send + 'static,
        N: FnOnce(&OperationOutcome) -> Option<String> + 'static,
        F: for<'a> FnOnce(&mut Self, Result<&'a OperationOutcome, OpFailure>, &mut Context<Self>)
            + 'static,
    {
        use crate::app::{self, Delivery, FamilyEvidence};
        self.refresh_write_busy();
        // The lease answers for every other writer; the UI latch is what a
        // planning task in flight is refused by (ADR-0196 Wave 3).
        let latched = self.op_latched();
        let owner = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id));
        let admitted = owner
            .ok_or(app::AdmissionError::StaleApproval)
            .and_then(|owner| {
                let backend = kagi_git::Backend::open(&repo_path)
                    .map_err(|error| app::AdmissionError::Identity(error.to_string()))?;
                let repo = backend
                    .write_repo_id()
                    .map_err(|error| app::AdmissionError::Identity(error.to_string()))?;
                // Frozen here, before the write: what the remote should say
                // afterwards. A reconcile compares against this, never against
                // whatever the repository holds later (#702 re-review).
                let remote = backend.remote_expectation(op_name, &plan);
                app::approve_run(
                    &mut self.app_sessions,
                    app::RunRequest {
                        owner,
                        name: op_name,
                        path: repo_path.clone(),
                        repo,
                        plan: plan.clone(),
                        remote,
                    },
                )
            })
            .and_then(|approved| {
                if latched {
                    return Err(app::AdmissionError::Busy);
                }
                app::prepare_run(&mut self.app_sessions, approved, Box::new(execute))
            });
        let job = match app::admit(&mut self.reads, admitted) {
            Ok(job) => job,
            Err(error) => {
                self.report_admission_refusal(error, cx);
                return false;
            }
        };
        self.mark_write_busy(op_name);
        let stamp = job.stamp();
        // #289: gpui does not propagate a background panic, so the task can end
        // without a completion. That is not evidence of termination — the write
        // may have happened — so it settles as `Unknown` through the same
        // `apply`, which keeps the operation id, retains the lease and parks
        // the reconcile entry. Clearing the busy mirror here instead would
        // leave `has_leases()` true with `op_latched()` false.
        let abandonment = job.abandonment();
        let task = cx.background_spawn(async move { job.run() });
        cx.spawn(async move |this, acx| {
            let completion = task.fallible().await;
            let _ = this.update(acx, move |app, cx| {
                let completion = match completion {
                    Some(completion) => completion,
                    None => {
                        klog!("op panicked: {} — busy_op cleared", op_name);
                        abandonment.into_completion()
                    }
                };
                let deliveries = app::apply(&mut app.app_sessions, completion);
                app.refresh_write_busy();
                let (completed, rest): (Vec<_>, Vec<_>) = deliveries
                    .into_iter()
                    .partition(|d| matches!(d, Delivery::Completed { .. }));
                let mut failed = false;
                for delivery in completed {
                    let Delivery::Completed { id, report, .. } = delivery else {
                        continue;
                    };
                    let FamilyEvidence::Run(report) = report.evidence else {
                        continue;
                    };
                    // Settle first, whatever the tab is doing now (#501). A
                    // parked reconcile requirement refuses every later write in
                    // this scope, so the way into it is settlement too — it must
                    // survive the tab guard below (#702 re-review). So must the
                    // `Partial` transport hold `settle_run_receipt` registers.
                    app.settle_run_receipt(op_name, &report, &repo_path);
                    app.notice_reconcile_required(id, op_name, &repo_path);
                    let current = app.active_session() == Some(stamp.session)
                        && app.app_sessions.visit(stamp.session) == Some(stamp.visit);
                    if !current {
                        klog!("op result dropped: tab switched during op");
                        continue;
                    }
                    failed = report.result.is_err();
                    match &report.result {
                        Ok(outcome) => {
                            klog!(
                                "async: {} {}",
                                op_name,
                                finished_note(outcome).unwrap_or_else(|| "finished".to_string())
                            );
                            app.present_recorded(&report.recording, cx);
                            let presented = app.status_footer.clone();
                            on_done(app, Ok(outcome), cx);
                            // A mutation that happened but was not recorded is
                            // presented as "changed but not recorded" (#501); a
                            // family's own success footer must not paper over it.
                            if matches!(report.recording, Recording::Failed { .. }) {
                                app.status_footer = presented;
                            }
                        }
                        Err(error) => {
                            let failure = OpFailure {
                                message: crate::ui::i18n::op_failed(op, error),
                                code: FailureCode::from(error),
                            };
                            klog!("async: {} failed — {}", op_name, failure.message);
                            app.present_recorded(&report.recording, cx);
                            on_done(app, Err(failure), cx);
                        }
                    }
                    // One completion per admitted write.
                    break;
                }
                for delivery in rest {
                    match delivery {
                        // A failed write reopened its modal with the error; the
                        // reload sweep would clear it (`reload.rs`), and the
                        // legacy path never reloaded a failure. Mark the reads
                        // stale, let the next reload pick them up.
                        Delivery::Invalidate(target) if failed => {
                            for session in app.app_sessions.sessions_for(&target.worktree) {
                                app.reads.invalidate(session);
                            }
                        }
                        other => app.deliver_app_result(other, cx),
                    }
                }
                app.present_app_notice();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        true
    }

    /// Synchronous twin of [`KagiApp::finish_recorded`] for the inline
    /// main-thread sites (create-branch, create-tag, the auto-stash before a
    /// checkout, the continued-merge commit): settle — notice a failed append
    /// — and present the receipt. Callers keep their own contract lines and
    /// modal handling around it; the receipt is the record either way.
    pub(crate) fn present_report(
        &mut self,
        op_name: &str,
        report: &RunReport,
        repo_path: &Path,
        cx: &mut Context<Self>,
    ) {
        self.notice_recording_failure(op_name, &report.recording, repo_path);
        self.present_recorded(&report.recording, cx);
    }

    /// Owner-named notice for a record the execution boundary could not append
    /// (#501). Call it from the settle half so a tab switch cannot swallow
    /// "changed but not recorded" — the same delivery the stash family uses.
    pub(crate) fn notice_recording_failure(
        &mut self,
        op: &str,
        recording: &Recording,
        repo: &Path,
    ) {
        if let Recording::Failed { error, .. } = recording {
            self.app_notices
                .push_back(format!("{}: {op}: recording failed: {error}", repo.display()).into());
        }
    }

    /// Present an outcome the execution boundary already recorded. When the
    /// append failed there is no durable trace at all, so a mutation that DID
    /// happen is presented as "changed but not recorded" — never a clean
    /// success toast (#501). A failure that changed nothing stands as it is.
    pub(crate) fn present_recorded(&mut self, recording: &Recording, cx: &mut Context<Self>) {
        // Presentation consumes the attempted receipt, never reconstructs its
        // structured recovery/owner metadata and never retries persistence.
        let entry = crate::ui::oplog_panel::OpLogPanel::entry_for_recording(recording);
        self.record_op_impl(entry, cx, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_may_start_only_when_no_op_is_in_flight() {
        // #283: the in-flight-op latch is the concurrent-mutation gate, and
        // ADR-0196 Wave 3 makes the *lease* its first term — a write that
        // reserved one blocks the next op with no busy mirror in the picture.
        assert!(op_may_start(false, None, None), "idle must allow a new op");
        assert!(
            !op_may_start(true, None, None),
            "a held lease alone must block a new op"
        );
        assert!(
            !op_may_start(false, None, Some("merge-plan")),
            "a plan in flight alone must block a new op"
        );
        // The mirror is the only latch remote pull (lease-less) has.
        assert!(
            !op_may_start(false, Some("pull"), None),
            "a lease-less writer's mirror alone must block a new op"
        );
    }

    #[test]
    fn pr_merge_is_blocked_while_another_op_is_in_flight() {
        // #402: a PR merge is a write op — with any op latched (its own lease
        // or a plan) `reject_if_busy` must refuse it, same as every other
        // start_*. Guards the regression where start_pr_merge never read the
        // latch at all.
        assert!(
            !op_may_start(true, Some("pr-merge"), None),
            "a pr-merge in flight must block a new op"
        );
        assert!(
            !op_may_start(true, Some("checkout"), None),
            "a local op in flight must block a pr-merge"
        );
    }
}
