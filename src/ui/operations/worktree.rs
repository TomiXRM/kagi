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
            localized_blockers: Vec::new(),
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
                // W29-I18N-WAVE2: localize the keyed branch-name reasons (only
                // when creating a new branch) and the keyed worktree-path reasons
                // (empty / already exists). Other blockers stay English.
                let mut keyed: Vec<(String, String)> = Vec::new();
                if !allow_existing_branch {
                    for e in repo.create_branch_name_errors(&branch) {
                        keyed.push((e.to_string(), crate::ui::i18n::branch_name_error(&e)));
                    }
                }
                if let Err(kagi_git::ops::WorktreeValidationError::Keyed(e)) =
                    repo.validate_worktree_path_keyed(&path)
                {
                    keyed.push((e.to_string(), crate::ui::i18n::worktree_path_error(&e)));
                }
                let localized = localize_plan_blockers(&plan.blockers, keyed.into_iter());
                if let Some(modal) = self.create_worktree_modal_mut() {
                    modal.plan = Some(std::sync::Arc::new(plan));
                    modal.localized_blockers = localized;
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
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        klog!("async: create-worktree finished");
                        app.record_op(
                            "create-worktree",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: create-worktree failed — {}", err_msg);
                        app.record_op(
                            "create-worktree",
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
}
