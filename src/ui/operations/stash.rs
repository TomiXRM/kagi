//! Stash operations (push/apply/pop/drop + menu).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Open the stash push modal.
    ///
    /// Plans the stash push immediately and stores the result in
    /// `self.stash_push_modal`.  The input is initially empty (no message).
    pub fn open_stash_push_modal(&mut self, cx: &mut Context<Self>) {
        if self.stash_push_focus.is_none() {
            self.stash_push_focus = Some(cx.focus_handle());
        }
        self.set_stash_push_modal(StashPushModal {
            input: String::new(),
            input_state: None, // lazy (render)
            plan: None,
            error: None,
        });
        self.replan_stash_push();
    }

    /// Close the stash push modal without making any changes.
    pub fn cancel_stash_push_modal(&mut self) {
        self.clear_stash_push_modal();
    }

    /// Re-generate the live stash push plan from the current input.
    pub(crate) fn replan_stash_push(&mut self) {
        let message_str = match self.stash_push_modal() {
            Some(m) => m.input.clone(),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("replan_stash_push: repo open error: {}", e);
                return;
            }
        };
        let msg_opt = if message_str.is_empty() {
            None
        } else {
            Some(message_str.as_str())
        };
        match repo.plan_stash_push(msg_opt, true) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-push blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(modal) = self.stash_push_modal_mut() {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                klog!("plan: stash-push error: {}", e);
            }
        }
    }

    /// Confirm the stash push plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_stash_push(&mut self, cx: &mut Context<Self>) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.stash_push_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            klog!("refused: stash-push plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Stashing copies the working tree (incl. untracked) into the stash
        // — minutes on big repos. Run it on a background thread (W3 pattern)
        // so the UI stays responsive instead of appearing frozen.
        self.busy_op = Some("stash");
        self.clear_stash_push_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStash.t()));
        klog!("async: stash-push started");

        let msg_opt = if modal.input.is_empty() {
            None
        } else {
            Some(modal.input.clone())
        };
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task =
            cx.background_spawn(async move { stash_push_blocking(&bg_path, &bg_plan, msg_opt) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: stash-push finished");
                        app.record_op(
                            "stash-push",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: stash-push failed — {}", err_msg);
                        app.record_op(
                            "stash-push",
                            plan.current.clone(),
                            OpOutcome::Failed { error: err_msg },
                            &repo_path,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Open the stash apply modal for stash entry at `index`.
    ///
    /// Plans the apply using the current repository state and stores the result.
    pub fn open_stash_apply_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                klog!("open_stash_apply_modal: no repo_path set");
                return;
            }
        };

        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("plan: stash-apply repo open error: {}", e);
                return;
            }
        };

        match repo.plan_stash_apply(index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-apply index={} blockers={} warnings={}",
                    index,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_stash_apply_modal(StashApplyModal {
                    index,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                klog!("plan: stash-apply error: {}", e);
            }
        }
    }

    /// Close the stash apply modal without making any changes.
    pub fn cancel_stash_apply_modal(&mut self) {
        self.clear_stash_apply_modal();
    }

    /// Confirm the stash apply plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    /// The stash entry is **never** removed (apply, not pop).
    pub fn confirm_stash_apply(&mut self) {
        let modal = match self.stash_apply_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = modal.plan.clone();
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            klog!("refused: stash-apply plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "stash-apply",
                    plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "stash-apply",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(m) = self.stash_apply_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (HEAD + stash count).
        if let Err(e) = repo.preflight_check_stash(&plan, plan.stash_count_at_plan()) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(m) = self.stash_apply_modal_mut() {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute stash apply (apply only — no pop, no drop).
        if let Err(e) = repo.execute_stash_apply(modal.index) {
            let err_msg = format!("Stash apply failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(m) = self.stash_apply_modal_mut() {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        klog!("executed: stash-apply index={}", modal.index);

        // Verify: check working tree is dirty and stash entry still exists.
        let mut repo2 = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("verify: repo open error: {}", e);
                self.reload();
                return;
            }
        };
        let after_summary = match repo2.snapshot(10_000) {
            Ok(snap) => {
                let is_dirty = snap.status.is_dirty();
                let stash_count = snap.stashes.len();
                if is_dirty {
                    klog!("verified: working tree dirty (stash applied)");
                } else {
                    klog!("verify: working tree NOT dirty after stash-apply");
                }
                // Stash must remain (apply, not pop).
                if stash_count >= plan.stash_count_at_plan() {
                    eprintln!(
                        "[kagi] verified: stash count={} (entry preserved)",
                        stash_count
                    );
                } else {
                    eprintln!(
                        "[kagi] verify: stash count={} (expected >= {})",
                        stash_count,
                        plan.stash_count_at_plan()
                    );
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_dirty {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                klog!("verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        };

        // Record success to oplog + update footer.
        self.record_op(
            "stash-apply",
            plan.current.clone(),
            OpOutcome::Success {
                after: after_summary,
            },
            &repo_path,
        );

        // Reload display data.
        self.reload();
    }

    /// Build a stash-pop plan and open the confirmation modal.
    pub fn open_pop_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "pop: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_stash_pop(index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-pop index={} blockers={} warnings={}",
                    index,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_pop_modal(PopPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    stash_index: index,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("pop plan error: {}", e)));
            }
        }
    }

    pub fn cancel_pop_modal(&mut self) {
        self.clear_pop_modal();
    }

    /// Open the standalone stash-drop confirmation (ADR-0087, Destructive).
    pub fn open_stash_drop_modal(&mut self, index: usize) {
        // Remote read-only view (ADR-0089 Phase 3): there is no local Repository
        // to dry-run against, so synthesise the danger-confirm plan; the drop
        // itself runs over SSH in `start_stash_drop`.
        if self.remote_view.is_some() {
            let label = self
                .active_view
                .stashes
                .iter()
                .find(|s| s.index == index)
                .map(|s| format!("stash@{{{}}}: {}", s.index, s.message))
                .unwrap_or_else(|| format!("stash@{{{index}}}"));
            let head = self.active_view.header.to_string();
            let plan = kagi::git::plan_stash_drop_remote(&label, head);
            eprintln!("[kagi] plan: remote stash-drop index={index} blockers=0");
            self.set_stash_drop_modal(StashDropModal {
                plan: std::sync::Arc::new(plan),
                error: None,
                stash_index: index,
            });
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "drop: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_stash_drop(index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-drop index={} blockers={}",
                    index,
                    plan.blockers.len()
                );
                self.set_stash_drop_modal(StashDropModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    stash_index: index,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("drop plan error: {}", e)));
            }
        }
    }

    pub fn cancel_stash_drop_modal(&mut self) {
        self.clear_stash_drop_modal();
    }

    /// Open the stash right-click context menu (Apply / Drop). Left-click on a
    /// stash row pops instead (handled in the sidebar row builder).
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
        _cx: &mut Context<Self>,
    ) {
        match action {
            stash_menu::StashAction::Pop => self.open_pop_modal(state.index),
            stash_menu::StashAction::Apply => self.open_stash_apply_modal(state.index),
            stash_menu::StashAction::Drop => self.open_stash_drop_modal(state.index),
        }
    }

    /// Execute the stash drop on a background thread (Destructive, ADR-0087).
    pub fn start_stash_drop(&mut self, cx: &mut Context<Self>) {
        let modal = match self.stash_drop_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }

        // Remote read-only view (ADR-0089 Phase 3): drop over SSH, then
        // re-snapshot. Same confirm + oplog discipline as the local path.
        if let Some(rv) = self.remote_view.clone() {
            let stash_index = modal.stash_index;
            let plan = modal.plan.clone();
            let before = plan.current.clone();
            let oplog_path = std::path::PathBuf::from(format!("{}:{}", rv.host.label(), rv.root));
            self.busy_op = Some("stash-drop");
            self.clear_stash_drop_modal();
            self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashDrop.t()));
            klog!("async: remote stash-drop started");
            let (host, root) = (rv.host.clone(), rv.root.clone());
            let task = cx.background_spawn(async move {
                kagi::remote::remote_stash_drop(&host, &root, stash_index)
                    .map_err(|e| e.to_string())
            });
            cx.spawn(async move |this, acx| {
                let result = task.await;
                let _ = this.update(acx, |app, cx| {
                    app.busy_op = None;
                    match result {
                        Ok(summary) => {
                            klog!("async: remote stash-drop finished");
                            app.record_op(
                                "stash-drop",
                                before.clone(),
                                OpOutcome::Success {
                                    after: kagi::git::StateSummary {
                                        head: before.head.clone(),
                                        dirty: "stash entry removed".to_string(),
                                    },
                                },
                                &oplog_path,
                            );
                            app.status_footer = FooterStatus::Success(SharedString::from(format!(
                                "stash drop: {summary}"
                            )));
                            // Re-snapshot the remote so the dropped entry and its
                            // graph row disappear (indices shift; one drop at a time).
                            app.refresh_remote_view(cx);
                        }
                        Err(err_msg) => {
                            klog!("async: remote stash-drop failed — {err_msg}");
                            app.record_op(
                                "stash-drop",
                                before.clone(),
                                OpOutcome::Failed {
                                    error: err_msg.clone(),
                                },
                                &oplog_path,
                            );
                            app.set_stash_drop_modal(StashDropModal {
                                plan: plan.clone(),
                                error: Some(SharedString::from(err_msg)),
                                stash_index,
                            });
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: drop plan has blockers, not executing");
            self.record_op(
                "stash-drop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.clear_stash_drop_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("stash-drop");
        self.clear_stash_drop_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashDrop.t()));
        klog!("async: stash-drop started");

        let plan = modal.plan.clone();
        let stash_index = modal.stash_index;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx
            .background_spawn(async move { stash_drop_blocking(&bg_path, &bg_plan, stash_index) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: stash-drop finished");
                        app.record_op(
                            "stash-drop",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash drop: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: stash-drop failed — {}", err_msg);
                        app.record_op(
                            "stash-drop",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.set_stash_drop_modal(StashDropModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            stash_index,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Confirm stash pop: preflight → apply-then-drop → oplog → reload.
    pub fn confirm_pop(&mut self) {
        let modal = match self.pop_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: pop plan has blockers, not executing");
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_pop_modal(PopPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    stash_index: modal.stash_index,
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.set_pop_modal(PopPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
                stash_index: modal.stash_index,
            });
            return;
        }
        match repo.execute_stash_pop(modal.stash_index) {
            Ok(()) => {
                klog!("executed: stash-pop index={}", modal.stash_index);
                self.clear_pop_modal();
                let after = StateSummary {
                    head: modal.plan.current.head.clone(),
                    dirty: "changes restored (stash removed)".to_string(),
                };
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from("stash pop: applied and dropped"));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Pop failed: {}", e);
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_pop_modal(PopPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    stash_index: modal.stash_index,
                });
            }
        }
    }

    /// W15-ASYNCOPS: UI-path stash-pop — background thread + start/finish toasts.
    /// Headless keeps `confirm_pop` (sync).
    pub fn start_pop(&mut self, cx: &mut Context<Self>) {
        let modal = match self.pop_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: pop plan has blockers, not executing");
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.clear_pop_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("stash-pop");
        self.clear_pop_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashPop.t()));
        klog!("async: stash-pop started");

        let plan = modal.plan.clone();
        let stash_index = modal.stash_index;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task =
            cx.background_spawn(async move { stash_pop_blocking(&bg_path, &bg_plan, stash_index) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: stash-pop finished");
                        app.record_op(
                            "stash-pop",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash pop: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: stash-pop failed — {}", err_msg);
                        app.record_op(
                            "stash-pop",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.set_pop_modal(PopPlanModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            stash_index,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
