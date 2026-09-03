//! Worktree creation operations.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;

use crate::ui::*;

impl KagiApp {
    pub fn open_create_worktree_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        self.open_create_worktree_modal_prefilled(at, String::new(), false, cx);
    }

    pub fn open_create_worktree_modal_prefilled(
        &mut self,
        at: CommitId,
        branch_prefill: String,
        allow_existing_branch: bool,
        cx: &mut Context<Self>,
    ) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        let branch_input = branch_prefill;
        let default_branch = if branch_input.is_empty() {
            "new-branch"
        } else {
            branch_input.as_str()
        };
        let path_input = self.default_worktree_path(default_branch);
        self.set_create_worktree_modal(CreateWorktreeModal {
            at,
            start_title,
            branch_input,
            branch_state: None, // lazy (render)
            path_input,
            path_state: None, // lazy (render)
            path_touched: false,
            allow_existing_branch,
            plan: None,
            error: None,
        });
        self.replan_create_worktree();
    }

    pub fn cancel_create_worktree_modal(&mut self) {
        self.clear_create_worktree_modal();
    }

    pub(crate) fn default_worktree_path(&self, branch: &str) -> String {
        let repo_path = match self.repo_path.as_ref() {
            Some(path) => path,
            None => return String::new(),
        };
        let repo_name = repo_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo");
        let safe_branch: String = branch
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                    ch
                } else {
                    '-'
                }
            })
            .collect();
        let safe_branch = if safe_branch.is_empty() {
            "new-branch".to_string()
        } else {
            safe_branch
        };
        format!("../{}-worktrees/{}", repo_name, safe_branch)
    }

    pub(crate) fn replan_create_worktree(&mut self) {
        let (at, branch, path, allow_existing_branch) = match self.create_worktree_modal() {
            Some(m) => (
                m.at.clone(),
                m.branch_input.clone(),
                m.path_input.clone(),
                m.allow_existing_branch,
            ),
            None => return,
        };
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("replan_create_worktree: repo session unavailable");
                return;
            }
        };
        let plan_result = if allow_existing_branch {
            repo.plan_open_worktree_for_branch(&branch, &path)
        } else {
            repo.plan_create_worktree(&branch, &path, &at)
        };
        match plan_result {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-worktree '{}' path='{}' blockers={} warnings={}",
                    branch,
                    path,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // ADR-0129 Phase 3: the keyed branch-name and worktree-path
                // reasons are now typed (`CommonNote::BranchNameErrorKeyed` /
                // `WorktreePathErrorKeyed`) and localize automatically via
                // `plan_note_text()` — no separate localized-blocker
                // computation needed.
                if let Some(modal) = self.create_worktree_modal_mut() {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                klog!("plan: create-worktree error: {}", e);
            }
        }
    }

    /// W15-ASYNCOPS: UI-path create-worktree — checks out a full tree into a new
    /// linked worktree on a background thread. The headless KAGI_* path executes
    /// `execute_create_worktree` directly (no confirm_* wrapper). On failure the
    /// footer/toast carry the error (the modal is already closed, matching the
    /// stash async path).
    pub fn start_create_worktree(&mut self, cx: &mut Context<Self>) {
        // Rebuild from the latest input so a fast type-then-click can't execute
        // a stale plan.
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.create_worktree_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            klog!("refused: create-worktree plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-worktree",
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

        // issue #341: the plan visibly enumerated the (escaped) command steps and
        // said confirming trusts them. Confirming IS that consent, so record the
        // repository-level trust now; execute then runs the command steps.
        if kagi_git::ops::plan_requires_worktree_trust(&plan) {
            // issue #393: trust ONLY the exact config the plan showed. If it
            // changed between plan and confirm, refuse — do not trust or run the
            // unreviewed content; leave the modal open for a re-review.
            let sha = kagi_git::ops::plan_worktree_config_sha(&plan).unwrap_or("");
            if let Err(e) = kagi_git::ops::trust_worktree_config_at(&repo_path, sha) {
                klog!("refused: create-worktree config changed after plan, not executing");
                self.status_footer = FooterStatus::Failed(SharedString::from(e.to_string()));
                self.record_op(
                    "create-worktree",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: e.to_string(),
                    },
                    &repo_path,
                    cx,
                );
                return;
            }
            klog!("worktree: trusted .kagi/worktree.toml (post_create)");
        }

        self.busy_op = Some("create-worktree");
        self.clear_create_worktree_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCreateWorktree.t()));
        klog!("async: create-worktree started");

        let branch_input = modal.branch_input.clone();
        let path_input = modal.path_input.clone();
        let at = modal.at.clone();
        let allow_existing_branch = modal.allow_existing_branch;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx.background_spawn(async move {
            create_worktree_blocking(
                &bg_path,
                &bg_plan,
                &branch_input,
                &path_input,
                &at,
                allow_existing_branch,
            )
        });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok(after) => {
                klog!("async: create-worktree finished");
                app.record_op(
                    "create-worktree",
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: create-worktree failed — {}", err_msg);
                app.record_op(
                    "create-worktree",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                    cx,
                );
            }
        });
    }

    // ── Unlock worktree (sidebar right-click → Unlock worktree…) ─────────

    /// Open the worktree right-click context menu. The main worktree never
    /// gets here (the sidebar row installs no handler for it).
    pub fn open_worktree_menu(
        &mut self,
        name: String,
        locked: bool,
        position: gpui::Point<gpui::Pixels>,
    ) {
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = None;
        self.worktree_menu = Some(worktree_menu::WorktreeMenuState {
            name: name.clone(),
            locked,
            position,
        });
        klog!("worktree-menu: open '{}'", name);
    }

    /// Dispatch a worktree context-menu action.
    pub fn dispatch_worktree_action(
        &mut self,
        action: worktree_menu::WorktreeAction,
        state: worktree_menu::WorktreeMenuState,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        use worktree_menu::WorktreeAction::*;
        match action {
            Unlock => self.open_unlock_worktree_modal(state.name),
            Remove { delete_branch } => self.open_remove_worktree_modal(state.name, delete_branch),
            Lock => self.open_lock_worktree_modal(state.name),
            Prune => self.open_prune_worktrees_modal(),
            Repair => self.open_repair_worktrees_modal(),
        }
    }

    // ── issue #340: remove / lock / prune / repair (plan → confirm) ──

    /// Open a backend `Backend`, returning `None` and setting the footer on
    /// failure. Shared by the four lifecycle open_* methods below.
    fn worktree_backend(&mut self, op: &str) -> Option<kagi_git::Backend> {
        let repo_path = self.repo_path.clone()?;
        match kagi_git::Backend::open(&repo_path) {
            Ok(r) => Some(r),
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "{}: repo open error: {}",
                    op, e
                )));
                None
            }
        }
    }

    pub fn open_remove_worktree_modal(&mut self, name: String, delete_branch: bool) {
        let Some(repo) = self.worktree_backend("remove-worktree") else {
            return;
        };
        match repo.plan_remove_worktree(&name, delete_branch) {
            Ok(plan) => {
                klog!(
                    "plan: remove-worktree {} delete_branch={}",
                    name,
                    delete_branch
                );
                self.set_remove_worktree_modal(RemoveWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    delete_branch,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::RemoveWorktree, e),
                ));
            }
        }
    }

    pub fn cancel_remove_worktree_modal(&mut self) {
        self.clear_remove_worktree_modal();
    }

    /// Confirm remove: preflight → ODB-backup → containment-checked delete →
    /// prune → optional branch delete → verify → oplog → reload.
    pub fn confirm_remove_worktree(&mut self, cx: &mut Context<Self>) {
        let modal = match self.remove_worktree_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: remove-worktree plan has blockers, not executing");
            self.record_op(
                "remove-worktree",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_remove_worktree_modal();
            cx.notify();
            return;
        }
        let Some(repo) = self.worktree_backend("remove-worktree") else {
            return;
        };
        // issue #341: confirming a plan whose pre_remove note is trust-required
        // records repository-level trust so the command steps may run; an
        // untrusted (or failing) pre_remove command aborts the removal below.
        if kagi_git::ops::plan_requires_worktree_trust(&modal.plan) {
            // issue #393: trust ONLY the exact config the plan showed.
            let sha = kagi_git::ops::plan_worktree_config_sha(&modal.plan).unwrap_or("");
            if let Err(e) = repo.trust_worktree_config_for_worktree(&modal.name, sha) {
                klog!("refused: remove-worktree config changed after plan, not executing");
                let err_msg = e.to_string();
                self.record_op(
                    "remove-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.remove_worktree_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
            klog!("worktree: trusted .kagi/worktree.toml (pre_remove)");
        }
        match repo.execute_remove_worktree(&modal.plan, &modal.name, modal.delete_branch) {
            Ok(backups) => {
                klog!(
                    "executed: remove-worktree {} (backups={})",
                    modal.name,
                    backups.len()
                );
                self.record_op(
                    "remove-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.clear_remove_worktree_modal();
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "removed worktree '{}'",
                    modal.name
                )));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::RemoveWorktree, e);
                self.record_op(
                    "remove-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.remove_worktree_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
            }
        }
    }

    pub fn open_lock_worktree_modal(&mut self, name: String) {
        // Free-text reason entry is a follow-up; kagi records a default reason
        // so the lock is attributable in `git worktree list --porcelain`.
        let reason = Msg::WorktreeLockDefaultReason.t().to_string();
        let Some(repo) = self.worktree_backend("lock-worktree") else {
            return;
        };
        match repo.plan_lock_worktree(&name, Some(&reason)) {
            Ok(plan) => {
                klog!("plan: lock-worktree {}", name);
                self.set_lock_worktree_modal(LockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                    reason,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::LockWorktree, e),
                ));
            }
        }
    }

    pub fn cancel_lock_worktree_modal(&mut self) {
        self.clear_lock_worktree_modal();
    }

    pub fn confirm_lock_worktree(&mut self, cx: &mut Context<Self>) {
        let modal = match self.lock_worktree_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: lock-worktree plan has blockers, not executing");
            self.record_op(
                "lock-worktree",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_lock_worktree_modal();
            cx.notify();
            return;
        }
        let Some(repo) = self.worktree_backend("lock-worktree") else {
            return;
        };
        match repo.execute_lock_worktree(&modal.plan, &modal.name, Some(&modal.reason)) {
            Ok(()) => {
                klog!("executed: lock-worktree {}", modal.name);
                self.record_op(
                    "lock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.clear_lock_worktree_modal();
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "locked worktree '{}'",
                    modal.name
                )));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::LockWorktree, e);
                self.record_op(
                    "lock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.lock_worktree_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
            }
        }
    }

    pub fn open_prune_worktrees_modal(&mut self) {
        let Some(repo) = self.worktree_backend("prune-worktrees") else {
            return;
        };
        match repo.plan_prune_worktrees() {
            Ok(plan) => {
                klog!("plan: prune-worktrees");
                self.set_prune_worktrees_modal(PruneWorktreesModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::PruneWorktrees, e),
                ));
            }
        }
    }

    pub fn cancel_prune_worktrees_modal(&mut self) {
        self.clear_prune_worktrees_modal();
    }

    pub fn confirm_prune_worktrees(&mut self, cx: &mut Context<Self>) {
        let modal = match self.prune_worktrees_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: prune-worktrees plan has blockers, not executing");
            self.record_op(
                "prune-worktrees",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_prune_worktrees_modal();
            cx.notify();
            return;
        }
        let Some(repo) = self.worktree_backend("prune-worktrees") else {
            return;
        };
        match repo.execute_prune_worktrees(&modal.plan) {
            Ok(pruned) => {
                klog!("executed: prune-worktrees ({} pruned)", pruned);
                self.record_op(
                    "prune-worktrees",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.clear_prune_worktrees_modal();
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "pruned {} stale worktree(s)",
                    pruned
                )));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::PruneWorktrees, e);
                self.record_op(
                    "prune-worktrees",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.prune_worktrees_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
            }
        }
    }

    pub fn open_repair_worktrees_modal(&mut self) {
        let Some(repo) = self.worktree_backend("repair-worktrees") else {
            return;
        };
        match repo.plan_repair_worktrees() {
            Ok(plan) => {
                klog!("plan: repair-worktrees");
                self.set_repair_worktrees_modal(RepairWorktreesModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    i18n::op_plan_failed(i18n::Op::RepairWorktrees, e),
                ));
            }
        }
    }

    pub fn cancel_repair_worktrees_modal(&mut self) {
        self.clear_repair_worktrees_modal();
    }

    pub fn confirm_repair_worktrees(&mut self, cx: &mut Context<Self>) {
        let modal = match self.repair_worktrees_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(repo) = self.worktree_backend("repair-worktrees") else {
            return;
        };
        match repo.execute_repair_worktrees(&modal.plan) {
            Ok(()) => {
                klog!("executed: repair-worktrees");
                self.record_op(
                    "repair-worktrees",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.clear_repair_worktrees_modal();
                self.status_footer =
                    FooterStatus::Success(SharedString::from("repaired worktree links"));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::RepairWorktrees, e);
                self.record_op(
                    "repair-worktrees",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.repair_worktrees_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
            }
        }
    }

    /// Plan the unlock and open the confirmation modal. The plan's warning
    /// surfaces the recorded lock reason (a lock is deliberate protection).
    pub fn open_unlock_worktree_modal(&mut self, name: String) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "unlock-worktree: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_unlock_worktree(&name) {
            Ok(plan) => {
                klog!("plan: unlock-worktree {}", name);
                self.set_unlock_worktree_modal(UnlockWorktreeModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    name,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "unlock-worktree plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_unlock_worktree_modal(&mut self) {
        self.clear_unlock_worktree_modal();
    }

    /// Confirm the unlock: preflight → unlock → verify → oplog → reload.
    /// Unlock is an instant admin-file removal, so it runs synchronously.
    pub fn confirm_unlock_worktree(&mut self, cx: &mut Context<Self>) {
        let modal = match self.unlock_worktree_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: unlock-worktree plan has blockers, not executing");
            self.record_op(
                "unlock-worktree",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_unlock_worktree_modal();
            cx.notify();
            return;
        }
        let repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                if let Some(m) = self.unlock_worktree_modal_mut() {
                    m.error = Some(SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, e)));
                }
                return;
            }
        };
        match repo.execute_unlock_worktree(&modal.plan, &modal.name) {
            Ok(()) => {
                klog!("executed: unlock-worktree {}", modal.name);
                self.record_op(
                    "unlock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.clear_unlock_worktree_modal();
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "unlocked worktree '{}'",
                    modal.name
                )));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::UnlockWorktree, e);
                self.record_op(
                    "unlock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.unlock_worktree_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
            }
        }
    }
}
