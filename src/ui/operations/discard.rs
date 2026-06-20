//! Discard (working-tree restore) operations.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 4, P1) as an additional
//! `impl KagiApp` block. Behaviour and signatures are unchanged; a descendant
//! module can access `KagiApp` privates so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Collect the eligible unstaged paths (excluding untracked / conflicted)
    /// plus the skipped set, from the current commit-panel status.
    /// Returns `(eligible, skipped)` as repo-relative forward-slash strings.
    fn discard_partition(&self) -> (Vec<String>, Vec<String>) {
        let mut eligible = Vec::new();
        let mut skipped = Vec::new();
        if let Some(panel) = self.commit_panel.as_ref() {
            for f in &panel.unstaged {
                let rel = f.path.to_string_lossy().replace('\\', "/");
                // Conflicted rows are not discardable. Untracked rows (surfaced as
                // `Added` entries) ARE discardable — they are deleted from disk
                // after an ODB backup (ADR-0083).
                if panel.is_conflicted(&f.path) {
                    skipped.push(rel);
                } else {
                    eligible.push(rel);
                }
            }
        }
        (eligible, skipped)
    }

    /// Open the discard modal for a single unstaged row (by its index in the
    /// commit panel's `unstaged` vector). Conflicted rows are not offered a
    /// Discard menu; untracked rows are (they are deleted after an ODB backup,
    /// ADR-0083).
    pub fn open_discard_modal_for_index(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.unstaged.get(index))
        {
            Some(f) => f.path.to_string_lossy().replace('\\', "/"),
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "discard: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let paths = vec![path];
        match repo.plan_discard(&paths) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: discard 1 target blockers={}",
                    plan.blockers.len()
                );
                self.set_discard_modal(DiscardModal {
                    plan: std::sync::Arc::new(plan),
                    paths,
                    skipped: Vec::new(),
                    is_all: false,
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("discard plan error: {}", e)));
            }
        }
    }

    /// Open the "Discard all" modal: every eligible unstaged file in one
    /// operation; untracked / conflicted files are listed as skipped.
    pub fn open_discard_all_modal(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let (eligible, skipped) = self.discard_partition();
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "discard: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_discard(&eligible) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: discard-all {} target(s) blockers={} skipped={}",
                    eligible.len(),
                    plan.blockers.len(),
                    skipped.len()
                );
                self.set_discard_modal(DiscardModal {
                    plan: std::sync::Arc::new(plan),
                    paths: eligible,
                    skipped,
                    is_all: true,
                    error: None,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("discard plan error: {}", e)));
            }
        }
    }

    /// Dismiss the discard modal without acting.
    pub fn cancel_discard_modal(&mut self) {
        self.clear_discard_modal();
    }

    /// Confirm the discard: run `discard_blocking` on a background thread
    /// (busy_op="discard"), then reload. Mirrors `start_pop`.
    ///
    /// Two-stage confirm (T-REARCH-014, mirrors `start_amend`): the first click
    /// only *arms* the red Discard button; the second click executes. Discard is
    /// the most destructive working-tree op (checkout -- + untracked deletion),
    /// and the single-click execute was the biggest safety gap vs the product
    /// thesis.
    pub fn start_discard(&mut self, cx: &mut Context<Self>) {
        let modal = match self.discard_modal().cloned() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }

        // ── Two-stage confirm: first click only arms ──────────────────
        if !modal.confirm_armed {
            self.set_discard_modal(DiscardModal {
                confirm_armed: true,
                ..modal
            });
            klog!("discard: armed (second confirm required — destructive)");
            cx.notify();
            return;
        }

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() || modal.paths.is_empty() {
            klog!("refused: discard plan has blockers / no targets");
            self.record_op(
                "discard",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.clear_discard_modal();
            cx.notify();
            return;
        }

        self.busy_op = Some("discard");
        self.clear_discard_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDiscard.t()));
        klog!("async: discard started");

        let plan = modal.plan.clone();
        let paths = modal.paths.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_paths = paths.clone();
        let task =
            cx.background_spawn(async move { discard_blocking(&bg_path, &bg_plan, &bg_paths) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        klog!("async: discard finished");
                        app.record_op(
                            "discard",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "discard: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        klog!("async: discard failed — {}", err_msg);
                        app.record_op(
                            "discard",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.set_discard_modal(DiscardModal {
                            plan: plan.clone(),
                            paths: paths.clone(),
                            skipped: modal.skipped.clone(),
                            is_all: modal.is_all,
                            error: Some(SharedString::from(err_msg)),
                            // Force re-arm after a failure: the user is
                            // re-confirming, so require the two-stage flow again.
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
