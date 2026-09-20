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
mod commit_stage;
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
mod smart_generate;
mod staging_failure;
pub mod stash;
pub mod tag;
pub(crate) mod transport_hold;
pub mod worktree;

use crate::ui::i18n::Msg;
use crate::ui::types::FooterStatus;
use crate::ui::{BottomTab, ToastKind};

/// A failure handed to a family's `on_done` by [`KagiApp::finish_recorded`]:
/// the localized text plus the receipt's typed code (ADR-0195), so a family
/// can special-case a code without matching on prose.
pub(crate) struct OpFailure {
    pub message: String,
    pub code: FailureCode,
}

pub(crate) enum RunHistory {
    FromCurrentHead {
        kind: kagi_git::OperationKind,
        before: Option<(String, kagi_git::CommitId)>,
        summary: RunHistorySummary,
    },
    Exact {
        kind: kagi_git::OperationKind,
        branch: String,
        before: kagi_git::CommitId,
        after: kagi_git::CommitId,
        summary: String,
    },
}

pub(crate) enum RunHistorySummary {
    Fixed(String),
    Commit(String),
}

pub(crate) struct CommitPanelFailure {
    /// Weak by type (#722 P1). This travels inside a `RunPresentation` that
    /// the `finish_run` helper's detached task holds for the whole background
    /// write, so a strong handle kept a closed tab's `CommitPanelView` — and
    /// its title/body `InputState` — alive until a large Commit finished.
    /// Making the field weak makes the strong path fail to compile.
    pub(crate) expected: gpui::WeakEntity<crate::ui::commit_panel::CommitPanelView>,
    pub(crate) message: SharedString,
}

pub(crate) struct GithubMergePresentation {
    number: u64,
    detail: String,
}

#[derive(Default)]
pub(crate) struct RunPresentation {
    status: Option<FooterStatus>,
    history: Option<RunHistory>,
    consume_commit_message: Option<PathBuf>,
    refresh_worktree_wip: Option<PathBuf>,
    reload: bool,
    open_operation_log: bool,
    github_merge: Option<GithubMergePresentation>,
    /// A posted PR comment: clear the composer and re-read the thread.
    pr_comment: Option<u64>,
    issue_write: Option<(Option<u64>, u64)>,
    /// A confirmed field edit: the tab's own copy of the PR carries these
    /// values now, until the next list fetch confirms them from GitHub.
    pr_edit: Option<(u64, crate::ui::modals::PrField, Vec<String>)>,
    commit_panel_failure: Option<CommitPanelFailure>,
    outcome_notice: Option<String>,
}

impl RunPresentation {
    pub(crate) fn none() -> Self {
        Self::default()
    }

    pub(crate) fn status(status: FooterStatus) -> Self {
        Self {
            status: Some(status),
            ..Self::default()
        }
    }

    pub(crate) fn with_history(mut self, history: RunHistory) -> Self {
        self.history = Some(history);
        self
    }

    pub(crate) fn consume_commit_message(mut self, repo: PathBuf) -> Self {
        self.consume_commit_message = Some(repo);
        self
    }

    pub(crate) fn refresh_worktree_wip(mut self, repo: PathBuf) -> Self {
        self.refresh_worktree_wip = Some(repo);
        self
    }

    pub(crate) fn reload(mut self) -> Self {
        self.reload = true;
        self
    }

    pub(crate) fn open_operation_log(mut self) -> Self {
        self.open_operation_log = true;
        self
    }

    pub(crate) fn github_merge(mut self, number: u64, detail: String) -> Self {
        self.github_merge = Some(GithubMergePresentation { number, detail });
        self
    }

    pub(crate) fn pr_comment(mut self, number: u64) -> Self {
        self.pr_comment = Some(number);
        self
    }

    pub(crate) fn issue_write(mut self, number: Option<u64>, revision: u64) -> Self {
        self.issue_write = Some((number, revision));
        self
    }

