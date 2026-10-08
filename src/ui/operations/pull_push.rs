//! Pull / push operations (open/confirm/start/finish).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;
use kagi_domain::plan_note::{
    CommonNote, DirtyParts, PlanNote, PlanRecovery, PullNote, PullRecovery, RecoveryKind,
    UntrackedCtx,
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
            && self.fetch_async_for(false, self.active_session(), cx)
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
            self.planning = Some("pull");
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
                                    app.offer_plan_from_async(
                                        AsyncPlanOffer::new(
                                            i18n::Op::Pull,
                                            ActiveModal::Pull(PullPlanModal {
                                                plan: preview,
                                                auto_stash: false,
                                                error: None,
                                                dirty_digest: None,
                                                fetch_owner: None,
                                            }),
                                        )
                                        .with_session_token(),
                                    );
                                }
                                PlanState::Error { error, blocker, .. } => {
                                    let reason = blocker.as_ref().map(i18n::plan_note_text);
                                    let message =
                                        SharedString::from(reason.unwrap_or_else(|| error.clone()));
                                    app.status_footer = FooterStatus::Failed(message.clone());
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
        let Some(session) = self.active_session() else {
            return;
        };
        // Verify cached structural blockers locally without doing an extra full
        // merge prediction on every healthy Pull. A stale cached blocker never
        // authorizes an executable plan or an up-to-date claim.
        let status = &self.view().status_summary;
        if status.is_detached || status.is_unborn || status.no_upstream || status.conflict_count > 0
        {
            match self.build_pull_modal() {
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
        let _ = self.fetch_async_for(false, Some(session), cx);
    }

    /// Deliver a successful fetch's confirmation only to the visit that
    /// requested it. The fetch completion records failures independently;
    /// a waiter must neither create a second receipt nor revive an old visit.
    pub(crate) fn deliver_pull_confirm(
        &mut self,
        session: crate::app::SessionId,
        displaced: bool,
        cx: &mut Context<Self>,
    ) {
        if self.active_session() == Some(session) {
            if displaced || self.has_active_modal() || self.op_latched() {
                self.discard_contended_plan_from_async(i18n::Op::Pull, AsyncPlanToken::None);
            } else {
                self.plan_and_offer_pull_modal_from_async(cx);
            }
        }
    }

    fn plan_and_offer_pull_modal_from_async(&mut self, cx: &mut Context<Self>) -> bool {
        match self.build_pull_modal() {
            Ok(Some(mut modal)) => {
                klog!(
                    "plan: pull blockers={} warnings={}",
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                modal.fetch_owner = self.active_session().and_then(|session| {
                    self.app_sessions
                        .visit(session)
                        .map(|visit| (session, visit))
                });
                self.offer_plan_from_async(AsyncPlanOffer::new(
                    i18n::Op::Pull,
                    ActiveModal::Pull(modal),
                ))
            }
            Ok(None) => {
                self.push_toast(
                    ToastKind::Sync,
                    SharedString::from(Msg::AlreadyUpToDatePull.t()),
                    cx,
                );
                self.status_footer = FooterStatus::Idle(SharedString::from(""));
                false
            }
            Err(error) => {
                self.report_plan_failure(i18n::Op::Pull, error);
                false
            }
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
        match self.build_pull_modal() {
            Ok(Some(mut modal)) => {
                // The accepted snapshot may predate a live checkout change.
                // Do not rewrite a clean approval to that different checkout.
                if self.pull_modal().is_some_and(|current| {
                    !current.auto_stash
                        && current.fetch_owner.is_some()
                        && (modal.auto_stash
                            || current.plan.head_at_plan != modal.plan.head_at_plan
                            || current.dirty_digest.as_ref() != modal.dirty_digest.as_ref())
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
    fn build_pull_modal(&mut self) -> Result<Option<PullPlanModal>, String> {
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => return Err("pull: repo session unavailable".to_string()),
        };
        let mut plan = repo
            .plan_pull()
            .map_err(|e| i18n::op_plan_failed(i18n::Op::Pull, e))?;
        let auto_stash = plan.blockers.is_empty() && self.view().is_dirty;
        if auto_stash {
            let status = &self.view().status_summary;
            // Auto-stash makes the two generic dirty warnings wrong —
            // kagi is about to stash, not refuse — so they are replaced
            // by `AutoStash` below. Only those two: every other note
            // stays, and `RestoreConflict` (#625) in particular must,
            // since naming the colliding paths before the user confirms
            // is the whole point of that note.
            plan.warnings.retain(|note| {
                !matches!(
                    note,
                    PlanNote::Pull(PullNote::DirtyPullGuard { .. })
                        | PlanNote::Common(CommonNote::UntrackedRemain {
                            ctx: UntrackedCtx::PullFetchMayTouch,
                            ..
                        })
                )
            });
            plan.warnings.push(PlanNote::Pull(PullNote::AutoStash {
                parts: DirtyParts {
                    staged: status.staged,
                    modified: status.unstaged,
                },
                untracked: status.untracked,
            }));
            plan.recovery = Some(PlanRecovery {
                kind: RecoveryKind::Pull(PullRecovery::PullAutoStash),
                commands: Vec::new(),
            });
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
        Ok(Some(PullPlanModal {
            plan: std::sync::Arc::new(plan),
            auto_stash,
            error: None,
            dirty_digest: repo
                .working_tree_status()
                .ok()
                .map(|status| status.digest()),
            fetch_owner: None,
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
                Ok(approved) => self.dispatch_job(approved, cx),
                Err(error) => self.report_admission_refusal(error, cx),
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

        self.clear_pull_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
        klog!("async: pull started");
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
                self.status_footer =
                    FooterStatus::Failed(SharedString::from("push: repo session unavailable"));
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
                    self.push_toast(
                        ToastKind::Sync,
                        SharedString::from(Msg::AlreadyUpToDatePush.t()),
                        cx,
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
                self.set_push_modal(PushPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
                // #817: Enter / Escape reach the plan through the root.
                self.focus_root_for_modal();
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::Push, e),
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
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPush.t()));
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
                Ok(outcome) => RunPresentation::status(FooterStatus::Success(SharedString::from(
                    format!("push: {}", push_summary(outcome)),
                ))),
                Err(_) => RunPresentation::none(),
            },
        );
    }
}
