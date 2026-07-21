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
