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
            plan: None,
            error: None,
            localized_blockers: Vec::new(),
        });
        // Re-plan immediately (empty name → blocker).
        self.replan_create_branch();
    }

    pub(crate) fn commit_title_for(&self, at: &CommitId) -> String {
        self.row_for_commit_id(at)
            .and_then(|idx| self.active_view.details.get(idx))
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
                return;
            }
        };
        match repo.plan_create_branch_with_checkout(&name, &at, checkout_after) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-branch '{}' checkout_after={} blockers={} warnings={}",
                    name,
                    checkout_after,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // W29-I18N-WAVE2: localize the keyed branch-name reasons; any
                // non-keyed plan blocker (commit-existence, checkout-after) is
                // passed through in English.
                let keyed = repo.create_branch_name_errors(&name);
                let localized = localize_plan_blockers(
                    &plan.blockers,
                    keyed
                        .iter()
                        .map(|e| (e.to_string(), crate::ui::i18n::branch_name_error(e))),
                );
                if let Some(modal) = self.create_branch_modal_mut() {
                    modal.plan = Some(std::sync::Arc::new(plan));
                    modal.localized_blockers = localized;
                }
            }
            Err(e) => {
                klog!("plan: create-branch error: {}", e);
            }
        }
    }

    /// Confirm the create-branch plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_create_branch(&mut self, cx: &mut Context<Self>) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        let modal = match self.create_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            klog!("refused: create-branch plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-branch",
                    plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
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
        let op = kagi_git::Operation::CreateBranch {
            name: modal.input.clone(),
            at: modal.at.clone(),
        };
        if let Err(e) = repo.run(&op, &plan) {
            let err_msg = format!("Create branch failed: {}", e);
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

        eprintln!(
            "[kagi] executed: create-branch '{}' @ {}",
            modal.input,
            modal.at.short()
        );

        // Verify: confirm the branch now exists.
        let mut repo2 = match kagi_git::Backend::open(&repo_path) {
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

        // Record branch creation success first. If checkout_after is on, the
        // checkout below records its own second operation entry.
        let create_after = StateSummary {
            head: plan.current.head.clone(),
            dirty: plan.current.dirty.clone(),
        };
        self.record_op(
            "create-branch",
            plan.current.clone(),
            OpOutcome::Success {
                after: create_after.clone(),
            },
            &repo_path,
            cx,
        );

        if modal.checkout_after {
            let checkout_plan = match repo2.plan_checkout(&modal.input) {
                Ok(plan) => plan,
                Err(e) => {
                    let err_msg = format!("Checkout plan failed after branch creation: {}", e);
                    self.record_op(
                        "checkout",
                        create_after,
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
            if !checkout_plan.blockers.is_empty() {
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: checkout_plan.blockers.clone(),
                    },
                    &repo_path,
                    cx,
                );
                if let Some(m) = self.create_branch_modal_mut() {
                    m.error = Some(SharedString::from(
                        "Branch created, but checkout was refused by the checkout plan.",
                    ));
                }
                return;
            }
            // ADR-0104 Phase 2: route through Backend::run so preflight is
            // enforced in one place (the separate preflight_check + execute
            // above collapses into run()).
            let checkout_op = kagi_git::Operation::Checkout {
                branch: modal.input.clone(),
            };
            if let Err(e) = repo2.run(&checkout_op, &checkout_plan) {
                let err_msg = format!("Checkout failed: {}", e);
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
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
            klog!("executed: checkout {}", modal.input);
            self.record_op(
                "checkout",
                checkout_plan.current.clone(),
                OpOutcome::Success {
                    after: checkout_plan.predicted.clone(),
                },
                &repo_path,
                cx,
            );
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
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_branch_plan_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some(op_name);
        self.clear_branch_plan_modal();
        self.status_footer =
            FooterStatus::Busy(SharedString::from(format!("{} in progress...", op_name)));
        let bg_path = repo_path.clone();
        let bg_modal = modal.clone();
        let task = cx.background_spawn(async move { branch_plan_blocking(&bg_path, &bg_modal) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            op_name,
                            modal.plan.current.clone(),
                            OpOutcome::Success {
                                after: after.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "{}: {}",
                            op_name, after.dirty
                        )));
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        app.record_op(
                            op_name,
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_branch_plan_modal(BranchPlanModal {
                            kind: modal.kind.clone(),
                            branch_name: modal.branch_name.clone(),
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
    }

    pub fn open_set_upstream_modal(&mut self, branch_name: String) {
        let input = self
            .active_view
            .branch_upstream_info
            .get(&branch_name)
            .map(|u| u.remote_branch.clone())
            .unwrap_or_else(|| format!("origin/{}", branch_name));
        self.set_set_upstream_modal(SetUpstreamModal {
            branch_name,
            input,
            input_state: None,
            plan: None,
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
            None => return,
        };
        match repo.plan_set_upstream(&branch_name, &input) {
            Ok(plan) => {
                if let Some(m) = self.set_upstream_modal_mut() {
                    m.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                if let Some(m) = self.set_upstream_modal_mut() {
                    m.error = Some(SharedString::from(format!(
                        "Set upstream plan error: {}",
                        e
                    )));
                }
            }
        }
    }

    pub fn start_set_upstream(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.set_upstream_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.clone() {
            Some(p) => p,
            None => return,
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
                    blockers: plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }

        self.busy_op = Some("set-upstream");
        self.clear_set_upstream_modal();
        let branch_name = modal.branch_name.clone();
        let upstream = modal.input.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx.background_spawn(async move {
            set_upstream_blocking(&bg_path, &bg_plan, &branch_name, &upstream)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            "set-upstream",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        app.record_op(
                            "set-upstream",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_set_upstream_modal(SetUpstreamModal {
                            branch_name: modal.branch_name.clone(),
                            input: modal.input.clone(),
                            input_state: None,
                            plan: Some(plan.clone()),
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

    pub fn open_rename_branch_modal(&mut self, branch_name: String) {
        let existing: Vec<String> = self
            .active_view
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
            plan: None,
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
            .active_view
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
            None => return,
        };
        match repo.plan_rename_branch(&old_name, &input) {
            Ok(plan) => {
                if let Some(m) = self.rename_branch_modal_mut() {
                    m.validation = validation;
                    m.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                if let Some(m) = self.rename_branch_modal_mut() {
                    m.validation = validation;
                    m.error = Some(SharedString::from(format!("Rename plan error: {}", e)));
                }
            }
        }
    }

    pub fn start_rename_branch(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.rename_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.clone() {
            Some(p) => p,
            None => return,
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
                    blockers: plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }
        self.busy_op = Some("rename-branch");
        self.clear_rename_branch_modal();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let old_name = modal.old_name.clone();
        let new_name = modal.input.clone();
        let task = cx.background_spawn(async move {
            rename_branch_blocking(&bg_path, &bg_plan, &old_name, &new_name)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            "rename-branch",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        app.record_op(
                            "rename-branch",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_rename_branch_modal(RenameBranchModal {
                            old_name: modal.old_name.clone(),
                            input: modal.input.clone(),
                            input_state: None,
                            validation: modal.validation.clone(),
                            plan: Some(plan.clone()),
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

    pub fn open_merge_modal(&mut self, target: String, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Current (checked-out) branch = the merge destination, captured on the
        // main thread for the modal's into-branch label (ADR-0079).
        let into_branch = self
            .active_view
            .branches
            .iter()
            .find(|(_, is_head)| *is_head)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "HEAD".to_string());

        // Planning a merge runs an in-memory merge (conflict dry-run) which is
        // heavy on large repos — do it off the UI thread so the window doesn't
        // freeze. `busy_op` drives the spinning sync icon + blocks re-entry.
        self.busy_op = Some("merge-plan");
        self.status_footer = FooterStatus::Busy(SharedString::from("Planning merge…"));
        klog!("async: merge plan started for {}", target);
        let bg_path = repo_path.clone();
        let bg_target = target.clone();
        let task = cx.background_spawn(async move {
            let repo =
                kagi_git::Backend::open(&bg_path).map_err(|e| format!("repo open error: {e}"))?;
            repo.plan_merge_branch(&bg_target)
                .map_err(|e| format!("{e}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((plan, kind)) => {
                        eprintln!(
                            "[kagi] plan: merge {} blockers={} warnings={} preview_files={} kind={:?}",
                            target,
                            plan.blockers.len(),
                            plan.warnings.len(),
                            plan.preview_files.len(),
                            kind
                        );
                        app.status_footer = FooterStatus::Idle(SharedString::from(""));
                        app.set_merge_modal(MergePlanModal {
                            target,
                            into_branch,
                            plan: std::sync::Arc::new(plan),
                            kind,
                            error: None,
                        });
                    }
                    Err(e) => {
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "merge plan error: {}",
                            e
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn cancel_merge_modal(&mut self) {
        self.clear_merge_modal();
    }

    /// T-DNDMERGE-001 / ADR-0079 layer 2: the single entry point a branch
    /// drag-and-drop dispatches to.  `source` is the dragged branch (the merge
    /// source = the branch merged INTO HEAD) — a local branch name, or a
    /// remote-tracking ref like `origin/feature` for an upstream-only branch,
    /// which the planner resolves directly (no local branch is created).  This
    /// validates the obvious rejections (busy / not a branch / dropping the
    /// current branch onto itself) and, on success, delegates to the merge
    /// pipeline via
    /// [`open_merge_modal`] — it never touches git directly (the safety
    /// thesis: drop is a trigger; `plan_merge_branch` remains authoritative for
    /// dirty-WT / ff / conflict prediction).
    pub fn start_merge_from_drag(&mut self, source: String, cx: &mut Context<Self>) {
        let remotes: Vec<String> = self
            .active_view
            .remote_branches
            .iter()
            .map(|rb| format!("{}/{}", rb.remote, rb.name))
            .collect();
        match validate_merge_from_drag(
            &source,
            &self.active_view.branches,
            &remotes,
            self.busy_op.is_some(),
        ) {
            Ok(()) => {
                eprintln!(
                    "[kagi] drag-merge: start merge from drag — source={}",
                    source
                );
                self.open_merge_modal(source, cx);
            }
            Err(reason) => {
                klog!("drag-merge: rejected — {}", reason);
                self.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
        }
        cx.notify();
    }

    pub fn start_merge(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.merge_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: merge plan has blockers, not executing");
            self.record_op(
                "merge",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_merge_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("merge");
        self.clear_merge_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyMerge.t()));
        klog!("async: merge started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let kind = modal.kind.clone();
        let bg_path = repo_path.clone();
        let history_target = modal.target.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the merge (main thread).
        let history_before = self.head_branch_and_sha();
        let task =
            cx.background_spawn(async move { merge_blocking(&bg_path, &plan, &target, &kind) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: merge finished — {}", summary);
                        app.record_op(
                            "merge",
                            modal.plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
                        // Record for undo/redo only when the merge actually moved
                        // the branch ref (clean merge / fast-forward). A merge
                        // left in conflict has not moved HEAD, so before==after
                        // and record_history is a no-op.
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi_git::OperationKind::Merge,
                                &branch,
                                before,
                                after_sha,
                                format!("merge {}", history_target),
                            );
                        }
                        // reload() resets the conflict-mode detection guard and
                        // re-runs detect_conflict_mode(); a merge that left
                        // conflict markers (MergeKind::Conflicts) therefore enters
                        // Conflict Mode here. Non-conflict merges stay Normal.
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        klog!("async: merge failed — {}", err_msg);
                        app.record_op(
                            "merge",
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_merge_modal(MergePlanModal {
                            target: modal.target.clone(),
                            into_branch: modal.into_branch.clone(),
                            plan: modal.plan.clone(),
                            kind: modal.kind.clone(),
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
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_tracking_checkout_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("checkout");
        self.clear_tracking_checkout_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        klog!("async: checkout-tracking started");

        let plan = modal.plan.clone();
        let remote_branch = modal.remote_branch.clone();
        let local_branch = modal.local_branch.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            checkout_tracking_blocking(&bg_path, &plan, &remote_branch, &local_branch)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: checkout-tracking finished — {}", summary);
                        app.record_op(
                            "checkout-tracking",
                            modal.plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        klog!("async: checkout-tracking failed — {}", err_msg);
                        app.record_op(
                            "checkout-tracking",
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_tracking_checkout_modal(TrackingCheckoutPlanModal {
                            remote_branch: modal.remote_branch.clone(),
                            local_branch: modal.local_branch.clone(),
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
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_switch_to_latest_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("switch");
        self.clear_switch_to_latest_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusySwitchToLatest.t()));
        klog!("async: switch-to-latest started");

        let plan = modal.plan.clone();
        let branch_name = modal.branch_name.clone();
        let remote_branch = modal.remote_branch.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            switch_to_latest_blocking(&bg_path, &plan, &branch_name, &remote_branch)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: switch-to-latest finished — {}", summary);
                        app.record_op(
                            "switch-to-latest",
                            modal.plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
                        app.reload(cx);
                    }
                    Err(err_msg) => {
                        klog!("async: switch-to-latest failed — {}", err_msg);
                        app.record_op(
                            "switch-to-latest",
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        app.set_switch_to_latest_modal(SwitchToLatestPlanModal {
                            branch_name: modal.branch_name.clone(),
                            remote_branch: modal.remote_branch.clone(),
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
    pub fn open_delete_branch_modal(&mut self, branch_name: impl Into<String>) {
        let branch_name = branch_name.into();
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                klog!("open_delete_branch_modal: no repo_path set");
                return;
            }
        };
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    "delete-branch: repo session unavailable",
                ));
                return;
            }
        };
        match repo.plan_delete_branch(&branch_name) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: delete-branch {} blockers={}",
                    branch_name,
                    plan.blockers.len()
                );
                self.set_delete_branch_modal(DeleteBranchModal {
                    branch_name,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "delete-branch plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_delete_branch_modal(&mut self) {
        self.clear_delete_branch_modal();
    }

    /// Confirm delete-branch: preflight → execute → oplog → reload.
    pub fn confirm_delete_branch(&mut self, cx: &mut Context<Self>) {
        let modal = match self.delete_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: delete-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            return;
        }

        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.set_delete_branch_modal(DeleteBranchModal {
                    branch_name: modal.branch_name.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };

        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
        // in one place (run() calls preflight_check as its first line — the
        // separate preflight_check call above was redundant).
        let del_op = kagi_git::Operation::DeleteBranch {
            name: modal.branch_name.clone(),
        };
        match repo.run(&del_op, &modal.plan) {
            Ok(_) => {
                klog!("executed: delete-branch {}", modal.branch_name);
                self.clear_delete_branch_modal();
                let after = kagi_git::ops::StateSummary {
                    head: modal.plan.current.head.clone(),
                    dirty: format!("branch '{}' deleted", modal.branch_name),
                };
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "delete-branch: '{}' deleted (restore: {})",
                    modal.branch_name,
                    modal.plan.recovery.lines().nth(1).unwrap_or("git branch …")
                )));
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = format!("Delete failed: {}", e);
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi_git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
                );
                self.set_delete_branch_modal(DeleteBranchModal {
                    branch_name: modal.branch_name.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// W15-ASYNCOPS: UI-path delete-branch — background thread + start/finish
    /// toasts (ref delete is lightweight, but kept on the background path for a
    /// uniform busy/disabled experience). Headless keeps `confirm_delete_branch`.
    pub fn start_delete_branch(&mut self, cx: &mut Context<Self>) {
        let modal = match self.delete_branch_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: delete-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi_git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
                cx,
            );
            self.clear_delete_branch_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("delete-branch");
        self.clear_delete_branch_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDeleteBranch.t()));
        klog!("async: delete-branch started");

        let plan = modal.plan.clone();
        let branch_name = modal.branch_name.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_branch = branch_name.clone();
        let task =
            cx.background_spawn(
                async move { delete_branch_blocking(&bg_path, &bg_plan, &bg_branch) },
            );
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        klog!("async: delete-branch finished");
                        let recovery_line = plan
                            .recovery
                            .lines()
                            .nth(1)
                            .unwrap_or("git branch …")
                            .to_string();
                        app.record_op(
                            "delete-branch",
                            plan.current.clone(),
                            kagi_git::oplog::OpOutcome::Success { after },
                            &repo_path,
                            cx,
                        );
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
                            branch_name: branch_name.clone(),
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
                    .active_view
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
                        .active_view
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
                    self.open_delete_branch_modal(state.name);
                }
            }
            BranchAction::Pull => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .active_view
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
                        .active_view
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
                    .active_view
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
            BranchAction::MergeIntoCurrent => {
                self.open_merge_modal(state.name, cx);
            }
            BranchAction::CreateWorktreeFromHere => {
                self.open_create_worktree_modal_prefilled(state.target, state.name, false, cx);
            }
            BranchAction::NoUpstreamInfo
            | BranchAction::PullFfOnly
            | BranchAction::FetchRemoteBranch
            | BranchAction::CreatePr
            | BranchAction::RebaseCurrentOnto
            | BranchAction::CreateTagHere
            | BranchAction::ResetCurrentToHead
            | BranchAction::ForceWithLeasePush
            | BranchAction::DeleteRemoteBranch => {
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::BcmNotImplementedYet.t()));
            }
        }
    }
}