    pub(crate) fn pr_edit(
        mut self,
        number: u64,
        field: crate::ui::modals::PrField,
        selected: Vec<String>,
    ) -> Self {
        self.pr_edit = Some((number, field, selected));
        self
    }
    pub(crate) fn update_commit_panel(mut self, failure: CommitPanelFailure) -> Self {
        self.commit_panel_failure = Some(failure);
        self
    }

    pub(crate) fn outcome_notice(mut self, message: String) -> Self {
        self.outcome_notice = Some(message);
        self
    }
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
/// truth about every write that can take one, `remote_write` is the latch of
/// the one that cannot (remote pull over SSH), `planning` the in-flight *plan*
/// latch (ADR-0196 Wave 3). A mutation started while any of the three holds is
/// exactly the concurrent-mutation hazard #283 is about, so every entry point
/// that begins one consults this — through [`KagiApp::op_latched`]. Pure so the
/// gate is testable without a Context.
pub(crate) fn op_may_start(
    has_leases: bool,
    remote_write: Option<&'static str>,
    planning: Option<&'static str>,
) -> bool {
    !has_leases && remote_write.is_none() && planning.is_none()
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

    /// The one gate every **owner-targeted pane mutation** passes through
    /// (ADR-0197 決定 3 / 決定 5).
    ///
    /// Round 1 gave each of these actions an explicit `owner` argument so a
    /// deferred click could not act on whatever tab is now on screen. That
    /// same argument answers the second question too — whether that owner's
    /// retained panes are authoritative yet — so both live here rather than
    /// at each entry point. Adding a guard per entry is what let staging get
    /// the revalidation check while the single-file Discard did not, exactly
    /// as Continue once had an owner guard that Abort and Skip lacked.
    ///
    /// Refused when the frozen owner is not the tab on screen, or when that
    /// owner's panes are still awaiting the activation read that re-anchors
    /// them. Pure display — scroll, divider drags, closing a pane — does not
    /// come through here.
    pub(crate) fn pane_mutation_admitted(&self, owner: crate::app::SessionId) -> bool {
        self.active_session() == Some(owner) && !self.ui().panes_revalidating()
    }

    /// Complete a **planning** task: the last shape of background work that is
    /// not a write (ADR-0196 Wave 3). It holds no lease, so all it releases is
    /// its own `planning` tag — and only while that tag is still the one in
    /// place, since a later plan owns whatever it latched.
    ///
    /// Its result belongs to the tab that launched it, identified by the owner
    /// stamp frozen here — same session, same visit (決定 3).
    pub(crate) fn finish_planning<R, F>(
        &mut self,
        cx: &mut Context<Self>,
        task: Task<R>,
        tag: &'static str,
        on_done: F,
    ) where
        R: 'static,
        F: FnOnce(R) -> modal_state::PlanningPresentation + 'static,
    {
        let owner = self.active_session();
        let visit = owner.and_then(|session| self.app_sessions.visit(session));
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            let _ = this.update(acx, move |app, cx| {
                if app.planning == Some(tag) {
                    app.planning = None;
                    app.status_footer = FooterStatus::Idle(SharedString::from(""));
                }
                let current = app.active_session() == owner
                    && owner.and_then(|session| app.app_sessions.visit(session)) == visit;
                match result {
                    Some(result) if current => match on_done(result) {
                        modal_state::PlanningPresentation::Offer(offer) => {
                            app.offer_plan_from_async(*offer);
                        }
                        modal_state::PlanningPresentation::Failed { operation, error } => {
                            app.report_plan_failure(operation, error);
                        }
                    },
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
    /// presented to the tab the stamp names — same tab, same visit — or dropped.
    /// The `Invalidate` delivery reloads the owner. `on_done` receives no
    /// `KagiApp`; it returns typed status/history/follow-up presentation only.
    /// `finished_note` is the tail of the `async: <op> …`
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
        F: for<'a> FnOnce(Result<&'a OperationOutcome, &'a OpFailure>) -> RunPresentation + 'static,
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
                let mut on_done = Some(on_done);
                let mut finished_note = Some(finished_note);
                for delivery in completed {
                    let Delivery::Completed { id, report, .. } = delivery else {
                        continue;
                    };
                    let FamilyEvidence::Run(report) = report.evidence else {
                        continue;
                    };
                    let on_done = on_done.take().expect("one completion per admitted write");
                    let finished_note = finished_note
                        .take()
                        .expect("one completion per admitted write");
                    // Settle first, whatever the tab is doing now (#501). A
                    // parked reconcile requirement refuses every later write in
                    // this scope, so the way into it is settlement too — it must
                    // survive the tab guard below (#702 re-review). So must the
                    // `Partial` transport hold `settle_run_receipt` registers.
                    app.settle_run_receipt(op_name, &report, &repo_path);
                    app.notice_reconcile_required(id, op_name, &repo_path);
                    let current = app.active_session() == Some(stamp.session)
                        && app.app_sessions.visit(stamp.session) == Some(stamp.visit);
                    let (mut presentation, failure_message, finished) = match &report.result {
                        Ok(outcome) => (
                            on_done(Ok(outcome)),
                            None,
                            Some(finished_note(outcome).unwrap_or_else(|| "finished".to_string())),
                        ),
                        Err(error) => {
                            let failure = OpFailure {
                                message: crate::ui::i18n::op_failed(op, error),
                                code: FailureCode::from(error),
                            };
                            let message = failure.message.clone();
                            (on_done(Err(&failure)), Some(message), None)
                        }
                    };
                    app.record_run_history_for(
                        stamp.session,
                        &repo_path,
                        presentation.history.take(),
                    );
                    // A posted comment or review settles for the session that
                    // posted it, whatever is on screen now: the composer is
                    // emptied and the thread re-read. Leaving this to the
                    // presentation below meant a post that landed while the
                    // reader was on another tab left its text in the box, and
                    // coming back and pressing the button sent it twice
                    // (review finding).
                    if let Some((number, field, selected)) = presentation.pr_edit.take() {
                        // The write succeeded, so the owner's copy of the PR
                        // carries the new values; the list ticker is what
                        // confirms them from GitHub afterwards.
                        app.apply_pr_fields(
                            Some(stamp.session),
                            repo_path.clone(),
                            number,
                            field,
                            selected,
                            cx,
                        );
                    }
                    if let Some(number) = presentation.pr_comment.take() {
                        // `repo_path` is the one the write was planned against,
                        // frozen at dispatch - not `app.repo_path`, which is
                        // whatever is on screen now (review finding).
                        app.settle_pr_write(Some(stamp.session), repo_path.clone(), number, cx);
                    }
                    if let Some((number, revision)) = presentation.issue_write.take() {
                        app.settle_issue_write(
                            stamp.session,
                            repo_path.clone(),
                            number,
                            revision,
                            cx,
                        );
                    }
                    if !current {
                        klog!("op result dropped: tab switched during op");
                        continue;
                    }
                    failed = report.result.is_err();
                    let recorded_status = if let Some(finished) = finished {
                        klog!("async: {} {}", op_name, finished);
                        app.present_recorded(&report.recording, cx);
                        Some(app.status_footer.clone())
                    } else {
                        klog!(
                            "async: {} failed — {}",
                            op_name,
                            failure_message.as_deref().unwrap_or_default()
                        );
                        app.present_recorded(&report.recording, cx);
                        None
                    };
                    let notice_override = presentation.outcome_notice.clone();
                    app.apply_run_presentation(presentation, cx);
                    app.enqueue_run_outcome_notice(
                        id,
                        &report.recording,
                        failure_message.as_deref(),
                        notice_override,
                    );
                    if matches!(report.recording, Recording::Failed { .. }) {
                        if let Some(recorded_status) = recorded_status {
                            app.status_footer = recorded_status;
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

    fn record_run_history_for(
        &mut self,
        owner: crate::app::SessionId,
        repo_path: &std::path::Path,
        history: Option<RunHistory>,
    ) {
        let Some(history) = history else {
            return;
        };
        match history {
            RunHistory::FromCurrentHead {
                kind,
                before,
                summary,
            } => {
                let after = kagi_git::Backend::open(repo_path).ok().and_then(|backend| {
                    Some((backend.head_shorthand()?, backend.head_commit_id()?))
                });
                if let (Some((branch, before)), Some((_, after))) = (before, after) {
                    let summary = match summary {
                        RunHistorySummary::Fixed(summary) => summary,
                        RunHistorySummary::Commit(subject) => {
                            format!("commit {} '{}'", after.short(), subject)
                        }
                    };
                    self.record_history_for(owner, kind, &branch, before, after, summary);
                }
            }
            RunHistory::Exact {
                kind,
                branch,
                before,
                after,
                summary,
            } => self.record_history_for(owner, kind, &branch, before, after, summary),
        }
    }

    fn apply_run_presentation(&mut self, presentation: RunPresentation, cx: &mut Context<Self>) {
        if let Some(repo) = presentation.consume_commit_message {
            self.consume_commit_panel_message(&repo, cx);
        }
        if let Some(failure) = presentation.commit_panel_failure {
            // Re-resolve through the *current* pane: same identity means the
            // owner is still on screen with the panel this failure belongs to.
            // A closed tab dropped it, so there is nothing to write to.
            let expected = failure.expected.entity_id();
            if let Some(panel) = self
                .ui()
                .commit_panel
                .clone()
                .filter(|panel| panel.entity_id() == expected)
            {
                panel.update(cx, |panel, _| {
                    if let Some(modal) = &mut panel.state.plan_modal {
                        modal.error = Some(failure.message);
                    }
                });
            }
        }
        if let Some(repo) = presentation.refresh_worktree_wip {
            self.refresh_worktree_wip_row(&repo);
        }
        if presentation.reload {
            self.reload(cx);
        }
        if presentation.open_operation_log {
            self.bottom_panel_open = true;
            self.bottom_tab = BottomTab::OperationLog;
            if let Some(panel) = self.op_log.clone() {
                panel.update(cx, |panel, cx| {
                    panel.toggle_expanded(0);
                    cx.notify();
                });
            }
        }
        if let Some(merged) = presentation.github_merge {
            self.push_toast(
                ToastKind::Info,
                SharedString::from(if merged.detail.is_empty() {
                    format!("{} #{}", Msg::PrModeMergeDone.t(), merged.number)
                } else {
                    merged.detail
                }),
                cx,
            );
            self.pr_mode_close_tab_for(merged.number, cx);
            self.refresh_github_prs(cx);
            self.fetch_async(true, cx);
        }
        if let Some(status) = presentation.status {
            self.status_footer = status;
        }
    }

    pub(crate) fn enqueue_run_outcome_notice(
        &mut self,
        id: crate::app::OperationId,
        recording: &Recording,
        failure_message: Option<&str>,
        override_message: Option<String>,
    ) {
        use kagi_git::oplog::OpOutcome;
        let entry = recording.entry();
        let (message, inspect) = match &entry.outcome {
            OpOutcome::Success { .. } | OpOutcome::Unknown { .. } => return,
            OpOutcome::Partial { .. } => (
                format!(
                    "{}: {}",
                    entry.op,
                    crate::ui::oplog_panel::outcome_summary(&entry.outcome)
                ),
                self.app_sessions.needs_reconcile(id).then_some(id),
            ),
            OpOutcome::Failed { .. } | OpOutcome::Refused { .. } => (
                override_message
                    .or_else(|| failure_message.map(str::to_owned))
                    .unwrap_or_else(|| {
                        format!(
                            "{}: {}",
                            entry.op,
                            crate::ui::oplog_panel::outcome_summary(&entry.outcome)
                        )
                    }),
                None,
            ),
        };
        let mut notice =
            crate::ui::modals::AppNotice::from(crate::ui::i18n::recorded_outcome_notice(message));
        notice.inspect = inspect;
        self.enqueue_outcome_notice(notice);
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
        // The remote latch is the only one remote pull (lease-less) has.
        assert!(
            !op_may_start(false, Some("pull"), None),
            "a lease-less writer's own latch alone must block a new op"
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
