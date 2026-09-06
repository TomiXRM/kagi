//! Local stash intent adapters; remote remains legacy until PR 2.
use crate::app::{self, PlanState, Planned, StashAction, StashPolicy, StashRequest};
use crate::ui::*;
impl KagiApp {
    pub(crate) fn present_stash_followup(&mut self, cx: &mut Context<Self>) {
        // `conflict_continue` notifies before its authoritative reload settles.
        // Do not consume the one-shot payload while the old ConflictView is still
        // mounted: that would start a plan whose modal is immediately cleared by
        // `apply_reload_data`, leaving nothing for the post-reload retry to show.
        if self.conflict.is_some() || self.has_active_modal() {
            return;
        }
        let Some(owner) = self.active_session() else {
            return;
        };
        let Some(payload) = self.app_sessions.take_stash_followup(owner) else {
            return;
        };
        self.set_stash_drop_modal(StashDropModal {
            stash_index: 0,
            plan: None,
            error: None,
        });
        self.begin_stash_plan_with_oid(StashAction::Drop { index: 0 }, Some(payload.oid), cx);
    }
    pub fn open_stash_push_modal(&mut self, cx: &mut Context<Self>) {
        if self.stash_push_focus.is_none() {
            self.stash_push_focus = Some(cx.focus_handle());
        }
        self.set_stash_push_modal(StashPushModal {
            input: String::new(),
            input_state: None,
            plan: None,
            error: None,
        });
        self.replan_stash_push(cx);
    }
    pub(crate) fn replan_stash_push(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.stash_push_modal() else {
            return;
        };
        let message = if m.input.is_empty() {
            None
        } else {
            Some(m.input.clone())
        };
        self.begin_stash_plan(
            StashAction::Push {
                message,
                include_untracked: true,
            },
            cx,
        );
    }
    pub fn open_stash_apply_modal(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.repo_path.is_none() {
            klog!("open_stash_apply_modal: no repo_path set");
            return;
        }
        self.set_stash_apply_modal(StashApplyModal {
            index,
            plan: None,
            error: None,
        });
        self.begin_stash_plan(StashAction::Apply { index }, cx);
    }
    pub fn open_pop_modal(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_pop_modal(PopPlanModal {
            stash_index: index,
            plan: None,
            error: None,
        });
        self.begin_stash_plan(StashAction::Pop { index }, cx);
    }
    pub fn open_stash_drop_modal(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.remote_view.is_some() {
            let label = self
                .active_view
                .stashes
                .iter()
                .find(|s| s.index == index)
                .map(|s| format!("stash@{{{}}}: {}", s.index, s.message))
                .unwrap_or_else(|| format!("stash@{{{index}}}"));
            let head = self.active_view.header.to_string();
            let plan = kagi_git::plan_stash_drop_remote(&label, head);
            klog!("plan: remote stash-drop index={index} blockers=0");
            self.set_stash_drop_modal(StashDropModal {
                plan: Some(std::sync::Arc::new(plan)),
                error: None,
                stash_index: index,
            });
            return;
        }

        self.set_stash_drop_modal(StashDropModal {
            stash_index: index,
            plan: None,
            error: None,
        });
        self.begin_stash_plan(StashAction::Drop { index }, cx);
    }
    pub(crate) fn stash_policy(&self) -> StashPolicy {
        crate::ui::blocking_ops::execution_policy()
    }
    fn begin_stash_plan(&mut self, action: StashAction, cx: &mut Context<Self>) {
        self.begin_stash_plan_with_oid(action, None, cx);
    }
    fn begin_stash_plan_with_oid(
        &mut self,
        action: StashAction,
        oid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
        else {
            return;
        };
        let policy = self.stash_policy();
        let request = StashRequest {
            owner,
            action: action.clone(),
        };
        let owner = request.owner.clone();
        let job = if let Some(oid) = oid {
            app::plan_stash_followup(&mut self.app_sessions, request.owner, oid, policy)
        } else {
            app::plan_stash(&mut self.app_sessions, request, policy)
        };
        let task = cx.background_spawn(async move { job.run() });
        cx.spawn(async move |this, cx| {
            let completion = task.await;
            let _ = this.update(cx, |app, cx| {
                if app.active_session() != Some(owner.session) || !app.stash_modal_matches(&action)
                {
                    if completion.is_current(&app.app_sessions) {
                        app.app_sessions.invalidate_plan();
                    }
                } else if app::apply_plan(&mut app.app_sessions, completion) {
                    app.show_stash_plan(&action, cx);
                }
                for job in app.app_sessions.take_plan_error_jobs() {
                    let task = cx.background_spawn(async move { job.run() });
                    cx.spawn(async move |this, cx| {
                        let completion = task.await;
                        let _ = this.update(cx, |app, cx| {
                            if let kagi_git::backend::recording::Recording::Failed {
                                attempted,
                                error,
                            } = &completion.recording
                            {
                                let message =
                                    format!("{}: recording failed: {}", attempted.repo, error);
                                app.push_toast(ToastKind::Error, message.clone(), cx);
                                app.app_notices.push_back(message.into());
                            }
                            app.app_sessions.apply_plan_error(completion);
                            cx.notify();
                        });
                    })
                    .detach();
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn stash_modal_matches(&self, action: &StashAction) -> bool {
        match action {
            StashAction::Push { message, .. } => self
                .stash_push_modal()
                .is_some_and(|m| m.input == message.as_deref().unwrap_or("")),
            StashAction::Apply { index } => {
                self.stash_apply_modal().is_some_and(|m| m.index == *index)
            }
            StashAction::Pop { index } => self.pop_modal().is_some_and(|m| m.stash_index == *index),
            StashAction::Drop { index } => self
                .stash_drop_modal()
                .is_some_and(|m| m.stash_index == *index),
        }
    }
    fn show_stash_plan(&mut self, action: &StashAction, cx: &mut Context<Self>) {
        if !self.stash_modal_matches(action) {
            self.app_sessions.invalidate_plan();
            return;
        }
        let (plan, error, resolved) = match self.app_sessions.plan_state() {
            PlanState::Ready {
                prepared: Planned::Stash { plan, request, .. },
                ..
            } => {
                if self.active_session() != Some(request.owner.session) {
                    self.app_sessions.invalidate_plan();
                    return;
                }
                (Some(plan.preview.clone()), None, request.action.clone())
            }
            PlanState::Error {
                error, open_failed, ..
            } => {
                match (action, open_failed) {
                    (StashAction::Push { .. }, true) => {
                        klog!("replan_stash_push: repo open error: {}", error)
                    }
                    (StashAction::Push { .. }, false) => klog!("plan: stash-push error: {}", error),
                    (StashAction::Apply { .. }, true) => {
                        klog!("plan: stash-apply repo open error: {}", error)
                    }
                    (StashAction::Apply { .. }, false) => {
                        klog!("plan: stash-apply error: {}", error)
                    }
                    _ => {}
                }
                (
                    None,
                    Some(SharedString::from(error.clone())),
                    action.clone(),
                )
            }
            PlanState::Draft => {
                self.clear_stash_drop_modal();
                return;
            }
            _ => return,
        };
        let action = &resolved;
        if let Some(p) = &plan {
            match action {
                StashAction::Push { .. } => klog!(
                    "plan: stash-push blockers={} warnings={}",
                    p.blockers.len(),
                    p.warnings.len()
                ),
                StashAction::Apply { index } => klog!(
                    "plan: stash-apply index={} blockers={} warnings={}",
                    index,
                    p.blockers.len(),
                    p.warnings.len()
                ),
                StashAction::Pop { index } => klog!(
                    "plan: stash-pop index={} blockers={} warnings={}",
                    index,
                    p.blockers.len(),
                    p.warnings.len()
                ),
                StashAction::Drop { index } => klog!(
                    "plan: stash-drop index={} blockers={}",
                    index,
                    p.blockers.len()
                ),
            }
        }
        if let Some(error) = &error {
            self.status_footer = FooterStatus::Failed(error.clone());
            self.push_toast(ToastKind::Error, error.clone(), cx);
        }
        match action {
            StashAction::Push { .. } => {
                if let Some(m) = self.stash_push_modal_mut() {
                    m.plan = plan;
                    m.error = error;
                }
            }
            StashAction::Apply { index } => self.set_stash_apply_modal(StashApplyModal {
                index: *index,
                plan,
                error,
            }),
            StashAction::Pop { index } => self.set_pop_modal(PopPlanModal {
                stash_index: *index,
                plan,
                error,
            }),
            StashAction::Drop { index } => self.set_stash_drop_modal(StashDropModal {
                stash_index: *index,
                plan,
                error,
            }),
        }
    }
    fn confirm_stash(&mut self, cx: &mut Context<Self>) {
        let PlanState::Ready {
            token,
            prepared: Planned::Stash {
                request, policy, ..
            },
        } = self.app_sessions.plan_state()
        else {
            return;
        };
        if self.active_session() != Some(request.owner.session)
            || !self.stash_modal_matches(&request.action)
        {
            self.app_sessions.invalidate_plan();
            return;
        }
        let token = token.clone();
        let current = self.stash_policy();
        if &current != policy {
            let action = request.action.clone();
            self.begin_stash_plan(action, cx);
            return;
        }
        match app::approve(&mut self.app_sessions, token, current) {
            Ok(approved) => self.dispatch_job(approved, cx),
            Err(error) => {
                self.app_notices.push_back(error.to_string().into());
                cx.notify();
            }
        }
    }
    pub fn confirm_stash_push(&mut self, cx: &mut Context<Self>) {
        // Flush pending input, but never approve before the fresh async plan arrives.
        if matches!(self.app_sessions.plan_state(), PlanState::Draft)
            && self
                .stash_push_modal()
                .is_some_and(|m| m.plan.is_none() && m.error.is_none())
        {
            self.modal_replan_gen = self.modal_replan_gen.wrapping_add(1);
            self.replan_stash_push(cx);
            return;
        }
        self.confirm_stash(cx);
    }
    pub fn confirm_stash_apply(&mut self, cx: &mut Context<Self>) {
        self.confirm_stash(cx);
    }
    pub fn start_pop(&mut self, cx: &mut Context<Self>) {
        self.confirm_stash(cx);
    }
    pub fn cancel_stash_push_modal(&mut self) {
        self.clear_stash_push_modal();
        self.app_sessions.invalidate_plan();
    }
    pub fn cancel_stash_apply_modal(&mut self) {
        self.clear_stash_apply_modal();
        self.app_sessions.invalidate_plan();
    }
    pub fn cancel_pop_modal(&mut self) {
        self.clear_pop_modal();
        self.app_sessions.invalidate_plan();
    }
    pub fn cancel_stash_drop_modal(&mut self) {
        self.clear_stash_drop_modal();
        self.app_sessions.invalidate_plan();
    }
    pub fn open_stash_menu(
        &mut self,
        index: usize,
        message: String,
        position: gpui::Point<gpui::Pixels>,
    ) {
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = Some(stash_menu::StashMenuState {
            index,
            message,
            position,
        });
        klog!("stash-menu: open index={}", index);
    }

    /// Dispatch a stash context-menu action.
    pub fn dispatch_stash_action(
        &mut self,
        action: stash_menu::StashAction,
        state: stash_menu::StashMenuState,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            stash_menu::StashAction::Peek => self.open_stash_peek(state.index, cx),
            stash_menu::StashAction::Pop => self.open_pop_modal(state.index, cx),
            stash_menu::StashAction::Apply => self.open_stash_apply_modal(state.index, cx),
            stash_menu::StashAction::Drop => self.open_stash_drop_modal(state.index, cx),
        }
    }

    pub fn start_stash_drop(&mut self, cx: &mut Context<Self>) {
        if self.remote_view.is_none() {
            self.confirm_stash(cx);
            return;
        }
        let Some(modal) = self.stash_drop_modal().cloned() else {
            return;
        };
        if self.reject_if_busy(cx) {
            return;
        }
        if let Some(rv) = self.remote_view.clone() {
            let stash_index = modal.stash_index;
            let Some(plan) = modal.plan.clone() else {
                return;
            };
            let before = plan.current.clone();
            let oplog_path = std::path::PathBuf::from(format!("{}:{}", rv.host.label(), rv.root));
            self.busy_op = Some("stash-drop");
            self.clear_stash_drop_modal();
            self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashDrop.t()));
            klog!("async: remote stash-drop started");
            let (host, root) = (rv.host.clone(), rv.root.clone());
            let task = cx.background_spawn(async move {
                crate::remote::remote_stash_drop(&host, &root, stash_index, &before)
                    .map_err(|e| e.to_string())
            });
            self.finish_op_on_main(cx, task, move |app, result, cx| match result {
                Ok(summary) => {
                    klog!("async: remote stash-drop finished");
                    app.record_op(
                        "stash-drop",
                        plan.current.clone(),
                        OpOutcome::Success {
                            after: kagi_git::StateSummary {
                                head: plan.current.head.clone(),
                                dirty: "stash entry removed".to_string(),
                            },
                        },
                        &oplog_path,
                        cx,
                    );
                    app.status_footer =
                        FooterStatus::Success(SharedString::from(format!("stash drop: {summary}")));
                    // Re-snapshot the remote so the dropped entry and its
                    // graph row disappear (indices shift; one drop at a time).
                    app.refresh_remote_view(cx);
                }
                Err(err_msg) => {
                    klog!("async: remote stash-drop failed — {err_msg}");
                    app.record_op(
                        "stash-drop",
                        plan.current.clone(),
                        OpOutcome::Failed {
                            error: err_msg.clone(),
                        },
                        &oplog_path,
                        cx,
                    );
                    app.set_stash_drop_modal(StashDropModal {
                        plan: Some(plan.clone()),
                        error: Some(SharedString::from(err_msg)),
                        stash_index,
                    });
                }
            });
        }
    }
}
