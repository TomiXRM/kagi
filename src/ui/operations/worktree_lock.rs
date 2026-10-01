//! Worktree lock input, confirmation, and explicit unlock operations.
use crate::ui::*;

impl KagiApp {
    pub fn open_lock_worktree_modal(&mut self, name: String) {
        self.set_worktree_lock_reason_modal(WorktreeLockReasonModal {
            name,
            reason: Msg::WorktreeLockDefaultReason.t().to_string(),
            input_state: None,
            error: None,
        });
    }

    pub(crate) fn confirm_worktree_lock_reason(&mut self, cx: &mut Context<Self>) {
        let Some(modal) = self.worktree_lock_reason_modal() else {
            return;
        };
        let name = modal.name.clone();
        // Read the entity at confirmation, even if the last keystroke has not
        // reached the render-time String synchronization yet.
        let reason = modal.input_state.as_ref().map_or_else(
            || modal.reason.clone(),
            |input| input.read(cx).value().to_string(),
        );
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
                let error = SharedString::from(i18n::op_plan_failed(i18n::Op::LockWorktree, e));
                self.status_footer = FooterStatus::Failed(error.clone());
                if let Some(modal) = self.worktree_lock_reason_modal_mut() {
                    modal.error = Some(error);
                }
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
            self.record_refused(
                "lock-worktree",
                modal.plan.current.clone(),
                &modal.plan.blockers,
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
        // #885: the moves are observed (a lock only writes the worktree's
        // admin file, so normally: recorded, nothing moved).
        let (executed, ref_moves) = repo.observe_ref_moves(|repo| {
            repo.execute_lock_worktree(&modal.plan, &modal.name, Some(&modal.reason))
        });
        match executed {
            Ok(()) => {
                klog!("executed: lock-worktree {}", modal.name);
                self.record_op_persist_moves(
                    "lock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    ref_moves,
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
                self.record_op_persist_moves(
                    "lock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    ref_moves,
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.lock_worktree_modal_mut() {
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
        let repo = match crate::ui::blocking_ops::open_backend(&repo_path) {
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
                    auto: None,
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
            self.record_refused(
                "unlock-worktree",
                modal.plan.current.clone(),
                &modal.plan.blockers,
                &repo_path,
                cx,
            );
            self.clear_unlock_worktree_modal();
            cx.notify();
            return;
        }
        let repo = match crate::ui::blocking_ops::open_backend(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                if let Some(m) = self.unlock_worktree_modal_mut() {
                    m.error = Some(SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, e)));
                }
                return;
            }
        };
        // #772: the auto-lock's release offer runs the token/identity-checked
        // op; the manual unlock stays the manual op. Same card, same writer.
        let (executed, ref_moves) = repo.observe_ref_moves(|repo| match modal.auto.as_ref() {
            Some(target) => repo.execute_auto_unlock_worktree(&modal.plan, &modal.name, target),
            None => repo.execute_unlock_worktree(&modal.plan, &modal.name),
        });
        match executed {
            Ok(()) => {
                klog!("executed: unlock-worktree {}", modal.name);
                self.record_op_persist_moves(
                    "unlock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Success {
                        after: modal.plan.predicted.clone(),
                    },
                    ref_moves,
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
                self.record_op_persist_moves(
                    "unlock-worktree",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    ref_moves,
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
