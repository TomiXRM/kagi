//! Pull / push operations (open/confirm/start/finish).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Build a pull plan and open the confirmation modal.
    pub fn open_pull_modal(&mut self) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "pull: repo open error: {}",
                    e
                )));
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
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
                self.pull_modal = Some(PullPlanModal {
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
        self.pull_modal = None;
    }

    /// Confirm the pull plan synchronously: preflight, fetch via CLI, then
    /// FF / in-memory merge (see `execute_pull`).  Used by the headless
    /// KAGI_PULL path (no event loop). The UI button uses `start_pull`,
    /// which runs the same blocking core on a background thread (W3-NOTIFY).
    pub fn confirm_pull(&mut self) {
        let modal = match self.pull_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        match pull_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.pull_modal = None;
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
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
                );
                self.pull_modal = Some(PullPlanModal {
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
        let modal = match self.pull_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.pull_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("pull");
        self.pull_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
        eprintln!("[kagi] async: pull started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { pull_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_pull(result, modal, repo_path);
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
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                eprintln!("[kagi] async: pull finished — {}", summary);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("pull: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                eprintln!("[kagi] async: pull failed — {}", err_msg);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
    }

    /// Build a push plan and open the confirmation modal.
    pub fn open_push_modal(&mut self) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "push: repo open error: {}",
                    e
                )));
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
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
                self.push_modal = Some(PushPlanModal {
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
        self.push_modal = None;
    }

    /// Confirm the push plan synchronously: preflight, execute push via CLI.
    /// Used by the headless KAGI_PUSH path. The UI button uses `start_push`
    /// (background thread + toasts, W3-NOTIFY).
    pub fn confirm_push(&mut self) {
        let modal = match self.push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: push plan has blockers, not executing");
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        match push_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.push_modal = None;
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
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
                );
                self.push_modal = Some(PushPlanModal {
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
        let modal = match self.push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: push plan has blockers, not executing");
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.push_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("push");
        self.push_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPush.t()));
        eprintln!("[kagi] async: push started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { push_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_push(result, modal, repo_path);
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
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                eprintln!("[kagi] async: push finished — {}", summary);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: after_summary,
                    },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("push: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                eprintln!("[kagi] async: push failed — {}", err_msg);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
    }
}
