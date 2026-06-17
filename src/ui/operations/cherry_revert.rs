//! Cherry-pick and revert operations.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Open the cherry-pick plan modal for commit `id`.
    ///
    /// Plans the cherry-pick using the current repository state (in-memory,
    /// no working-tree modification) and stores the result in
    /// `self.cherry_pick_modal`.  Emits a plan log entry.
    pub fn open_cherry_pick_modal(&mut self, commit_id: CommitId) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_cherry_pick_modal: no repo_path set");
                return;
            }
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] cherry-pick plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_cherry_pick(&commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: cherry-pick {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                self.cherry_pick_modal = Some(CherryPickModal {
                    commit_id,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] cherry-pick plan: error: {}", e);
            }
        }
    }

    /// Cancel and close the cherry-pick modal without making any changes.
    pub fn cancel_cherry_pick_modal(&mut self) {
        self.cherry_pick_modal = None;
    }

    /// W15-ASYNCOPS: UI-path cherry-pick — background thread + start/finish
    /// toasts. The headless KAGI_* path executes `execute_cherry_pick` directly.
    pub fn start_cherry_pick(&mut self, cx: &mut Context<Self>) {
        let modal = match self.cherry_pick_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: cherry-pick plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
                    rp,
                );
            }
            self.cherry_pick_modal = None;
            cx.notify();
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        self.busy_op = Some("cherry-pick");
        self.cherry_pick_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCherryPick.t()));
        eprintln!("[kagi] async: cherry-pick started");

        let plan = modal.plan.clone();
        let commit_id = modal.commit_id.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_commit = commit_id.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the op (main thread).
        let history_before = self.head_branch_and_sha();
        let task = cx
            .background_spawn(async move { cherry_pick_blocking(&bg_path, &bg_plan, &bg_commit) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_summary, after)) => {
                        eprintln!("[kagi] async: cherry-pick finished");
                        app.record_op(
                            "cherry-pick",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi::git::OperationKind::CherryPick,
                                &branch,
                                before,
                                after_sha,
                                format!("cherry-pick {}", commit_id.short()),
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: cherry-pick failed — {}", err_msg);
                        app.record_op(
                            "cherry-pick",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.cherry_pick_modal = Some(CherryPickModal {
                            commit_id: commit_id.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Open the revert plan modal for commit `id`.
    pub fn open_revert_modal(&mut self, commit_id: CommitId) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_revert_modal: no repo_path set");
                return;
            }
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] revert plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_revert(&commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: revert {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                self.revert_modal = Some(RevertModal {
                    commit_id,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] revert plan: error: {}", e);
            }
        }
    }

    /// Cancel and close the revert modal without making any changes.
    pub fn cancel_revert_modal(&mut self) {
        self.revert_modal = None;
    }

    /// W15-ASYNCOPS: UI-path revert — background thread + start/finish toasts.
    /// The headless KAGI_* path executes `execute_revert` directly.
    pub fn start_revert(&mut self, cx: &mut Context<Self>) {
        let modal = match self.revert_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: revert plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
                    rp,
                );
            }
            self.revert_modal = None;
            cx.notify();
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        self.busy_op = Some("revert");
        self.revert_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyRevert.t()));
        eprintln!("[kagi] async: revert started");

        let plan = modal.plan.clone();
        let commit_id = modal.commit_id.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_commit = commit_id.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the op (main thread).
        let history_before = self.head_branch_and_sha();
        let task =
            cx.background_spawn(async move { revert_blocking(&bg_path, &bg_plan, &bg_commit) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_summary, after)) => {
                        eprintln!("[kagi] async: revert finished");
                        app.record_op(
                            "revert",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi::git::OperationKind::Revert,
                                &branch,
                                before,
                                after_sha,
                                format!("revert {}", commit_id.short()),
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: revert failed — {}", err_msg);
                        app.record_op(
                            "revert",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.revert_modal = Some(RevertModal {
                            commit_id: commit_id.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
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
