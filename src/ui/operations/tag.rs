//! Create-tag-here operation (branch-menu "Create tag here...").
//!
//! Mirrors `operations/branch.rs`'s create-branch flow minus the
//! checkout-after option — a tag is a ref only, never checked out.

use crate::ui::*;

impl KagiApp {
    /// Open the create-tag modal for the commit at `at`.
    pub fn open_create_tag_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        self.set_create_tag_modal(CreateTagModal {
            at,
            start_title,
            input: String::new(),
            input_state: None,
            plan: None,
            error: None,
        });
        self.replan_create_tag();
    }

    /// Close the create-tag modal without making any changes.
    pub fn cancel_create_tag_modal(&mut self) {
        self.clear_create_tag_modal();
    }

    /// Re-generate the live plan from the current modal input.
    pub(crate) fn replan_create_tag(&mut self) {
        let (at, name) = match self.create_tag_modal() {
            Some(m) => (m.at.clone(), m.input.clone()),
            None => return,
        };
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("replan_create_tag: repo session unavailable");
                return;
            }
        };
        match repo.plan_create_tag(&name, &at) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-tag '{}' blockers={} warnings={}",
                    name,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(modal) = self.create_tag_modal_mut() {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                klog!("plan: create-tag error: {}", e);
            }
        }
    }

    /// Confirm the create-tag plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_create_tag(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans();
        let modal = match self.create_tag_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            klog!("refused: create-tag plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-tag",
                    plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                    },
                    rp,
                    cx,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "create-tag",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.create_tag_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
        // in one place.
        let op = kagi_git::Operation::CreateTag {
            name: modal.input.clone(),
            at: modal.at.clone(),
        };
        if let Err(e) = repo.run(&op, &plan) {
            let err_msg = format!("Create tag failed: {}", e);
            self.record_op(
                "create-tag",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
                cx,
            );
            if let Some(m) = self.create_tag_modal_mut() {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!(
            "[kagi] executed: create-tag '{}' @ {}",
            modal.input,
            modal.at.short()
        );

        let create_after = StateSummary {
            head: plan.current.head.clone(),
            dirty: plan.current.dirty.clone(),
        };
        self.record_op(
            "create-tag",
            plan.current.clone(),
            OpOutcome::Success {
                after: create_after,
            },
            &repo_path,
            cx,
        );

        self.clear_create_tag_modal();
        self.reload(cx);
    }
}

// ────────────────────────────────────────────────────────────
// Push tag (ADR-0140)
// ────────────────────────────────────────────────────────────

impl KagiApp {
    /// Right-click on a sidebar tag row. Resolves the push remote up front so
    /// the menu label can name it (and so the item can be *disabled with a
    /// reason* rather than hidden when there is no remote).
    pub fn open_tag_menu(&mut self, name: String, position: gpui::Point<gpui::Pixels>) {
        let remote = self
            .repo_session
            .as_ref()
            .and_then(|s| s.backend().push_tag_remote());
        self.tag_menu = Some(tag_menu::TagMenuState {
            name,
            position,
            remote,
        });
        klog!("tag-menu: open");
    }

    pub fn dispatch_tag_action(
        &mut self,
        action: tag_menu::TagAction,
        state: tag_menu::TagMenuState,
        cx: &mut Context<Self>,
    ) {
        match action {
            tag_menu::TagAction::Push => self.open_push_tag_modal(state.name, cx),
            tag_menu::TagAction::CopyName => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(state.name.clone()));
                self.push_toast(ToastKind::Info, i18n::copied_fmt(&state.name), cx);
            }
        }
        cx.notify();
    }

    /// Plan the push and show the confirmation. Single confirm, not armed:
    /// publishing a tag adds a ref on the remote and never moves or removes
    /// one (`plan_push_tag` never forces), so it is Guarded, not Destructive.
    pub fn open_push_tag_modal(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(session) = self.repo_session.as_ref() else {
            self.status_footer =
                FooterStatus::Failed(SharedString::from("push-tag: repo session unavailable"));
            return;
        };
        let repo = session.backend();
        let remote = repo.push_tag_remote().unwrap_or_default();
        match repo.plan_push_tag(&name) {
            Ok(plan) => {
                klog!("plan: push-tag blockers={}", plan.blockers.len());
                self.set_push_tag_modal(PushTagModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    remote,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("push-tag plan error: {}", e)));
            }
        }
        cx.notify();
    }

    pub fn cancel_push_tag_modal(&mut self) {
        self.clear_push_tag_modal();
    }

    /// Confirm: refuse a blocked plan (recording it), otherwise run the push on
    /// a background thread and reload so the tag list reflects reality.
    pub fn start_push_tag(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.push_tag_modal().cloned() else {
            return;
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if !modal.plan.blockers.is_empty() {
            klog!(
                "refused: push-tag plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "push-tag",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_push_tag_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("push-tag");
        self.clear_push_tag_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPushTag.t()));
        klog!("async: push-tag started");

        let plan = modal.plan.clone();
        let (bg_path, bg_plan) = (repo_path.clone(), plan.clone());
        let (name, remote) = (modal.name.clone(), modal.remote.clone());
        let task = cx.background_spawn(async move {
            let run = || -> Result<kagi_git::StateSummary, String> {
                let mut repo = kagi_git::Backend::open(&bg_path)
                    .map_err(|e| format!("Repo open error: {}", e))?;
                let op = kagi_git::Operation::PushTag {
                    name: name.clone(),
                    remote: remote.clone(),
                };
                repo.run(&op, &bg_plan)
                    .map_err(|e| format!("Push tag failed: {}", e))?;

                klog!("executed: push-tag {} -> {}", name, remote);
                Ok(kagi_git::StateSummary {
                    head: bg_plan.current.head.clone(),
                    dirty: format!("tag '{}' pushed to '{}'", name, remote),
                })
            };
            run()
        });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(after) => {
                klog!("async: push-tag finished");
                app.record_op(
                    "push-tag",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.status_footer = FooterStatus::Success(SharedString::from(Msg::PushTagDone.t()));
                app.reload(cx);
            }
            Err(err_msg) => {
                app.record_op(
                    "push-tag",
                    plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                // The remote refusing a moved tag lands here — its own message
                // says exactly why, so show it rather than paraphrasing.
                app.set_push_tag_modal(PushTagModal {
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    name: modal.name.clone(),
                    remote: modal.remote.clone(),
                });
            }
        });
    }
}
