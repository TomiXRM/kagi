//! Pull / push operations (open/confirm/start/finish).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;
use kagi_domain::plan_note::{
    CommonNote, DirtyParts, OpPhrase, PlanNote, PlanRecovery, PullNote, PullRecovery, RecoveryKind,
};

mod settle;

use super::modal_state::{AsyncPlanOffer, AsyncPlanToken};
use super::RunPresentation;
use crate::app::{self, PlanState, Planned};
use crate::ui::*;

impl KagiApp {
    /// Fetch the local upstream before offering a pull confirmation.
    pub fn open_pull_modal(&mut self, cx: &mut Context<Self>) {
        // A remote identity probe owns the planning latch; local fetch waiters
        // must not start another probe while it is held.
        if self.remote_view.is_some() && self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // Attaching a waiter is not admitting another plan or write.
        if self.remote_view.is_none()
            && self.fetch_in_flight.is_some()
            && self.fetch_async_for(false, Some(crate::ui::commands::PullTarget::Current), cx)
        {
            return;
        }
        // W3-NOTIFY: refuse while a background op runs.
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // The preview remains a synthesis of cached status; resolving the
        // remote write scope happens in the background before confirmation.
        if let Some(rv) = self.remote_view.clone() {
            let s = &self.view().status_summary;
            let branch = s.branch.clone();
            let behind = s.behind.unwrap_or(0);
            let ahead = s.ahead.unwrap_or(0);
            let upstream = self
                .view()
                .branch_upstream_info
                .get(&branch)
                .map(|u| u.remote_branch.clone())
                .unwrap_or_else(|| "upstream".to_string());
            let plan = kagi_git::plan_pull_remote(
                &branch,
                &upstream,
                behind,
                ahead,
                s.is_dirty,
                self.view().header.to_string(),
            );
            klog!("plan: remote pull branch={branch} behind={behind} ahead={ahead}");
            let Some(session) = self.active_session() else {
                return;
            };
            let visit = self.app_sessions.visit(session);
            let owner = crate::remote::stash::RemoteAttachment {
                session,
                host: rv.host,
                root: rv.root,
            };
            let cached_head_oid = self.view().head_oid.clone();
            let cached_remote_dirty = s.is_dirty;
            let job = app::plan_remote_pull(
                &mut self.app_sessions,
                app::RemotePullRequest {
                    owner: owner.clone(),
                    plan: std::sync::Arc::new(plan),
                    cached_head_oid,
                    cached_remote_dirty,
                },
            );
            let progress_request = self.mark_planning_busy("pull");
            let task = cx.background_spawn(async move { job.run() });
            cx.spawn(async move |this, cx| {
                let completion = task.fallible().await;
                let _ = this.update(cx, |app, cx| {
                    let owns_latch = app.planning == Some("pull");
                    if owns_latch {
                        app.planning = None;
                    }
                    let Some(completion) = completion else {
                        if owns_latch {
                            app.discard_contended_plan_from_async(
                                i18n::Op::Pull,
                                AsyncPlanToken::Session,
                            );
                        }
                        cx.notify();
                        return;
                    };
                    let current = app.remote_view.as_ref().is_some_and(|view| {
                        view.host == owner.host
                            && view.root == owner.root
                            && app.active_session() == Some(owner.session)
                            && app.app_sessions.visit(session) == visit
                    });
                    let current_completion = completion.is_current(&app.app_sessions);
                    if current && !app.has_active_modal() {
                        if app::apply_plan(&mut app.app_sessions, completion) {
                            match app.app_sessions.plan_state() {
                                PlanState::Ready {
                                    prepared: Planned::RemotePull { plan, .. },
                                    ..
                                } => {
                                    let preview = plan.preview.clone();
                                    let modal = PullPlanModal {
                                        plan: preview,
                                        auto_stash: false,
                                        error: None,
                                        dirty_digest: None,
                                        fetch_owner: None,
                                        progress_request: Some(progress_request),
                                    };
                                    // SSH planning freezes the live identity,
                                    // but does not predict fetched merge or
                                    // checkout safety. Keep its explicit choice.
                                    app.offer_plan_from_async(
                                        AsyncPlanOffer::new(
                                            i18n::Op::Pull,
                                            ActiveModal::Pull(modal),
                                        )
                                        .with_session_token(),
                                    );
                                }
                                PlanState::Error { error, blocker, .. } => {
                                    let reason = blocker.as_ref().map(i18n::plan_note_text);
                                    let message =
                                        SharedString::from(reason.unwrap_or_else(|| error.clone()));
                                    app.status_footer = FooterStatus::Failed(
                                        crate::ui::operations::display_text(message.clone()),
                                    );
                                    app.push_toast(ToastKind::Error, message, cx);
                                }
                                _ => {}
                            }
                        }
                    } else if current_completion {
                        app.discard_contended_plan_from_async(
                            i18n::Op::Pull,
                            AsyncPlanToken::Session,
                        );
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(_) = self.active_session() else {
            return;
        };
        // Verify cached structural blockers locally without doing an extra full
        // merge prediction on every healthy Pull. A stale cached blocker never
        // authorizes an executable plan or an up-to-date claim.
        let status = &self.view().status_summary;
        if status.is_detached || status.is_unborn || status.no_upstream || status.conflict_count > 0
        {
            match self.build_pull_modal(None) {
                Ok(Some(modal)) if !modal.plan.blockers.is_empty() => {
                    klog!(
                        "plan: pull blockers={} warnings={}",
                        modal.plan.blockers.len(),
                        modal.plan.warnings.len()
                    );
                    self.set_pull_modal(modal);
                    return;
                }
                Err(error) => {
                    self.report_plan_failure(i18n::Op::Pull, error);
                    return;
                }
                _ => {}
            }
        }
        // Neither an executable preview nor an up-to-date claim may rely on
        // cached tracking refs, regardless of working-tree dirtiness. Admission
        // refusal is not freshness: never fall back to the stale local plan.
        if !self.fetch_async_for(false, Some(crate::ui::commands::PullTarget::Current), cx)
            && self.fetch_in_flight.is_some()
        {
            // A different in-flight scope cannot accept this waiter. Admission
            // refusals already own their specific footer and must retain it.
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
        }
    }

    /// Deliver a successful fetch's confirmation only to the visit that
    /// requested it. The fetch completion records failures independently;
    /// a waiter must neither create a second receipt nor revive an old visit.
    pub(crate) fn deliver_pull_confirm(
        &mut self,
        request: (crate::app::SessionId, u64, std::time::Instant),
        displaced: bool,
        target: crate::ui::commands::PullTarget,
        fetched_remote: &str,
        progress_hidden: bool,
        cx: &mut Context<Self>,
    ) {
        let (session, visit, request_started) = request;
        if self.active_session() == Some(session) && self.app_sessions.visit(session) == Some(visit)
        {
            if displaced || self.has_active_modal() || self.op_latched() {
                self.discard_contended_plan_from_async(i18n::Op::Pull, AsyncPlanToken::None);
            } else {
                if progress_hidden {
                    self.hidden_busy_job = Some(super::super::busy::BusyIdentity::pull(request));
                }
                match target {
                    crate::ui::commands::PullTarget::Current => {
                        self.plan_and_offer_pull_modal_from_async(
                            fetched_remote,
                            request_started,
                            cx,
                        );
                    }
                    crate::ui::commands::PullTarget::BranchFfOnly(branch) => {
                        self.offer_fetched_branch_pull(branch, fetched_remote, request_started, cx);
                    }
                }
            }
        }
    }

    fn plan_and_offer_pull_modal_from_async(
        &mut self,
        fetched_remote: &str,
        request_started: std::time::Instant,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.build_pull_modal(Some(fetched_remote)) {
            Ok(Some(mut modal)) => {
                klog!(
                    "plan: pull blockers={} warnings={}",
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                modal.fetch_owner = self.active_session().and_then(|session| {
                    self.app_sessions
                        .visit(session)
                        .map(|visit| (session, visit, request_started))
                });
                let needs_choice = modal.plan.sync_needs_choice();
                let offered = self.offer_plan_from_async(AsyncPlanOffer::new(
                    i18n::Op::Pull,
                    ActiveModal::Pull(modal),
                ));
                if offered && !needs_choice {
                    // The Pull request is already the user's intent. Keep
                    // the same approval/preflight path without painting a card.
                    self.start_pull(cx);
                }
                offered
            }
            Ok(None) => {
                self.present_pull_up_to_date(request_started, cx);
                false
            }
            Err(error) => {
                self.report_plan_failure(i18n::Op::Pull, error);
                false
            }
        }
    }

    pub(crate) fn present_pull_up_to_date(
        &mut self,
        request_started: std::time::Instant,
        cx: &mut Context<Self>,
    ) {
        let long_running = cx
            .background_executor()
            .now()
            .saturating_duration_since(request_started)
            >= std::time::Duration::from_secs(2);
        self.present_sync_noop("pull", Msg::AlreadyUpToDatePull, long_running, cx);
    }

    fn present_sync_noop(
        &mut self,
        op: &str,
        message: Msg,
        long_running: bool,
        cx: &mut Context<Self>,
    ) {
        let text = SharedString::from(message.t());
        if self.operation_result_toast_needed(long_running) {
            if let Some(stack) = self.toast_stack.clone() {
                let key = Self::operation_result_key(
                    self.repo_path.as_deref().unwrap_or_else(|| Path::new("")),
                    op,
                    "success",
                );
                stack.update(cx, |stack, cx| {
                    stack.push_keyed_notify(
                        crate::ui::toast_stack::ToastSubject::Repository,
                        ToastKind::Sync,
                        text,
                        Some(key),
                        cx,
                    );
                });
            }
            self.status_footer = FooterStatus::Idle("".into());
        } else {
            self.status_footer = FooterStatus::Idle(text);
        }
    }

    /// Refresh a retained local Pull confirmation against reloaded repository
    /// state (#625, ADR-0192). Successful-fetch confirmations can outlive several
    /// unchanged watcher reads from that fetch; each read re-plans the contents
    /// rather than retaining a stale executable plan.
    ///
    /// Legacy dirty confirmations keep their error/recovery surface when
    /// replanning fails. A clean fetch-origin confirmation is invalidated
    /// instead: it must not retain an executable plan that cannot be refreshed.
    pub(crate) fn replan_pull_modal(&mut self) {
        match self.build_pull_modal(None) {
            Ok(Some(mut modal)) => {
                // The accepted snapshot may predate a live checkout change.
                // Do not rewrite a clean approval to that different checkout.
                if self.pull_modal().is_some_and(|current| {
                    current.fetch_owner.is_some()
                        && (current.plan.pull_identity != modal.plan.pull_identity
                            || (!current.auto_stash
                                && (modal.auto_stash
                                    || current.plan.head_at_plan != modal.plan.head_at_plan
                                    || current.dirty_digest.as_ref()
                                        != modal.dirty_digest.as_ref())))
                }) {
                    self.clear_pull_modal();
                    return;
                }
                klog!(
                    "replan: pull blockers={} warnings={}",
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                modal.fetch_owner = self.pull_modal().and_then(|current| current.fetch_owner);
                modal.progress_request = self
                    .pull_modal()
                    .and_then(|current| current.progress_request);
                self.update_pull_plan_from_async(modal);
            }
            Ok(None) => {
                if self
                    .pull_modal()
                    .is_some_and(|modal| !modal.auto_stash && modal.fetch_owner.is_some())
                {
                    self.clear_pull_modal();
                } else {
                    klog!("replan: pull nothing to pull; keeping the confirmation");
                }
            }
            Err(error) => {
                if self
                    .pull_modal()
                    .is_some_and(|modal| !modal.auto_stash && modal.fetch_owner.is_some())
                {
                    self.clear_pull_modal();
                }
                klog!("replan: pull failed: {error}");
            }
        }
    }

    /// The confirmation a local pull would show: `Ok(None)` when there is
    /// nothing to pull, `Err` when the plan could not be built.
    fn build_pull_modal(
        &mut self,
        expected_fetched_remote: Option<&str>,
    ) -> Result<Option<PullPlanModal>, String> {
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => return Err("pull: repo session unavailable".to_string()),
        };
        let mut plan = repo
            .plan_pull()
            .map_err(|e| i18n::op::op_plan_failed_contract(i18n::Op::Pull, e))?;
        // A blocked plan has no executable identity: preserve its actual blocker
        // instead of turning it into an upstream-change error after a successful fetch.
        if plan.blockers.is_empty()
            && expected_fetched_remote.is_some_and(|expected| {
                plan.pull_identity
                    .as_ref()
                    .map(|identity| identity.remote.as_str())
                    != Some(expected)
            })
        {
            return Err(Msg::PullUpstreamChangedDuringFetch.t().to_string());
        }
        // Dirtiness alone is not a reason to stash. Only the backend's
        // prediction of a local overwrite offers that explicit protection.
        // History conflicts take the merge/conflict workflow instead: a stash
        // cannot reconcile two committed histories.
        let history_conflict = plan
            .warnings
            .iter()
            .any(|note| matches!(note, PlanNote::Pull(PullNote::MergePrediction)));
        let auto_stash = plan.blockers.is_empty()
            && !history_conflict
            && plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::LocalChangesOverlap { .. })));
        if auto_stash || history_conflict {
            let status = repo
                .working_tree_status()
                .map_err(|error| i18n::op::op_plan_failed_contract(i18n::Op::Pull, error))?;
            if Some(status.digest()) != plan.worktree_digest {
                return Err(Msg::PullFilesChangedDuringPlanning.t().to_string());
            }
            if history_conflict && (!status.staged.is_empty() || !status.unstaged.is_empty()) {
                plan.warnings
                    .push(PlanNote::Common(CommonNote::DirtyBlocksOp {
                        parts: DirtyParts {
                            staged: status.staged.len(),
                            modified: status.unstaged.len(),
                        },
                        before: OpPhrase::Merging,
                    }));
            }
            if auto_stash {
                plan.warnings.push(PlanNote::Pull(PullNote::AutoStash {
                    parts: DirtyParts {
                        staged: status.staged.len(),
                        modified: status.unstaged.len(),
                    },
                    untracked: status.untracked.len(),
                }));
                plan.recovery = Some(PlanRecovery {
                    kind: RecoveryKind::Pull(PullRecovery::PullAutoStash),
                    commands: Vec::new(),
                });
            }
        }
        // No-op is local knowledge only. The Pull entry point must successfully
        // fetch before presenting it as up to date (ADR-0129 F-1: no title
        // string parsing).
        if plan.blockers.is_empty()
            && plan.warnings.is_empty()
            && matches!(
                plan.disposition,
                kagi_git::ops::PlanDisposition::NoOp(kagi_git::ops::NoOpKind::PullUpToDate)
            )
        {
            return Ok(None);
        }
        let dirty_digest = plan.worktree_digest;
        Ok(Some(PullPlanModal {
            plan: std::sync::Arc::new(plan),
            auto_stash,
            error: None,
            dirty_digest,
            fetch_owner: None,
            progress_request: None,
        }))
    }

    /// Close the pull modal without executing.
    pub fn cancel_pull_modal(&mut self) {
        self.clear_pull_modal();
        if matches!(
            self.app_sessions.plan_state(),
            PlanState::Ready {
                prepared: Planned::RemotePull { .. },
                ..
            }
        ) {
            self.app_sessions.invalidate_plan();
        }
    }

    /// W3-NOTIFY: UI-path pull — runs `pull_blocking` on a background thread
    /// so the window stays responsive, with start/finish toasts.
    pub fn start_pull(&mut self, cx: &mut Context<Self>) {
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.pull_modal().cloned() {
            Some(m) => m,
            None => return,
        };

        let progress_identity = modal
            .progress_request
            .map(crate::ui::busy::BusyIdentity::Planning)
            .or_else(|| modal.fetch_owner.map(crate::ui::busy::BusyIdentity::pull));
        let progress_hidden = self.busy_request_hidden(progress_identity);

        if let Some(rv) = self.remote_view.as_ref() {
            let PlanState::Ready {
                token,
                prepared: Planned::RemotePull { plan, request },
            } = self.app_sessions.plan_state()
            else {
                return;
            };
            if rv.host != request.owner.host
                || rv.root != request.owner.root
                || self.active_session() != Some(request.owner.session)
                || !std::sync::Arc::ptr_eq(&modal.plan, &plan.preview)
            {
                self.app_sessions.invalidate_plan();
                self.clear_pull_modal();
                return;
            }
            let token = token.clone();
            match app::approve(
                &mut self.app_sessions,
                token,
                app::Policy::Stash(kagi_git::backend::ExecutionPolicy::default()),
            ) {
                Ok(approved) => {
                    self.dispatch_job(approved, cx);
                    self.continue_busy_request(progress_identity, progress_hidden);
                }
                Err(error) => self.report_admission_refusal(error, cx),
            }
            return;
        }
        if modal.plan.blockers.is_empty()
            && modal
                .plan
                .warnings
                .iter()
                .any(|note| matches!(note, PlanNote::Pull(PullNote::MergePrediction)))
        {
            if let Some(identity) = modal.plan.pull_identity.as_ref() {
                let target = identity.upstream_ref.clone();
                let branch = identity.branch.clone();
                self.clear_pull_modal();
                self.open_merge_modal_with_progress(
                    target,
                    Some(branch),
                    progress_identity.map(|identity| (identity, progress_hidden)),
                    cx,
                );
                cx.notify();
            }
            return;
        }

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: pull plan has blockers, not executing");
            // #702 review P2: the UI authors no pull outcome at all. A known
            // blocker is a no-execute receipt like the runtime refusal, written
            // by the same core factory and only presented here.
            let refusal = refuse_blocked_pull(&repo_path, &modal.plan);
            self.present_refused_report("pull", &refusal, &modal.plan.blockers, &repo_path, cx);
            self.clear_pull_modal();
            cx.notify();
            return;
        }
        self.finish_pull(cx, modal, repo_path);
    }

    /// Build a push plan and open the confirmation modal.
    pub fn open_push_modal(&mut self, cx: &mut Context<Self>) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
                    SharedString::from("push: repo session unavailable"),
                ));
                return;
            }
        };
        match repo.plan_push() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: push blockers={} warnings={} preview_commits={}",
                    plan.blockers.len(),
                    plan.warnings.len(),
                    plan.preview_commits.len(),
                );
                // No-op push (already up to date — nothing to push) is not worth
                // a blocking popup (user request): show a snackbar instead.
                // ops::plan_push sets NoOp(PushUpToDate) exactly when the
                // up-to-date blocker is the *only* blocker (ADR-0129 F-2:
                // no string-matching of blocker text).
                if matches!(
                    plan.disposition,
                    kagi_git::ops::PlanDisposition::NoOp(kagi_git::ops::NoOpKind::PushUpToDate)
                ) {
                    self.present_sync_noop("push", Msg::AlreadyUpToDatePush, false, cx);
                    return;
                }
                let needs_choice = plan.sync_needs_choice();
                self.set_push_modal(PushPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
                if needs_choice {
                    // #817: Enter / Escape reach the plan through the root.
                    self.focus_root_for_modal();
                } else {
                    self.start_push(cx);
                }
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
                    SharedString::from(i18n::op::op_plan_failed_contract(i18n::Op::Push, e)),
                ));
            }
        }
    }

    /// Close the push modal without executing.
    pub fn cancel_push_modal(&mut self) {
        self.clear_push_modal();
    }

    /// W3-NOTIFY: UI-path push — background thread + start/finish toasts.
    pub fn start_push(&mut self, cx: &mut Context<Self>) {
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.push_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: push plan has blockers, not executing");
            self.record_refused(
                "push",
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_push_modal();
            cx.notify();
            return;
        }

        self.clear_push_modal();
        self.status_footer = FooterStatus::Busy(crate::ui::operations::display_text(
            SharedString::from(Msg::BusyPush.t()),
        ));
        klog!("async: push started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        self.finish_run(
            cx,
            "push",
            i18n::Op::Push,
            modal.plan.clone(),
            repo_path,
            move || push_blocking(&bg_path, &plan),
            |outcome| Some(format!("finished — {}", push_summary(outcome))),
            move |done| match done {
                Ok(outcome) => {
                    let text = i18n::op::user_text::operations::push_result(outcome).into();
                    RunPresentation::status(
                        if matches!(outcome, kagi_git::OperationOutcome::Push(_)) {
                            FooterStatus::Success(text)
                        } else {
                            FooterStatus::Failed(text)
                        },
                    )
                }
                Err(_) => RunPresentation::none(),
            },
        );
    }
}
