//! Checkout operations (branch/commit checkout, stash-before-checkout).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;

use crate::ui::*;

impl KagiApp {
    /// Open the checkout plan modal for `branch`.
    ///
    /// Plans the checkout using the current repository state and stores the
    /// result in `self.plan_modal`.  Emits a plan log entry.
    pub fn open_plan_modal(&mut self, branch: impl Into<String>) {
        let branch = branch.into();
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                klog!("open_plan_modal: no repo_path set");
                return;
            }
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!("plan: repo open error: {}", "session unavailable");
                return;
            }
        };

        match repo.plan_checkout(&branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout {} blockers={} warnings={}",
                    branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_plan_modal(CheckoutPlanModal {
                    stash_first: false,
                    target: CheckoutPlanTarget::Branch(branch.clone()),
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                klog!("plan: error: {}", e);
            }
        }
    }

    /// Double-click a local-branch pill → switch to that branch.
    ///
    /// Reuses [`open_plan_modal`](Self::open_plan_modal) to plan the checkout
    /// (and emit the same `[kagi] plan: checkout …` contract line + set the
    /// modal). When the plan is completely clean — no blockers **and** no
    /// warnings — the switch runs straight away via
    /// [`start_checkout`](Self::start_checkout), which consumes the modal before
    /// any render so the user never sees a popup. If the plan carries blockers
    /// or warnings the modal stays open so the user can review them first.
    pub fn dblclick_checkout_branch(&mut self, branch: impl Into<String>, cx: &mut Context<Self>) {
        let branch = branch.into();
        self.open_plan_modal(branch.clone());
        // `open_plan_modal` only sets the modal on a successful plan; treat a
        // missing modal (plan error) as "not clean" so nothing switches.
        let clean = self
            .plan_modal()
            .map(|m| m.plan.blockers.is_empty() && m.plan.warnings.is_empty())
            .unwrap_or(false);
        if clean {
            klog!("dblclick checkout: {} (clean, no modal)", branch);
            self.start_checkout(cx);
        }
    }

    /// Open the detached checkout plan modal for commit `commit_id`.
    pub fn open_checkout_commit_modal(&mut self, commit_id: CommitId) {
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                klog!("open_checkout_commit_modal: no repo_path set");
                return;
            }
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                klog!(
                    "checkout-commit plan: repo open error: {}",
                    "session unavailable"
                );
                return;
            }
        };

        match repo.plan_checkout_commit(&commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout-commit {} blockers={} warnings={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_plan_modal(CheckoutPlanModal {
                    stash_first: false,
                    target: CheckoutPlanTarget::Commit(commit_id),
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                klog!("checkout-commit plan: error: {}", e);
            }
        }
    }

    /// Confirm the plan: run preflight, execute checkout, then reload.
    ///
    /// On preflight or execute failure the modal remains open and shows the
    /// error text + recovery guidance.  The app never crashes.
    /// Stash the working tree ahead of an Enter-checkout. Returns `true`
    /// when the tree is clean afterwards; on Refused/Failed the plan modal
    /// shows the error and the checkout is aborted.
    fn stash_before_checkout(&mut self, cx: &mut Context<Self>) -> bool {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return false,
        };
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(format!("stash: repo open error: {}", e)));
                }
                return false;
            }
        };
        let msg = "kagi: auto-stash before checkout";
        let plan = match repo.plan_stash_push(Some(msg), true) {
            Ok(p) => p,
            Err(e) => {
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(format!("stash plan error: {}", e)));
                }
                return false;
            }
        };
        if !plan.blockers.is_empty() {
            klog!("refused: auto-stash has blockers, checkout aborted");
            self.record_op(
                "stash-push",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            if let Some(m) = self.plan_modal_mut() {
                m.error = Some(SharedString::from(format!(
                    "stash refused: {}",
                    plan.blockers
                        .iter()
                        .map(kagi_ui_core::i18n::plan_note_text)
                        .collect::<Vec<_>>()
                        .join(" / ")
                )));
            }
            return false;
        }
        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
        let stash_op = kagi_git::Operation::StashPush {
            message: Some(msg.to_string()),
            include_untracked: true,
        };
        match repo.run(&stash_op, &plan) {
            Ok(_) => {
                klog!("executed: auto-stash before checkout");
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Success {
                        after: plan.predicted.clone(),
                    },
                    &repo_path,
                    cx,
                );
                // Keep status fresh so the checkout preflight sees the
                // now-clean tree.
                self.reload(cx);
                true
            }
            Err(e) => {
                let err = format!("stash failed: {}", e);
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err.clone() },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(err));
                }
                false
            }
        }
    }

    pub fn confirm_checkout(&mut self, cx: &mut Context<Self>) {
        let modal = match self.plan_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        // Enter-checkout on a dirty tree: stash the changes first (plan
        // pipeline; refused/failed stash aborts the checkout with the error
        // shown in the modal).
        if modal.stash_first
            && self.active_view.status_summary.is_dirty
            && !self.stash_before_checkout(cx)
        {
            return;
        }
        // Defence in depth: the UI never renders the confirm button when
        // blockers exist, but refuse here too so no code path can execute a
        // blocked plan.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
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
        let op_name = match &modal.target {
            CheckoutPlanTarget::Branch(_) => "checkout",
            CheckoutPlanTarget::Commit(_) => "checkout-commit",
        };

        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.set_plan_modal(CheckoutPlanModal {
                    stash_first: false,
                    target: modal.target.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };

        // Preflight check.
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
                cx,
            );
            self.set_plan_modal(CheckoutPlanModal {
                stash_first: false,
                target: modal.target.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        // Execute checkout (safe mode only).
        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
        let checkout_op = match &modal.target {
            CheckoutPlanTarget::Branch(branch) => kagi_git::Operation::Checkout {
                branch: branch.clone(),
            },
            CheckoutPlanTarget::Commit(commit_id) => kagi_git::Operation::CheckoutCommit {
                id: commit_id.clone(),
            },
        };
        if let Err(e) = repo.run(&checkout_op, &modal.plan) {
            let err_msg = format!("Checkout failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
                cx,
            );
            self.set_plan_modal(CheckoutPlanModal {
                stash_first: false,
                target: modal.target.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        match &modal.target {
            CheckoutPlanTarget::Branch(branch) => klog!("executed: checkout {}", branch),
            CheckoutPlanTarget::Commit(commit_id) => {
                klog!("executed: checkout-commit {}", commit_id.short())
            }
        }

        // Verify: re-snapshot and confirm HEAD.
        let mut repo2 = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("verify: repo open error: {}", e);
                self.reload(cx);
                return;
            }
        };
        let after_summary = match repo2.snapshot(10_000) {
            Ok(snap) => {
                match (&modal.target, &snap.head) {
                    (
                        CheckoutPlanTarget::Branch(branch),
                        Head::Attached {
                            branch: actual_branch,
                            ..
                        },
                    ) if actual_branch == branch => {
                        klog!("verified: HEAD={}", actual_branch);
                    }
                    (CheckoutPlanTarget::Commit(commit_id), Head::Detached { target })
                        if target == &commit_id.0 =>
                    {
                        klog!("verified: detached HEAD={}", commit_id.short());
                    }
                    other => {
                        eprintln!(
                            "[kagi] verify: unexpected HEAD state after checkout: {:?}",
                            other
                        );
                    }
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                klog!("verify: snapshot error: {}", e);
                modal.plan.predicted.clone()
            }
        };

        // Record success to oplog + update footer.
        self.record_op(
            op_name,
            modal.plan.current.clone(),
            OpOutcome::Success {
                after: after_summary,
            },
            &repo_path,
            cx,
        );

        // Reload display data.
        self.reload(cx);
    }

    /// W15-ASYNCOPS: UI-path checkout — runs `checkout_blocking` on a background
    /// thread so a large `checkout_tree` write never freezes the window. The
    /// headless `KAGI_CHECKOUT*` path keeps using `confirm_checkout` (sync).
    pub fn start_checkout(&mut self, cx: &mut Context<Self>) {
        let modal = match self.plan_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // Enter-checkout on a dirty tree: stash the changes first (synchronous;
        // armed/two-stage style state stays on the main thread). A refused/failed
        // auto-stash aborts the checkout with the error shown in the modal.
        if modal.stash_first
            && self.active_view.status_summary.is_dirty
            && !self.stash_before_checkout(cx)
        {
            return;
        }
        // Defence in depth: never execute a blocked plan.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                    },
                    rp,
                    cx,
                );
            }
            self.clear_plan_modal();
            cx.notify();
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = match &modal.target {
            CheckoutPlanTarget::Branch(_) => "checkout",
            CheckoutPlanTarget::Commit(_) => "checkout-commit",
        };

        self.busy_op = Some("checkout");
        self.clear_plan_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        klog!("async: checkout started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_target = target.clone();
        let task =
            cx.background_spawn(async move { checkout_blocking(&bg_path, &bg_plan, &bg_target) });
        self.finish_op_on_main(cx, task, move |app, result, cx| match result {
            Ok((_summary, after)) => {
                klog!("async: checkout finished");
                app.record_op(
                    op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                app.reload(cx);
            }
            Err(err_msg) => {
                klog!("async: checkout failed — {}", err_msg);
                app.record_op(
                    op_name,
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                app.set_plan_modal(CheckoutPlanModal {
                    stash_first: false,
                    target: target.clone(),
                    plan: plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        });
    }

    /// Enter on a selected commit: open the checkout plan for it
    /// (branch checkout when a local branch points here, otherwise a
    /// detached commit checkout). On a dirty working tree the confirm
    /// stashes the changes first (user request) — surfaced as an extra
    /// plan warning + `stash_first` on the modal.
    pub fn checkout_selected_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt as _;
        if !self.root_has_focus(window) {
            return;
        }
        if self.busy_op.is_some() || self.repo_path.is_none() {
            return;
        }
        // Ignore Enter while any overlay / panel / text input is active.
        if self.plan_modal().is_some()
            || self.pull_modal().is_some()
            || self.push_modal().is_some()
            || self.branch_plan_modal().is_some()
            || self.set_upstream_modal().is_some()
            || self.rename_branch_modal().is_some()
            || self.merge_modal().is_some()
            || self.tracking_checkout_modal().is_some()
            || self.undo_modal().is_some()
            || self.history_modal().is_some()
            || self.amend_modal().is_some()
            || self.pop_modal().is_some()
            || self.create_branch_modal().is_some()
            || self.create_worktree_modal().is_some()
            || self.stash_push_modal().is_some()
            || self.stash_apply_modal().is_some()
            || self.cherry_pick_modal().is_some()
            || self.delete_branch_modal().is_some()
            || self.discard_modal().is_some()
            || self.commit_menu.is_some()
            || self.commit_panel_open
        {
            return;
        }
        if window.has_focused_input(cx) {
            return;
        }
        let Some(ix) = self.selected else {
            self.status_footer =
                FooterStatus::Idle(SharedString::from(Msg::CheckoutSelectFirst.t()));
            return;
        };
        let Some(ctx_info) = self.menu_context(ix) else {
            return;
        };
        if ctx_info.is_head {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::AlreadyHead.t()));
            return;
        }
        let Some(id) = self.commit_id_for_row(ix) else {
            return;
        };
        let dirty = self.active_view.status_summary.is_dirty;

        // Prefer a local branch pointing at the commit; fall back to a
        // detached commit checkout.
        let branch = ctx_info
            .refs_here
            .iter()
            .find(|b| matches!(b.kind, BadgeKind::Branch))
            .map(|b| b.label.to_string());
        match branch {
            Some(name) => self.open_plan_modal(name),
            None => self.open_checkout_commit_modal(id),
        }
        if dirty {
            if let Some(m) = self.plan_modal_mut() {
                m.stash_first = true;
                // Surface it in the plan card's warnings.
                let mut plan = (*m.plan).clone();
                plan.warnings.insert(
                    0,
                    kagi_git::ops::PlanNote::Common(kagi_git::ops::CommonNote::DirtyStashFirst),
                );
                m.plan = std::sync::Arc::new(plan);
            }
        }
        cx.notify();
    }
}
