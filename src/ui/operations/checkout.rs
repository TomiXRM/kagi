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
        // #510: a new request supersedes the previous one. Drop the old plan
        // before anything can fail, so a failure can never leave the modal (and
        // `dblclick_checkout_branch`'s clean check) holding the earlier target.
        self.clear_plan_modal();
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
                self.report_plan_failure(i18n::Op::Checkout, SESSION_UNAVAILABLE);
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
                self.report_plan_failure(i18n::Op::Checkout, e);
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
        // `open_plan_modal` clears the slot first and only refills it on a
        // successful plan, so a missing modal (plan error) is "not clean" and
        // nothing switches. #510: the target check makes that explicit — only a
        // plan produced by *this* request may start a checkout.
        let clean = self
            .plan_modal()
            .filter(|m| matches!(&m.target, CheckoutPlanTarget::Branch(b) if b == &branch))
            .map(|m| m.plan.blockers.is_empty() && m.plan.warnings.is_empty())
            .unwrap_or(false);
        if clean {
            klog!("dblclick checkout: {} (clean, no modal)", branch);
            self.start_checkout(cx);
        }
    }

    /// Open the detached checkout plan modal for commit `commit_id`.
    pub fn open_checkout_commit_modal(&mut self, commit_id: CommitId) {
        // #510: this request supersedes the previous plan (see `open_plan_modal`).
        self.clear_plan_modal();
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
                self.report_plan_failure(i18n::Op::Checkout, SESSION_UNAVAILABLE);
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
                self.report_plan_failure(i18n::Op::Checkout, e);
            }
        }
    }

    /// Stash the working tree ahead of an Enter-checkout. Returns `true`
    /// when the tree is clean afterwards; on Refused/Failed the plan modal
    /// shows the error and the checkout is aborted.
    fn stash_before_checkout(&mut self, cx: &mut Context<Self>) -> bool {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return false,
        };
        let mut repo = match crate::ui::blocking_ops::open_backend(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, e)));
                }
                return false;
            }
        };
        let msg = "kagi: auto-stash before checkout";
        let plan = match repo.plan_stash_push(Some(msg), true) {
            Ok(p) => p,
            Err(e) => {
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(i18n::op_plan_failed(i18n::Op::Stash, e)));
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
        let report = repo.run_recorded(&stash_op, &plan);
        match &report.result {
            Ok(_) => {
                klog!("executed: auto-stash before checkout");
                self.present_report("stash-push", &report, &repo_path, cx);
                // Keep status fresh so the checkout preflight sees the
                // now-clean tree.
                self.reload(cx);
                true
            }
            Err(e) => {
                let err = i18n::op_failed(i18n::Op::Stash, e);
                self.present_report("stash-push", &report, &repo_path, cx);
                if let Some(m) = self.plan_modal_mut() {
                    m.error = Some(SharedString::from(err));
                }
                false
            }
        }
    }

    /// W15-ASYNCOPS: UI-path checkout — runs `checkout_blocking` on a background
    /// thread so a large `checkout_tree` write never freezes the window.
    /// #493: the single checkout entry — the modal button and the root Enter
    /// dispatch both land here.
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
            && self.view().status_summary.is_dirty
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

        self.clear_plan_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        klog!("async: checkout started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let (bg_path, bg_plan, bg_target) = (repo_path.clone(), plan.clone(), target.clone());
        // ADR-0196 Wave 3: admitted through the application layer; the
        // `Invalidate` delivery reloads the tab, the receipt is the record.
        self.finish_run(
            cx,
            op_name,
            i18n::Op::Checkout,
            plan.clone(),
            repo_path,
            move || checkout_blocking(&bg_path, &bg_plan, &bg_target),
            |_| None,
            move |app, done, _cx| {
                if let Err(failure) = done {
                    app.set_plan_modal(CheckoutPlanModal {
                        stash_first: false,
                        target: target.clone(),
                        plan: plan.clone(),
                        error: Some(SharedString::from(failure.message)),
                    });
                }
            },
        );
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
        // #492: `has_active_modal` covers EVERY `ActiveModal` variant. The
        // hand-written accessor list this replaces named 18 of them, so a
        // reset-current / force-with-lease / rebase / create-tag /
        // delete-remote-branch confirmation could not stop Enter from checking
        // out the commit selected behind it. `confirm_active_modal` consumes
        // Enter first now; this stays as the second line of defence.
        if self.has_active_modal()
            || self.commit_menu.is_some()
            || self.branch_menu.is_some()
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
        let dirty = self.view().status_summary.is_dirty;

        // Prefer a local branch pointing at the commit; fall back to a
        // detached commit checkout.
        let branch = ctx_info
            .refs_here
            .iter()
            .find(|b| matches!(b.kind, BadgeKind::Branch))
            .and_then(context_ref_name);
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
