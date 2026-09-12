//! Branch operations (create/rename/delete/set-upstream/merge/tracking-checkout).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]
use crate::ui::blocking_ops::*;

use crate::ui::*;

impl KagiApp {
    /// Open the create-branch modal for the commit at `at`.
    ///
    /// The input is initially empty; the live plan will show a "name is empty"
    /// blocker until the user types a valid name.
    pub fn open_create_branch_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        // Allocate a focus handle if we don't have one yet.
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        self.set_create_branch_modal(CreateBranchModal {
            at,
            start_title,
            input: String::new(),
            input_state: None, // created lazily on first render (needs Window)
            checkout_after: false,
            plan: ModalPlan::Pending,
            error: None,
        });
        // Re-plan immediately (empty name → blocker).
        self.replan_create_branch();
    }

    pub(crate) fn commit_title_for(&self, at: &CommitId) -> String {
        self.row_for_commit_id(at)
            .and_then(|idx| self.view().details.get(idx))
            .map(|detail| {
                detail
                    .full_message
                    .as_ref()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default()
    }

    /// Close the create-branch modal without making any changes.
    pub fn cancel_create_branch_modal(&mut self) {
        self.clear_create_branch_modal();
    }

    /// Re-generate the live plan from the current modal input.
    pub(crate) fn replan_create_branch(&mut self) {
        let (at, name, checkout_after) = match self.create_branch_modal() {
            Some(m) => (m.at.clone(), m.input.clone(), m.checkout_after),
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
                klog!("replan_create_branch: repo session unavailable");
                let outcome = session_unavailable(i18n::Op::CreateBranch);
                if let Some(modal) = self.create_branch_modal_mut() {
                    modal.plan.replan(outcome);
                }
                return;
            }
        };
        let result = repo.plan_create_branch_with_checkout(&name, &at, checkout_after);
        match &result {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-branch '{}' checkout_after={} blockers={} warnings={}",
                    name,
                    checkout_after,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // ADR-0129 Phase 3: the keyed branch-name reasons are now
                // typed (`CommonNote::BranchNameErrorKeyed`) and localize
                // automatically via `plan_note_text()` — no separate
                // localized-blocker computation needed.
            }
            Err(e) => {
                klog!("plan: create-branch error: {}", e);
            }
        }
        // #510: adopt the outcome either way — a failure replaces the plan it
        // was recomputing, so neither Enter nor the button can confirm a stale one.
        let outcome = plan_outcome(i18n::Op::CreateBranch, result);
        if let Some(modal) = self.create_branch_modal_mut() {
            modal.plan.replan(outcome);
        }
    }

    /// Confirm the create-branch plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_create_branch(&mut self, cx: &mut Context<Self>) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans(cx);
        let modal = match self.create_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        // #510: `Pending` and `Failed` both yield None, so a failed replan
        // refuses Enter and the button alike.
        let Some(plan) = modal.plan.plan().cloned() else {
            return;
        };
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            klog!("refused: create-branch plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-branch",
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

        let mut repo = match crate::ui::blocking_ops::open_backend(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = i18n::op_failed(i18n::Op::RepoOpen, e);
                self.record_op(
                    "create-branch",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.create_branch_modal_mut() {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
        // in one place (run() calls preflight_check as its first line — the
        // separate preflight_check call above was redundant).
        let op = kagi_git::Operation::CreateBranchWithCheckout {
            name: modal.input.clone(),
            at: modal.at.clone(),
            checkout_after: modal.checkout_after,
        };
        let report = repo.run_recorded(&op, &plan);
        if let Err(e) = &report.result {
            let err_msg = i18n::op_failed(i18n::Op::CreateBranch, e);
            self.present_report("create-branch", &report, &repo_path, cx);
            if let Some(m) = self.create_branch_modal_mut() {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!(
            "[kagi] executed: create-branch '{}' @ {}",
            modal.input,
            modal.at.short()
        );

        // Verify: confirm the branch now exists.
        let repo2 = match crate::ui::blocking_ops::open_backend(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("verify: repo open error: {}", e);
                self.reload(cx);
                return;
            }
        };
        let branch_exists = repo2.local_branch_exists(&modal.input);
        if branch_exists {
            klog!("verified: branch '{}' exists", modal.input);
        } else {
            eprintln!(
                "[kagi] verify: branch '{}' NOT found after create",
                modal.input
            );
        }

        // The combined Backend operation already performed optional checkout and
        // persisted one receipt; present that receipt (ADR-0196 Wave 2).
        self.present_report("create-branch", &report, &repo_path, cx);
        if modal.checkout_after {
            klog!("executed: checkout {}", modal.input);
        }

        // Reload display data (new branch badge should appear).
        self.reload(cx);
    }

    pub fn open_branch_plan_modal(&mut self, branch_name: String, kind: BranchPlanKind) {
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "branch operation: repo session unavailable",
                ));
                return;
            }
        };
        let plan_result = match kind {
            BranchPlanKind::PullFfOnly => repo.plan_pull_branch_ff(&branch_name),
            BranchPlanKind::Push => repo.plan_push_branch(&branch_name, false),
            BranchPlanKind::PushSetUpstream => repo.plan_push_branch(&branch_name, true),
        };
        match plan_result {
            Ok(plan) => {
                self.set_branch_plan_modal(BranchPlanModal {
                    kind,
                    branch_name,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "branch operation plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_branch_plan_modal(&mut self) {
        self.clear_branch_plan_modal();
    }

    pub fn start_branch_plan(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.branch_plan_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = match modal.kind {
            BranchPlanKind::PullFfOnly => "branch-pull-ff",
            BranchPlanKind::Push => "branch-push",
            BranchPlanKind::PushSetUpstream => "branch-push-set-upstream",
        };
        if !modal.plan.blockers.is_empty() {
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_branch_plan_modal();
            cx.notify();
            return;
        }

        self.clear_branch_plan_modal();
        self.status_footer =
            FooterStatus::Busy(SharedString::from(format!("{} in progress...", op_name)));
        let bg_path = repo_path.clone();
        let bg_modal = modal.clone();
        let op = match modal.kind {
            BranchPlanKind::PullFfOnly => i18n::Op::Pull,
            BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => i18n::Op::Push,
        };
        self.finish_run(
            cx,
            op_name,
            op,
            modal.plan.clone(),
            repo_path,
            move || branch_plan_blocking(&bg_path, &bg_modal),
            |_| None,
            move |app, done, _cx| match done {
                Ok(outcome) => {
                    app.status_footer = FooterStatus::Success(SharedString::from(format!(
                        "{}: {}",
                        op_name,
                        branch_plan_summary(&modal.branch_name, outcome)
                    )));
                }
                Err(failure) => {
                    app.set_branch_plan_modal(BranchPlanModal {
                        kind: modal.kind.clone(),
                        branch_name: modal.branch_name.clone(),
                        plan: modal.plan.clone(),
                        error: Some(SharedString::from(failure.message)),
                    });
                }
            },
        );
    }

    pub fn open_set_upstream_modal(&mut self, branch_name: String) {
        let input = self
            .view()
            .branch_upstream_info
            .get(&branch_name)
            .map(|u| u.remote_branch.clone())
            .unwrap_or_else(|| format!("origin/{}", branch_name));
        self.set_set_upstream_modal(SetUpstreamModal {
            branch_name,
            input,
            input_state: None,
            plan: ModalPlan::Pending,
            error: None,
        });
        self.replan_set_upstream();
    }

    pub fn cancel_set_upstream_modal(&mut self) {
        self.clear_set_upstream_modal();
    }

    pub(crate) fn replan_set_upstream(&mut self) {
        let (branch_name, input) = match self.set_upstream_modal() {
            Some(m) => (m.branch_name.clone(), m.input.clone()),
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
                let outcome = session_unavailable(i18n::Op::SetUpstream);
                if let Some(m) = self.set_upstream_modal_mut() {
                    m.plan.replan(outcome);
                }
                return;
            }
        };
        // #510: the failure now lands in the plan slot, so the previous plan is
        // gone rather than merely accompanied by an error line.
        let outcome = plan_outcome(
            i18n::Op::SetUpstream,
            repo.plan_set_upstream(&branch_name, &input),
        );
        if let Some(m) = self.set_upstream_modal_mut() {
            m.plan.replan(outcome);
        }
    }

    pub fn start_set_upstream(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans(cx);
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.set_upstream_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        // #510: a failed replan leaves no plan, so Enter and the button both refuse.
        let Some(plan) = modal.plan.plan().cloned() else {
            return;
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !plan.blockers.is_empty() {
            self.record_op(
                "set-upstream",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            return;
        }

        self.clear_set_upstream_modal();
        let branch_name = modal.branch_name.clone();
        let upstream = modal.input.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        self.finish_run(
            cx,
            "set-upstream",
            i18n::Op::SetUpstream,
            plan.clone(),
            repo_path,
            move || set_upstream_blocking(&bg_path, &bg_plan, &branch_name, &upstream),
            |_| None,
            move |app, done, _cx| match done {
                Ok(_) => {}
                Err(err_msg) => {
                    app.set_set_upstream_modal(SetUpstreamModal {
                        branch_name: modal.branch_name.clone(),
                        input: modal.input.clone(),
                        input_state: None,
                        plan: ModalPlan::Ready(plan.clone()),
                        error: Some(SharedString::from(err_msg.message)),
                    });
                }
            },
        );
    }

    pub fn open_rename_branch_modal(&mut self, branch_name: String) {
        let existing: Vec<String> = self
            .view()
            .branches
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let validation = validate_branch_rename(&branch_name, &branch_name, &existing);
        self.set_rename_branch_modal(RenameBranchModal {
            old_name: branch_name.clone(),
            input: branch_name,
            input_state: None,
            validation,
            plan: ModalPlan::Pending,
            error: None,
        });
        self.replan_rename_branch();
    }

    pub fn cancel_rename_branch_modal(&mut self) {
        self.clear_rename_branch_modal();
    }

    pub(crate) fn replan_rename_branch(&mut self) {
        let (old_name, input) = match self.rename_branch_modal() {
            Some(m) => (m.old_name.clone(), m.input.clone()),
            None => return,
        };
        let existing: Vec<String> = self
            .view()
            .branches
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let validation = validate_branch_rename(&old_name, &input, &existing);
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                let outcome = session_unavailable(i18n::Op::Rename);
                if let Some(m) = self.rename_branch_modal_mut() {
                    m.validation = validation;
                    m.plan.replan(outcome);
                }
                return;
            }
        };
        // #510: the localized failure now lands in the plan slot, which drops the
        // plan it was recomputing instead of leaving it confirmable.
        let outcome = plan_outcome(i18n::Op::Rename, repo.plan_rename_branch(&old_name, &input));
        if let Some(m) = self.rename_branch_modal_mut() {
            m.validation = validation;
            m.plan.replan(outcome);
        }
    }

    pub fn start_rename_branch(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans(cx);
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.rename_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        // #510: a failed replan leaves no plan, so Enter and the button both refuse.
        let Some(plan) = modal.plan.plan().cloned() else {
            return;
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !plan.blockers.is_empty() {
            self.record_op(
                "rename-branch",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        self.clear_rename_branch_modal();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let old_name = modal.old_name.clone();
        let new_name = modal.input.clone();
        self.finish_run(
            cx,
            "rename-branch",
            i18n::Op::Rename,
            plan.clone(),
            repo_path,
            move || rename_branch_blocking(&bg_path, &bg_plan, &old_name, &new_name),
            |_| None,
            move |app, done, _cx| match done {
                Ok(_) => {}
                Err(err_msg) => {
                    app.set_rename_branch_modal(RenameBranchModal {
                        old_name: modal.old_name.clone(),
                        input: modal.input.clone(),
                        input_state: None,
                        validation: modal.validation.clone(),
                        plan: ModalPlan::Ready(plan.clone()),
                        error: Some(SharedString::from(err_msg.message)),
                    });
                }
            },
        );
    }

    pub fn open_tracking_checkout_modal(&mut self, remote_branch: String) {
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "checkout tracking: repo session unavailable",
                ));
                return;
            }
        };
        let local_branch = default_tracking_branch_name(&remote_branch);
        match repo.plan_checkout_tracking_branch(&remote_branch, &local_branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout-tracking {} -> {} blockers={} warnings={}",
                    remote_branch,
                    local_branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_tracking_checkout_modal(TrackingCheckoutPlanModal {
                    remote_branch,
                    local_branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "checkout tracking plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_tracking_checkout_modal(&mut self) {
        self.clear_tracking_checkout_modal();
    }

    pub fn start_tracking_checkout(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.tracking_checkout_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: checkout-tracking plan has blockers, not executing");
            self.record_op(
                "checkout-tracking",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_tracking_checkout_modal();
            cx.notify();
            return;
        }

        self.clear_tracking_checkout_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        klog!("async: checkout-tracking started");

        let plan = modal.plan.clone();
        let remote_branch = modal.remote_branch.clone();
        let local_branch = modal.local_branch.clone();
        let bg_path = repo_path.clone();
        let note_branch = modal.local_branch.clone();
        self.finish_run(
            cx,
            "checkout-tracking",
            i18n::Op::CheckoutTracking,
            modal.plan.clone(),
            repo_path,
            move || checkout_tracking_blocking(&bg_path, &plan, &remote_branch, &local_branch),
            move |_| Some(format!(" — checkout {}", note_branch)),
            move |app, done, _cx| match done {
                Ok(_) => {}
                Err(err_msg) => {
                    app.set_tracking_checkout_modal(TrackingCheckoutPlanModal {
                        remote_branch: modal.remote_branch.clone(),
                        local_branch: modal.local_branch.clone(),
                        plan: modal.plan.clone(),
                        error: Some(SharedString::from(err_msg.message)),
                    });
                }
            },
        );
    }

    /// Build a "switch to latest" plan (ADR-0101) and open the confirmation modal.
    pub fn open_switch_to_latest_modal(&mut self, branch_name: String, remote_branch: String) {
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
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "switch-to-latest: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_switch_to_latest(&branch_name, &remote_branch) {
            Ok(plan) => {
                klog!(
                    "plan: switch-to-latest {} <- {} blockers={} warnings={}",
                    branch_name,
                    remote_branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_switch_to_latest_modal(SwitchToLatestPlanModal {
                    branch_name,
                    remote_branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "switch-to-latest plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_switch_to_latest_modal(&mut self) {
        self.clear_switch_to_latest_modal();
    }

    pub fn start_switch_to_latest(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.switch_to_latest_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: switch-to-latest plan has blockers, not executing");
            self.record_op(
                "switch-to-latest",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_switch_to_latest_modal();
            cx.notify();
            return;
        }

        self.clear_switch_to_latest_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusySwitchToLatest.t()));
        klog!("async: switch-to-latest started");

        let plan = modal.plan.clone();
        let branch_name = modal.branch_name.clone();
        let remote_branch = modal.remote_branch.clone();
        let bg_path = repo_path.clone();
        let note_branch = modal.branch_name.clone();
        self.finish_run(
            cx,
            "switch-to-latest",
            i18n::Op::SwitchToLatest,
            modal.plan.clone(),
            repo_path,
            move || switch_to_latest_blocking(&bg_path, &plan, &branch_name, &remote_branch),
            move |_| Some(format!(" — switch {}", note_branch)),
            move |app, done, _cx| match done {
                Ok(_) => {}
                Err(err_msg) => {
                    app.set_switch_to_latest_modal(SwitchToLatestPlanModal {
                        branch_name: modal.branch_name.clone(),
                        remote_branch: modal.remote_branch.clone(),
                        plan: modal.plan.clone(),
                        error: Some(SharedString::from(err_msg.message)),
                    });
                }
            },
        );
    }

    /// Double-click a remote-branch pill → switch to its latest.
    ///
    /// Resolves the local tracking name from `remote_branch` (e.g.
    /// `origin/feature` → `feature`), plans the switch via
    /// [`open_switch_to_latest_modal`](Self::open_switch_to_latest_modal), and —
    /// when the plan is completely clean (no blockers **and** no warnings) —
    /// runs it immediately with no popup. `start_switch_to_latest` consumes the
    /// modal before any render, so nothing flashes on screen. Blockers/warnings
    /// leave the modal open for review (e.g. a dirty tree blocks; a local branch
    /// ahead of its remote warns that the switch can't fast-forward).
    pub fn dblclick_switch_to_latest(
        &mut self,
        remote_branch: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let remote_branch = remote_branch.into();
        let branch_name = default_tracking_branch_name(&remote_branch);
        self.open_switch_to_latest_modal(branch_name, remote_branch);
        // `open_switch_to_latest_modal` only sets the modal on a successful plan
        // (and bails when busy); treat a missing modal as "not clean".
        let clean = self
            .switch_to_latest_modal()
            .map(|m| m.plan.blockers.is_empty() && m.plan.warnings.is_empty())
            .unwrap_or(false);
        if clean {
            klog!("dblclick switch-to-latest: clean, no modal");
            self.start_switch_to_latest(cx);
        }
    }

    /// Build a delete-branch plan for `branch_name` and open the confirmation modal.
    ///
    /// The plan is built **off the UI thread** (same shape as
    /// [`open_merge_modal`]): on an unmerged branch `plan_delete_branch` runs
    /// the squash-merge patch-id probe (ADR-0138), which walks up to
    /// `SQUASH_SCAN_LIMIT` commits computing tree diffs — tens to hundreds of
    /// ms on a repo with an old fork point, i.e. a visible freeze if done here.
    ///
    /// The modal is created only from the finished plan, so there is no window
    /// in which a plan missing the probe is on screen and confirmable. See
    /// ADR-0141.
    pub fn open_delete_branch_modal(
        &mut self,
        branch_name: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let branch_name = branch_name.into();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if self.repo_path.is_none() {
            klog!("open_delete_branch_modal: no repo_path set");
            return;
        }
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
            .filter(|owner| owner.worktree.is_some())
        else {
            return;
        };
        let generation = self.switch_generation;
        let repo_path = owner.path.clone();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDeleteBranchPlan.t()));
        klog!("async: delete-branch plan started for {}", branch_name);

        let bg_path = repo_path.clone();
        let bg_branch = branch_name.clone();
        let expected_worktree = owner.worktree.clone();
        let task = cx.background_spawn(async move {
            let repo = crate::ui::blocking_ops::open_backend(&bg_path)
                .map_err(|e| format!("repo open error: {e}"))?;
            if repo.write_worktree_id().ok() != expected_worktree {
                return Err("worktree identity changed; reopen the repository".into());
            }
            repo.plan_delete_branch(&bg_branch)
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.fallible().await;
            let _ = this.update(acx, |app, cx| {
                let current = app
                    .active_session()
                    .and_then(|id| app.app_sessions.attachment(id));
                // Terminalize even stale/failed tasks before the display guard.
                if !DeleteBranchModal::settle_plan(
                    &owner,
                    current.as_ref(),
                    app.switch_generation == generation,
                    &mut app.busy_op,
                ) {
                    cx.notify();
                    return;
                }
                let result =
                    result.unwrap_or_else(|| Err("delete-branch plan failed unexpectedly".into()));
                match result {
                    Ok(plan) => {
                        eprintln!(
                            "[kagi] plan: delete-branch {} blockers={}",
                            branch_name,
                            plan.blockers.len()
                        );
                        app.status_footer = FooterStatus::Idle(SharedString::from(""));
                        app.set_delete_branch_modal(DeleteBranchModal {
                            owner,
                            confirm_armed: false,
                            branch_name,
                            plan: std::sync::Arc::new(plan),
                            error: None,
                        });
                    }
                    Err(e) => {
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "delete-branch plan error: {}",
                            e
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn cancel_delete_branch_modal(&mut self) {
        self.clear_delete_branch_modal();
    }

    /// W15-ASYNCOPS: UI-path delete-branch — background thread + start/finish
    /// toasts (ref delete is lightweight, but kept on the background path for a
    /// uniform busy/disabled experience). #493: the single delete-branch entry —
    /// the modal button and the root Enter dispatch both land here.
    pub fn start_delete_branch(&mut self, cx: &mut Context<Self>) {
        let mut modal = match self.delete_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
            .as_ref()
            != Some(&modal.owner)
        {
            self.clear_delete_branch_modal();
            cx.notify();
            return;
        }
        let owner = modal.owner.clone();
        let repo_path = owner.path.clone();
        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: delete-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.iter().map(|b| b.message_en()).collect(),
                },
                &repo_path,
                cx,
            );
            self.clear_delete_branch_modal();
            cx.notify();
            return;
        }

        if modal.arm_if_required() {
            self.reset_modal_sections();
            self.set_delete_branch_modal(modal);
            klog!("delete-branch: armed (second confirm required — unmerged)");
            cx.notify();
            return;
        }

        self.clear_delete_branch_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDeleteBranch.t()));
        klog!("async: delete-branch started");

        let plan = modal.plan.clone();
        let branch_name = modal.branch_name.clone();
        let bg_owner = owner.clone();
        let bg_plan = plan.clone();
        let bg_branch = branch_name.clone();
        let task =
            cx.background_spawn(
                async move { delete_branch_blocking(&bg_owner, &bg_plan, &bg_branch) },
            );
        let notice_path = repo_path.clone();
        self.finish_op_on_main_settled(
            cx,
            task,
            move |app, result: &Result<kagi_git::backend::recording::RunReport, String>, _cx| {
                if let Ok(report) = result {
                    app.notice_recording_failure("delete-branch", &report.recording, &notice_path);
                }
            },
            move |app, result, cx| {
                let result = match result {
                    Ok(report) => {
                        if report.result.is_ok() {
                            klog!("async: delete-branch finished");
                            // Worktree-removal plans (clean worktree pinned the branch)
                            // log the cleanup so the headless harness can assert it.
                            // ADR-0129 F-3: matched via the typed note variant, not a
                            // substring search over the rendered EN text.
                            if plan.warnings.iter().any(|w| {
                                matches!(
                        w,
                        kagi_git::ops::PlanNote::Branch(
                            kagi_domain::plan_note::BranchNote::DeleteRemovesPinningWorktree { .. }
                        )
                    )
                            }) {
                                klog!("executed: delete-branch removed pinning worktree");
                            }
                        }
                        if matches!(
                            report.recording.entry().outcome,
                            kagi_git::oplog::OpOutcome::Partial { .. }
                        ) {
                            if let Err(error) = &report.result {
                                let err_msg = i18n::op_failed(i18n::Op::Delete, error);
                                klog!("async: delete-branch failed — {}", err_msg);
                            }
                            app.present_recorded(&report.recording, cx);
                            app.reload(cx);
                            return;
                        }
                        if report.result.is_ok()
                            && matches!(
                                report.recording,
                                kagi_git::backend::recording::Recording::Failed { .. }
                            )
                        {
                            app.present_recorded(&report.recording, cx);
                            app.reload(cx);
                            return;
                        }
                        report
                            .result
                            .map_err(|e| i18n::op_failed(i18n::Op::Delete, e))
                            .and_then(|outcome| {
                                let kagi_git::OperationOutcome::DeleteBranch { reference, .. } =
                                    outcome
                                else {
                                    return Err("unexpected delete-branch outcome".into());
                                };
                                let recovery = format!("git branch {} {reference}", branch_name);
                                if !matches!(
                                    report.recording.entry().outcome,
                                    kagi_git::oplog::OpOutcome::Success { .. }
                                ) {
                                    return Err("unexpected delete-branch receipt".into());
                                }
                                Ok((report.recording, recovery))
                            })
                    }
                    Err(error) => Err(error),
                };
                match result {
                    Ok((recording, recovery_line)) => {
                        app.present_recorded(&recording, cx);
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "delete-branch: '{}' deleted (restore: {})",
                            branch_name, recovery_line
                        )));
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        klog!("async: delete-branch failed — {}", err_msg);
                        app.record_op(
                            "delete-branch",
                            plan.current.clone(),
                            kagi_git::oplog::OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_delete_branch_modal(DeleteBranchModal {
                            owner,
                            confirm_armed: false,
                            branch_name: branch_name.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
            },
        );
    }
}

// `dispatch_branch_action`, moved from `src/ui/mod.rs` (T-HOTSPOT-UIMOD-001).
// Behaviour-preserving relocation.
impl KagiApp {
    pub fn dispatch_branch_action(
        &mut self,
        action: BranchAction,
        state: BranchMenuState,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            BranchAction::CopyBranchName => {
                branch_menu::copy_branch_name(self, state.name, cx);
            }
            BranchAction::CopyHeadSha => {
                branch_menu::copy_head_sha(self, state.target.0, cx);
            }
            BranchAction::CopyUpstreamName => {
                let upstream = self
                    .view()
                    .branch_upstream_info
                    .get(&state.name)
                    .map(|u| u.remote_branch.clone());
                if let Some(upstream) = upstream {
                    branch_menu::copy_upstream_name(self, upstream, cx);
                }
            }
            BranchAction::RevealHead => {
                self.jump_to_commit(&state.target);
            }
            BranchAction::ToggleSolo => {
                self.toggle_branch_solo(state.name, state.target, cx);
            }
            BranchAction::Checkout => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_plan_modal(state.name);
                } else {
                    self.open_tracking_checkout_modal(state.name);
                }
            }
            BranchAction::SwitchToLatest => {
                let (branch_name, remote_branch) = if matches!(state.kind, BranchKind::Local) {
                    let upstream = self
                        .view()
                        .branch_upstream_info
                        .get(&state.name)
                        .map(|u| u.remote_branch.clone());
                    (state.name.clone(), upstream)
                } else {
                    (
                        default_tracking_branch_name(&state.name),
                        Some(state.name.clone()),
                    )
                };
                match remote_branch {
                    Some(remote_branch) => {
                        self.open_switch_to_latest_modal(branch_name, remote_branch);
                    }
                    None => {
                        self.status_footer =
                            FooterStatus::Idle(SharedString::from(Msg::BcmNoUpstream.t()));
                    }
                }
            }
            BranchAction::CreateBranchFromHere => {
                self.open_create_branch_modal(state.target, cx);
            }
            BranchAction::DeleteBranch => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_delete_branch_modal(state.name, cx);
                }
            }
            BranchAction::Pull => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .view()
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_pull_modal(cx);
                    } else {
                        self.open_branch_plan_modal(state.name, BranchPlanKind::PullFfOnly);
                    }
                }
            }
            BranchAction::Push => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .view()
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_push_modal(cx);
                    } else {
                        self.open_branch_plan_modal(state.name, BranchPlanKind::Push);
                    }
                }
            }
            BranchAction::PushAndCreateUpstream => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_branch_plan_modal(state.name, BranchPlanKind::PushSetUpstream);
                }
            }
            BranchAction::SetUpstream => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_set_upstream_modal(state.name);
                }
            }
            BranchAction::RenameBranch => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_rename_branch_modal(state.name);
                }
            }
            BranchAction::OpenWorktreeFromBranch => {
                let existing_path = self
                    .view()
                    .worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(state.name.as_str()))
                    .map(|wt| wt.path.display().to_string());
                if let Some(path) = existing_path {
                    self.status_footer = FooterStatus::Idle(SharedString::from(format!(
                        "worktree already exists: {}",
                        path
                    )));
                    self.push_toast(ToastKind::Info, format!("Worktree: {}", path), cx);
                } else if matches!(state.kind, BranchKind::Local) {
                    self.open_create_worktree_modal_prefilled(state.target, state.name, true, cx);
                }
            }
            // #473: open the worktree this branch is already checked out in.
            BranchAction::OpenWorktreeDir => {
                let path = self
                    .view()
                    .worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(state.name.as_str()) && !wt.is_current)
                    .map(|wt| wt.path.clone());
                if let Some(path) = path {
                    self.open_repository(path, cx);
                }
            }
            BranchAction::MergeIntoCurrent => {
                self.open_merge_modal(state.name, None, cx);
            }
            BranchAction::CreateWorktreeFromHere => {
                self.open_create_worktree_modal_prefilled(state.target, state.name, false, cx);
            }
            BranchAction::CreateTagHere => {
                self.open_create_tag_modal(state.target, cx);
            }
            BranchAction::PullFfOnly => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_branch_plan_modal(state.name, BranchPlanKind::PullFfOnly);
                }
            }
            BranchAction::DeleteRemoteBranch => {
                if matches!(state.kind, BranchKind::Remote) {
                    self.open_delete_remote_branch_modal(state.name);
                }
            }
            BranchAction::FetchRemoteBranch => {
                if matches!(state.kind, BranchKind::Remote) {
                    self.fetch_remote_branch_async(state.name, cx);
                }
            }
            BranchAction::CreatePr => {
                self.open_create_pr(state.name, state.kind, cx);
            }
            BranchAction::ResetCurrentToHead => {
                self.open_reset_current_modal(state.target, cx);
            }
            BranchAction::ForceWithLeasePush => {
                let is_current = self
                    .view()
                    .branches
                    .iter()
                    .any(|(name, current)| name == &state.name && *current);
                if is_current {
                    self.open_force_lease_push_modal(cx);
                }
            }
            BranchAction::RebaseCurrentOnto => {
                self.open_rebase_modal(state.name, cx);
            }
            BranchAction::NoUpstreamInfo => {
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::BcmNotImplementedYet.t()));
            }
        }
    }
}
