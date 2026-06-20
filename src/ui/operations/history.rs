//! History operations (undo/redo/amend + reflog seeding).
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Build an undo-commit plan and open the confirmation modal.
    pub fn open_undo_modal(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "undo: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_undo_commit() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: undo blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_undo_modal(UndoPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("undo plan error: {}", e)));
            }
        }
    }

    pub fn cancel_undo_modal(&mut self) {
        self.clear_undo_modal();
    }

    /// Confirm undo: preflight → execute (ref-only) → oplog → reload.
    pub fn confirm_undo(&mut self) {
        let modal = match self.undo_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            klog!("refused: undo plan has blockers, not executing");
            self.record_op(
                "undo-commit",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_undo_modal(UndoPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "undo-commit",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.set_undo_modal(UndoPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }
        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
        let undo_op = kagi::git::Operation::UndoCommit;
        match repo.run(&undo_op, &modal.plan) {
            Ok(kagi::git::OperationOutcome::Undo(outcome)) => {
                eprintln!(
                    "[kagi] executed: undo {} -> now at {}",
                    outcome.undone.short(),
                    outcome.now_at.short()
                );
                self.clear_undo_modal();
                let after = StateSummary {
                    head: format!("branch @ {}", outcome.now_at.short()),
                    dirty: "changes staged".to_string(),
                };
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // T-UNDOREDO-001: record so the undo-commit itself is redoable
                // (entry.before = undone commit, entry.after = parent). An undo
                // of THIS entry re-applies the commit; a redo undoes it again.
                if let Some((branch, _)) = self.head_branch_and_sha() {
                    self.record_history(
                        kagi::git::OperationKind::UndoCommit,
                        &branch,
                        outcome.undone.clone(),
                        outcome.now_at.clone(),
                        format!("undo-commit {}", outcome.undone.short()),
                    );
                }
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "undo: {} (restore: git reset --soft {})",
                    outcome.undone.short(),
                    outcome.undone.short()
                )));
                self.reload();
            }
            Ok(_) => {
                // UndoCommit only yields OperationOutcome::Undo.
                klog!("undo: unexpected outcome variant");
                return;
            }
            Err(e) => {
                let err_msg = format!("Undo failed: {}", e);
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_undo_modal(UndoPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// ADR-0084: hydrate the in-session [`OperationHistory`] from the current
    /// branch's reflog when it is **empty** (freshly-opened repo, or after a
    /// branch switch which clears the per-repo stack). This makes Cmd+Z work
    /// immediately, even on operations performed outside this session.
    ///
    /// Only seeds when empty — an in-session stack (with precise summaries) is
    /// never clobbered. Reflog read failures are logged and ignored (best-effort).
    pub(crate) fn seed_history_from_reflog(&mut self, backend: &kagi::git::Backend) {
        if self.operation_history.len() != 0 {
            return;
        }
        match backend.history_from_reflog() {
            Ok(entries) => {
                if !entries.is_empty() {
                    eprintln!(
                        "[kagi] history: seeded {} entries from reflog",
                        entries.len()
                    );
                    self.operation_history = kagi::git::OperationHistory::seeded(entries);
                }
            }
            Err(e) => {
                klog!("history: reflog seed failed: {}", e);
            }
        }
    }

    /// Record a successful ref-moving operation into the in-session
    /// [`OperationHistory`]. `before`/`after` are the branch tip SHAs around the
    /// operation; recording truncates any redo tail (standard undo-stack).
    ///
    /// No-op when the SHAs are identical (e.g. a no-op fast-forward) or when the
    /// branch name is empty (detached HEAD ops are not undoable in MVP).
    pub fn record_history(
        &mut self,
        kind: kagi::git::OperationKind,
        branch: &str,
        before: kagi::git::CommitId,
        after: kagi::git::CommitId,
        summary: impl Into<String>,
    ) {
        if branch.is_empty() || before == after {
            return;
        }
        let summary = summary.into();
        eprintln!(
            "[kagi] history: record {} on '{}' {} → {}",
            kind.slug(),
            branch,
            before.short(),
            after.short()
        );
        self.operation_history.record(kagi::git::HistoryEntry {
            kind,
            branch: branch.to_string(),
            before,
            after,
            summary,
        });
    }

    /// Open the Undo plan modal for the entry at the history cursor (the most
    /// recent applied operation). Builds a [`Backend::plan_undo`] preview.
    pub fn open_history_undo_modal(&mut self) {
        let entry = match self.operation_history.peek_undo().cloned() {
            Some(e) => e,
            None => {
                self.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
                return;
            }
        };
        self.open_history_modal(entry, true);
    }

    /// Open the Redo plan modal for the entry just past the cursor.
    pub fn open_history_redo_modal(&mut self) {
        let entry = match self.operation_history.peek_redo().cloned() {
            Some(e) => e,
            None => {
                self.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
                return;
            }
        };
        self.open_history_modal(entry, false);
    }

    /// Shared: build an undo/redo plan for `entry` and show the preview modal.
    fn open_history_modal(&mut self, entry: kagi::git::HistoryEntry, is_undo: bool) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "{}: repo open error: {}",
                    if is_undo { "undo" } else { "redo" },
                    e
                )));
                return;
            }
        };
        let plan_res = if is_undo {
            repo.plan_undo(&entry)
        } else {
            repo.plan_redo(&entry)
        };
        match plan_res {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: {} {} blockers={} warnings={}",
                    if is_undo { "undo" } else { "redo" },
                    entry.kind.slug(),
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.set_history_modal(HistoryPlanModal {
                    plan: std::sync::Arc::new(plan),
                    entry,
                    is_undo,
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "{}: plan error: {}",
                    if is_undo { "undo" } else { "redo" },
                    e
                )));
            }
        }
    }

    /// Confirm the open Undo/Redo modal: run preflight + execute via the safe
    /// pipeline, advance/retreat the history cursor, record in the oplog, and
    /// reload. On a stale entry (preflight failure) the entry is left in place
    /// and the error is surfaced.
    pub fn confirm_history(&mut self) {
        let modal = match self.history_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = if modal.is_undo {
            format!("undo-{}", modal.entry.kind.slug())
        } else {
            format!("redo-{}", modal.entry.kind.slug())
        };

        if !modal.plan.blockers.is_empty() {
            self.record_op(
                &op_name,
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.clear_history_modal();
            return;
        }

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_history_modal(HistoryPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
                return;
            }
        };

        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                &op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.set_history_modal(HistoryPlanModal {
                error: Some(SharedString::from(err_msg)),
                ..modal
            });
            return;
        }

        let exec_res = if modal.is_undo {
            repo.execute_undo(&modal.entry)
        } else {
            repo.execute_redo(&modal.entry)
        };

        match exec_res {
            Ok(outcome) => {
                // Advance/retreat the cursor only after the ref move succeeds.
                if modal.is_undo {
                    self.operation_history.undo();
                } else {
                    self.operation_history.redo();
                }
                self.clear_history_modal();
                let after = StateSummary {
                    head: format!("branch '{}' @ {}", outcome.branch, outcome.to.short()),
                    dirty: "index reset to target (working tree preserved)".to_string(),
                };
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "{}: {} → {} (recover: git reflog)",
                    op_name,
                    outcome.from.short(),
                    outcome.to.short()
                )));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!(
                    "{} failed: {}",
                    if modal.is_undo { "Undo" } else { "Redo" },
                    e
                );
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_history_modal(HistoryPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
            }
        }
    }

    /// Build an amend plan for `mode` and open the confirmation modal.
    ///
    /// The new message is read from the commit input (UI path) or the commit
    /// panel's `commit_msg` (headless path).  For [`AmendMode::Staged`] the
    /// message is ignored by the backend.
    ///
    /// Entry point for the Commit Panel "Amend" control — wired by the PM when
    /// the W14-PREVIEW/TEMPLATE commit-panel lanes merge (this lane owns the
    /// backend + modal/confirm plumbing, not `commit_panel.rs`).
    pub fn open_amend_modal(&mut self, mode: AmendMode, cx: &mut Context<Self>) {
        let message: String = if let Some(ref input_entity) = self.commit_input {
            input_entity.read(cx).value().to_string()
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        };
        self.open_amend_modal_with_message(mode, message);
    }

    /// Build an amend plan from an explicit `message` (no `Context` needed).
    /// Used by the headless `KAGI_AMEND` path and by [`open_amend_modal`].
    pub fn open_amend_modal_with_message(&mut self, mode: AmendMode, message: String) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "amend: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let msg_opt = if message.trim().is_empty() {
            None
        } else {
            Some(message.as_str())
        };
        match repo.plan_amend(mode, msg_opt) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: amend mode={:?} blockers={} warnings={} destructive={}",
                    mode,
                    plan.blockers.len(),
                    plan.warnings.len(),
                    plan.destructive
                );
                self.set_amend_modal(AmendPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    mode,
                    message,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("amend plan error: {}", e)));
            }
        }
    }

    /// Cancel the amend modal (also disarms the two-stage confirm).
    pub fn cancel_amend_modal(&mut self) {
        self.clear_amend_modal();
    }

    /// First stage of the two-stage confirm: arm the action.  If already armed
    /// this is the final stage and executes the amend (ADR-0023 history-rewrite).
    pub fn confirm_amend(&mut self) {
        let modal = match self.amend_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Defence: never execute with blockers present.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: amend plan has blockers, not executing");
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        // ── Two-stage confirm: first click only arms ─────────
        if !modal.confirm_armed {
            self.set_amend_modal(AmendPlanModal {
                confirm_armed: true,
                ..modal
            });
            klog!("amend: armed (second confirm required — history rewrite)");
            return;
        }

        // ── Armed: proceed to preflight → execute ────────────
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_amend_modal(AmendPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.set_amend_modal(AmendPlanModal {
                error: Some(SharedString::from(err_msg)),
                ..modal
            });
            return;
        }

        // ADR-0040: record the OLD HEAD SHA in the oplog BEFORE execution.
        // `record_op` writes the before-state; the success record below captures
        // the new HEAD so the旧→新 transition is fully logged.
        let msg_opt = if modal.message.trim().is_empty() {
            None
        } else {
            Some(modal.message.as_str())
        };
        // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
        let amend_op = kagi::git::Operation::Amend {
            mode: modal.mode,
            message: msg_opt.map(|s| s.to_string()),
        };
        match repo.run(&amend_op, &modal.plan) {
            Ok(kagi::git::OperationOutcome::Amend(outcome)) => {
                eprintln!(
                    "[kagi] executed: amend {} -> {}",
                    outcome.old.short(),
                    outcome.new.short()
                );
                self.clear_amend_modal();
                let after = StateSummary {
                    head: format!(
                        "branch @ {} (was {})",
                        outcome.new.short(),
                        outcome.old.short()
                    ),
                    dirty: "amended".to_string(),
                };
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // T-UNDOREDO-001: undo of an amend moves the branch from the new
                // commit back to the pre-amend commit (still in the reflog).
                if let Some((branch, _)) = self.head_branch_and_sha() {
                    self.record_history(
                        kagi::git::OperationKind::Amend,
                        &branch,
                        outcome.old.clone(),
                        outcome.new.clone(),
                        format!("amend {} → {}", outcome.old.short(), outcome.new.short()),
                    );
                }
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "amend: {} → {} (restore: git reset --hard {})",
                    outcome.old.short(),
                    outcome.new.short(),
                    outcome.old.short()
                )));
                self.reload();
            }
            Ok(_) => {
                // Amend only yields OperationOutcome::Amend.
                klog!("amend: unexpected outcome variant");
                return;
            }
            Err(e) => {
                let err_msg = format!("Amend failed: {}", e);
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.set_amend_modal(AmendPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
            }
        }
    }

    /// W15-ASYNCOPS: UI-path amend. The two-stage confirm (armed state) stays on
    /// the main thread; only the final armed execute (history rewrite — tree
    /// build + commit replace) runs on a background thread. Headless keeps
    /// `confirm_amend` (sync).
    pub fn start_amend(&mut self, cx: &mut Context<Self>) {
        let modal = match self.amend_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Defence: never execute with blockers present.
        if !modal.plan.blockers.is_empty() {
            klog!("refused: amend plan has blockers, not executing");
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        // First click only arms (main thread) — matches confirm_amend exactly.
        if !modal.confirm_armed {
            self.set_amend_modal(AmendPlanModal {
                confirm_armed: true,
                ..modal
            });
            klog!("amend: armed (second confirm required — history rewrite)");
            return;
        }

        // Armed → background execute. Refuse a concurrent background op.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }

        self.busy_op = Some("amend");
        self.clear_amend_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyAmend.t()));
        klog!("async: amend started");

        let plan = modal.plan.clone();
        let mode = modal.mode;
        let message = modal.message.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_msg = message.clone();
        let task =
            cx.background_spawn(async move { amend_blocking(&bg_path, &bg_plan, mode, &bg_msg) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((after, old, new)) => {
                        klog!("async: amend finished");
                        app.record_op(
                            "amend",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "amend: {} → {} (restore: git reset --hard {})",
                            old.short(),
                            new.short(),
                            old.short()
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: amend failed — {}", err_msg);
                        app.record_op(
                            "amend",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.set_amend_modal(AmendPlanModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            mode,
                            message: message.clone(),
                            confirm_armed: false,
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
