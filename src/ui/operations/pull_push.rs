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

use crate::ui::operations::PullConfirmDelivery;
use crate::ui::*;

impl KagiApp {
    /// Build a pull plan and open the confirmation modal.
    pub fn open_pull_modal(&mut self, cx: &mut Context<Self>) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.op_latched() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // Remote read-only view (ADR-0089 Phase 3): synthesise the plan from the
        // snapshot's ahead/behind; the pull runs over SSH in `start_pull`.
        if let Some(rv) = self.remote_view.clone() {
            let owner = PathBuf::from(format!("{}:{}", rv.host.label(), rv.root));
            if self.reject_transport_hold(&owner, "pull") {
                cx.notify();
                return;
            }
            let s = &self.view().status_summary;
            let branch = s.branch.clone();
            let behind = s.behind.unwrap_or(0);
            let ahead = s.ahead.unwrap_or(0);
            // Nothing to pull by local knowledge → snackbar, no modal (as local).
            if behind == 0 {
                self.push_toast(
                    ToastKind::Sync,
                    SharedString::from(Msg::AlreadyUpToDatePull.t()),
                    cx,
                );
                self.status_footer = FooterStatus::Idle(SharedString::from(""));
                return;
            }
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
            self.set_pull_modal(PullPlanModal {
                plan: std::sync::Arc::new(plan),
                auto_stash: false,
                error: None,
                // A remote pull stashes nothing locally.
                dirty_digest: None,
            });
            return;
        }
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // #625 / ADR-0192: a dirty Pull auto-stashes and restores, so its plan
        // must name the paths whose restore would conflict — and `plan_pull`
        // only knows the origin refs kagi already has. Auto-fetch runs every
        // 180s, so the modal could easily be planned against an upstream tip
        // minutes old and promise a clean restore that then fails. Fetch first
        // (a read that never touches the working tree), then plan.
        if self.view().is_dirty {
            if let Some(session) = self.active_session() {
                // The request rides *inside* this fetch's task (#626 review):
                // no global flag, so no unrelated fetch can consume it later
                // and no reload can drop it. Delivery rules live in
                // `deliver_pull_confirm`.
                // `false` means no fetch took the request: nothing started
                // and nothing in flight is refreshing this repo.
                if !self.fetch_async_for(false, Some(session), cx) {
                    // Plan on local knowledge rather than swallowing the
                    // user's click (no lease, no remote, or a fetch running for
                    // another repo). The plan is still the honest one — just as
                    // fresh as kagi's last fetch.
                    self.plan_and_open_pull_modal(cx);
                }
                return;
            }
        }
        self.plan_and_open_pull_modal(cx);
    }

    /// Deliver the confirmation a dirty Pull asked for when *its* fetch task
    /// finishes (#625, ADR-0192; #626 review).
    ///
    /// Every case is decided here rather than at the call site, because the
    /// three earlier attempts each fixed one branch and left another:
    ///
    /// | state at completion | delivery |
    /// |---|---|
    /// | fetch failed | oplog entry always; notice modal now if the tab is on screen, else parked |
    /// | ok, tab on screen, no other modal | plan and open the confirmation |
    /// | ok, tab **not** on screen | parked; opened when that tab is next activated |
    /// | ok, another modal open | request cancelled — the user's newer modal wins |
    /// | requesting tab closed | dropped with the tab |
    ///
    /// Parking is what makes "press Pull, switch tabs, come back" work: the
    /// answer belongs to the tab that asked, so it waits for that tab instead
    /// of being lost (or opening over a different repository).
    pub(crate) fn deliver_pull_confirm(
        &mut self,
        session: crate::app::SessionId,
        fetch_error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.app_sessions.is_attached(session) {
            klog!("pull-confirm: dropped (tab closed)");
            return;
        }
        if let Some(error) = fetch_error {
            // Persisted first: the oplog entry must exist even when the modal
            // has to wait for its tab.
            self.record_pull_fetch_failure(session, &error, cx);
            if self.active_session() == Some(session) {
                self.set_app_notice(i18n::op_failed(i18n::Op::Fetch, &error).into());
            } else {
                self.pending_pull_confirm
                    .insert(session, PullConfirmDelivery::FetchFailed(error));
            }
            return;
        }
        if self.active_session() != Some(session) {
            self.pending_pull_confirm
                .insert(session, PullConfirmDelivery::Confirm);
            klog!("pull-confirm: parked for its tab");
            return;
        }
        // "One modal at a time" is structural (ADR-0093). A confirmation that
        // opened while the fetch ran belongs to a newer decision and may hold
        // half-typed input, so this request yields instead of replacing it.
        if self.foreign_modal_open() {
            klog!("pull-confirm: cancelled (another modal is open)");
            return;
        }
        self.plan_and_open_pull_modal(cx);
    }

    /// Deliver a parked Pull confirmation to the tab that asked for it, now
    /// that it is on screen again.
    pub(crate) fn deliver_parked_pull_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.active_session() else {
            return;
        };
        let Some(parked) = self.pending_pull_confirm.remove(&session) else {
            return;
        };
        if self.foreign_modal_open() {
            klog!("pull-confirm: cancelled (another modal is open)");
            return;
        }
        match parked {
            PullConfirmDelivery::Confirm => {
                klog!("pull-confirm: delivered on tab activation");
                self.plan_and_open_pull_modal(cx);
            }
            PullConfirmDelivery::FetchFailed(error) => {
                self.set_app_notice(i18n::op_failed(i18n::Op::Fetch, &error).into());
            }
        }
    }

    /// Is a modal other than a Pull confirmation on screen?
    fn foreign_modal_open(&self) -> bool {
        self.has_active_modal() && self.pull_modal().is_none()
    }

    /// #625: a fetch run *for* a Pull confirmation failed, so there is no
    /// confirmation to show — but the user pressed Pull and is owed an answer
    /// that outlives a toast (CLAUDE.md: user-facing errors surface via the
    /// oplog **and** a modal).
    ///
    /// This half is the durable one and always runs, even when the modal has to
    /// wait for the tab that asked. The modal half is a notice, not the Pull
    /// confirmation: there is nothing to confirm, and a plan built on knowledge
    /// kagi just failed to refresh must not be confirmable. A notice is
    /// dismissed by the user, never by a reload.
    fn record_pull_fetch_failure(
        &mut self,
        session: crate::app::SessionId,
        error: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.app_sessions.attachment(session) else {
            return;
        };
        let repo_path = owner.path.clone();
        // The fetch has no OperationController boundary that records it, so it
        // takes ADR-0149's "non-run op" path and is persisted here.
        let before = StateSummary {
            head: format!("branch: {}", self.view().status_summary.branch),
            dirty: "unchanged".to_string(),
        };
        self.record_op_persist(
            "fetch",
            before,
            kagi_git::oplog::OpOutcome::Failed {
                error: i18n::op_failed(i18n::Op::Fetch, error),
            },
            &repo_path,
            cx,
        );
    }

    /// Plan a local pull and open (or skip) its confirmation modal. `true` when
    /// a confirmation is now on screen.
    ///
    /// Split from [`Self::open_pull_modal`] so the dirty path can run it after
    /// its fetch completes (#625) without duplicating the plan handling.
    pub(crate) fn plan_and_open_pull_modal(&mut self, cx: &mut Context<Self>) -> bool {
        match self.build_pull_modal() {
            Ok(Some(modal)) => {
                eprintln!(
                    "[kagi] plan: pull blockers={} warnings={}",
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                self.set_pull_modal(modal);
                true
            }
            // Already-up-to-date pull (nothing to pull by local knowledge)
            // is not worth a blocking popup (user request): snackbar instead.
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
                self.status_footer = FooterStatus::Failed(SharedString::from(error));
                false
            }
        }
    }

    /// Refresh an open dirty-Pull confirmation against reloaded repository
    /// state (#625, ADR-0192).
    ///
    /// A dirty Pull fetches before confirming, and that fetch fires the FS
    /// watcher — whose reload clears confirmation modals (ADR-0189), because a
    /// plan invalidated by a repository change must not be confirmable. Both
    /// halves of that hold here: the confirmation the user asked for **stays on
    /// screen until they act on it**, and it is re-planned so what they confirm
    /// is never the stale plan.
    ///
    /// A plan that now has nothing to confirm (someone else pulled meanwhile)
    /// or that fails to build leaves the existing modal in place rather than
    /// making the window empty under the user's cursor; `Backend::run`'s
    /// preflight is what refuses a stale confirmation at execute time.
    pub(crate) fn replan_pull_modal(&mut self) {
        match self.build_pull_modal() {
            Ok(Some(modal)) => {
                klog!(
                    "replan: pull blockers={} warnings={}",
                    modal.plan.blockers.len(),
                    modal.plan.warnings.len()
                );
                self.set_pull_modal(modal);
            }
            Ok(None) => klog!("replan: pull nothing to pull; keeping the confirmation"),
            Err(error) => klog!("replan: pull failed: {error}"),
        }
    }

    /// The confirmation a local pull would show: `Ok(None)` when there is
    /// nothing to pull, `Err` when the plan could not be built.
    fn build_pull_modal(&mut self) -> Result<Option<PullPlanModal>, String> {
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
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
        // Background auto-fetch keeps the behind count fresh; ops::plan_pull
        // sets NoOp(PullUpToDate) when behind == 0 (ADR-0129 F-1: no
        // string-parsing of the title).
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
        }))
    }

    /// Close the pull modal without executing.
    pub fn cancel_pull_modal(&mut self) {
        self.clear_pull_modal();
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

        // Remote read-only view (ADR-0089 Phase 3): pull over SSH (runs
        // `git pull` on the host), then re-snapshot. Same confirm + oplog path.
        if let Some(rv) = self.remote_view.clone() {
            let before = modal.plan.current.clone();
            let oplog_path = std::path::PathBuf::from(format!("{}:{}", rv.host.label(), rv.root));
            if self.reject_transport_hold(&oplog_path, "pull") {
                self.clear_pull_modal();
                self.present_app_notice();
                cx.notify();
                return;
            }
            self.mark_write_busy("pull");
            self.clear_pull_modal();
            self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
            klog!("async: remote pull started");
            let (host, root) = (rv.host.clone(), rv.root.clone());
            // #501: the transport records the attempt; this callback is
            // presentation only and may be dropped on a tab switch.
            let recorded_before = before.clone();
            let task = cx.background_spawn(async move {
                crate::remote::remote_pull(&host, &root, &recorded_before)
            });
            let notice_path = oplog_path.clone();
            self.finish_op_on_main_settled(
                cx,
                task,
                move |app, report: &crate::remote::RemotePullReport, _cx| {
                    app.notice_recording_failure("pull", &report.recording, &notice_path);
                    app.settle_transport(&notice_path, "pull", &report.recording.entry().outcome);
                },
                move |app, report, cx| {
                    let recorded_clean = matches!(
                        report.recording,
                        kagi_git::backend::recording::Recording::Appended { .. }
                    );
                    match &report.result {
                        Ok(summary) => {
                            klog!("async: remote pull finished — {summary}");
                            app.present_recorded(&report.recording, cx);
                            // A pull whose record never landed is not a clean
                            // success; the notice above already said so.
                            if recorded_clean {
                                app.status_footer = FooterStatus::Success(SharedString::from(
                                    format!("pull: {summary}"),
                                ));
                            }
                            app.refresh_remote_view(cx);
                        }
                        Err(error) => {
                            let err_msg = error.to_string();
                            klog!("async: remote pull failed — {err_msg}");
                            app.present_recorded(&report.recording, cx);
                            // Unknown/Partial changed the host: re-read rather
                            // than re-offering the same pull.
                            app.refresh_remote_view(cx);
                            if matches!(report.recording.entry().outcome, OpOutcome::Failed { .. })
                            {
                                app.set_pull_modal(PullPlanModal {
                                    plan: modal.plan.clone(),
                                    auto_stash: false,
                                    error: Some(SharedString::from(err_msg)),
                                    dirty_digest: modal.dirty_digest,
                                });
                            }
                        }
                    }
                },
            );
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
            self.present_report("pull", &refusal, &repo_path, cx);
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
        let repo = match self.repo_session.as_ref() {
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
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
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
            move |app, done, _cx| match done {
                Ok(outcome) => {
                    app.status_footer = FooterStatus::Success(SharedString::from(format!(
                        "push: {}",
                        push_summary(outcome)
                    )));
                }
                // #493 safety review: see `finish_pull` — the failure must reach
                // the modal, not just the oplog and the footer.
                Err(failure) => app.set_push_modal(PushPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(failure.message)),
                }),
            },
        );
    }
}
