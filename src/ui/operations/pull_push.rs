//! Pull / push operations (open/confirm/start/finish).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;

use crate::ui::*;

impl KagiApp {
    /// Build a pull plan and open the confirmation modal.
    pub fn open_pull_modal(&mut self, cx: &mut Context<Self>) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // Remote read-only view (ADR-0089 Phase 3): synthesise the plan from the
        // snapshot's ahead/behind; the pull runs over SSH in `start_pull`.
        if self.remote_view.is_some() {
            let s = &self.active_view.status_summary;
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
                .active_view
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
                self.active_view.header.to_string(),
            );
            klog!("plan: remote pull branch={branch} behind={behind} ahead={ahead}");
            self.set_pull_modal(PullPlanModal {
                plan: std::sync::Arc::new(plan),
                error: None,
            });
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
                    FooterStatus::Failed(SharedString::from("pull: repo session unavailable"));
                return;
            }
        };
        match repo.plan_pull() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: pull blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // Already-up-to-date pull (nothing to pull by local knowledge)
                // is not worth a blocking popup (user request): snackbar instead.
                // Background auto-fetch keeps the behind count fresh; the title
                // carries the "up to date (local knowledge…)" behind-label from
                // ops::plan_pull when behind == 0.
                if plan.blockers.is_empty()
                    && plan.warnings.is_empty()
                    && plan.title.contains("up to date (local knowledge")
                {
                    self.push_toast(
                        ToastKind::Sync,
                        SharedString::from(Msg::AlreadyUpToDatePull.t()),
                        cx,
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
                self.set_pull_modal(PullPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("pull plan error: {}", e)));
            }
        }
    }

    /// Close the pull modal without executing.
    pub fn cancel_pull_modal(&mut self) {
        self.clear_pull_modal();
    }

    /// Confirm the pull plan synchronously: preflight, fetch via CLI, then
    /// FF / in-memory merge (see `execute_pull`).  Used by the headless
    /// KAGI_PULL path (no event loop). The UI button uses `start_pull`,
    /// which runs the same blocking core on a background thread (W3-NOTIFY).
    pub fn confirm_pull(&mut self, cx: &mut Context<Self>) {
        let modal = match self.pull_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }

        match pull_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.clear_pull_modal();
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                    cx,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("pull: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.set_pull_modal(PullPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// W3-NOTIFY: UI-path pull — runs `pull_blocking` on a background thread
    /// so the window stays responsive, with start/finish toasts.
    pub fn start_pull(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
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
            self.busy_op = Some("pull");
            self.clear_pull_modal();
            self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
            klog!("async: remote pull started");
            let (host, root) = (rv.host.clone(), rv.root.clone());
            let task = cx.background_spawn(async move {
                kagi::remote::remote_pull(&host, &root).map_err(|e| e.to_string())
            });
            cx.spawn(async move |this, acx| {
                let result = task.await;
                let _ = this.update(acx, |app, cx| {
                    app.busy_op = None;
                    match result {
                        Ok(summary) => {
                            klog!("async: remote pull finished — {summary}");
                            app.record_op(
                                "pull",
                                before.clone(),
                                OpOutcome::Success {
                                    after: kagi_git::StateSummary {
                                        head: before.head.clone(),
                                        dirty: summary.clone(),
                                    },
                                },
                                &oplog_path,
                                cx,
                            );
                            app.status_footer = FooterStatus::Success(SharedString::from(format!(
                                "pull: {summary}"
                            )));
                            app.refresh_remote_view(cx);
                        }
                        Err(err_msg) => {
                            klog!("async: remote pull failed — {err_msg}");
                            app.record_op(
                                "pull",
                                before.clone(),
                                OpOutcome::Failed {
                                    error: err_msg.clone(),
                                },
                                &oplog_path,
                                cx,
                            );
                            app.set_pull_modal(PullPlanModal {
                                plan: modal.plan.clone(),
                                error: Some(SharedString::from(err_msg)),
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
            klog!("refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_pull_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("pull");
        self.clear_pull_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
        klog!("async: pull started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { pull_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_pull(result, modal, repo_path, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Apply the result of a background pull on the main thread.
    fn finish_pull(
        &mut self,
        result: Result<(String, StateSummary), String>,
        modal: PullPlanModal,
        repo_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                klog!("async: pull finished — {}", summary);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                    cx,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("pull: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                klog!("async: pull failed — {}", err_msg);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                    cx,
                );
            }
        }
    }

    /// Build a push plan and open the confirmation modal.
    pub fn open_push_modal(&mut self, cx: &mut Context<Self>) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
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
                // a blocking popup (user request): show a snackbar instead. The
                // "nothing to push" blocker is the *only* blocker in this case
                // (see ops::plan_push step 6).
                if !plan.blockers.is_empty()
                    && plan.blockers.iter().all(|b| b.contains("nothing to push"))
                {
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
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("push plan error: {}", e)));
            }
        }
    }

    /// Close the push modal without executing.
    pub fn cancel_push_modal(&mut self) {
        self.clear_push_modal();
    }

    /// Confirm the push plan synchronously: preflight, execute push via CLI.
    /// Used by the headless KAGI_PUSH path. The UI button uses `start_push`
    /// (background thread + toasts, W3-NOTIFY).
    pub fn confirm_push(&mut self, cx: &mut Context<Self>) {
        let modal = match self.push_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: push plan has blockers, not executing");
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }

        match push_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.clear_push_modal();
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                    cx,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("push: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.set_push_modal(PushPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// W3-NOTIFY: UI-path push — background thread + start/finish toasts.
    pub fn start_push(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
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
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_push_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("push");
        self.clear_push_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPush.t()));
        klog!("async: push started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { push_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_push(result, modal, repo_path, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Apply the result of a background push on the main thread.
    fn finish_push(
        &mut self,
        result: Result<(String, StateSummary), String>,
        modal: PushPlanModal,
        repo_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                klog!("async: push finished — {}", summary);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                    cx,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("push: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                klog!("async: push failed — {}", err_msg);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                    cx,
                );
            }
        }
    }
}
