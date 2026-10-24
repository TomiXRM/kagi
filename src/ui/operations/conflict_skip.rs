//! Sequencer conflict Skip adapter, split from the conflict editor module.

use crate::{app, ui::*};
#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_CONFLICT_SKIP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONFLICT_SKIP_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn panic_next_conflict_skip_for_e2e() {
        PANIC_NEXT_CONFLICT_SKIP.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn hold_next_conflict_skip_for_e2e(hold: gpui::Task<()>) {
        CONFLICT_SKIP_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
}

/// Classify before settling the guard: `Ok` means the child returned, not that
/// the sequencer's effect is known. Keep the original outcome for its observed
/// `after` state and evidence; this result only controls the existing C0 bridge.
fn skip_settlement(
    result: &Result<kagi_git::SkipOutcome, kagi_git::GitError>,
) -> Result<(), kagi_git::GitError> {
    match result {
        Ok(outcome) if outcome.progress == SkipProgress::Unclear => Err(
            kagi_git::GitError::TerminationUnknown(kagi_git::Termination::stopped(
                outcome
                    .error
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            )),
        ),
        Ok(_) => Ok(()),
        Err(error) => Err(error.clone()),
    }
}

fn skip_outcome(outcome: kagi_git::SkipOutcome) -> (OpOutcome, Option<String>) {
    let git_said = outcome.error.map(|e| e.to_string()).unwrap_or_default();
    match outcome.progress {
        SkipProgress::Finished | SkipProgress::Advanced => (
            OpOutcome::Success {
                after: outcome.after,
            },
            None,
        ),
        SkipProgress::NoProgress => (
            OpOutcome::Failed {
                error: git_said.clone(),
            },
            Some(git_said),
        ),
        SkipProgress::Unclear => (
            OpOutcome::Unknown {
                after: outcome.after,
                evidence: git_said.clone(),
            },
            Some(git_said),
        ),
    }
}

impl KagiApp {
    /// Skip the current sequencer step (rebase / cherry-pick / revert) through
    /// the plan pipeline (T-042, ADR-0067): `plan_conflict_skip` → execute →
    /// oplog → re-detect. Merge has no skip.
    pub fn conflict_skip(&mut self, owner: crate::app::Attachment, cx: &mut Context<Self>) {
        if !self.conflict_action_owner_on_screen(&owner) {
            return;
        }
        if self.reject_if_busy(cx) {
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict_mode_snapshot(cx) else {
            return;
        };
        let Some(expected) = mode.skip_approval.as_ref() else {
            return;
        };
        let repo = match self.ui().repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(i18n::op::op_failed_contract(
                        i18n::Op::RepoOpen,
                        "session unavailable",
                    )),
                    cx,
                );
                return;
            }
        };
        let op_name = format!("{}-skip", mode.session.op.slug());
        let plan = match repo.plan_conflict_skip(&mode.session, expected) {
            Ok(p) => p,
            Err(e) => {
                if let kagi_git::GitError::Blocked(note) = &e {
                    self.record_refused(
                        &op_name,
                        StateSummary {
                            head: String::new(),
                            dirty: String::new(),
                        },
                        std::slice::from_ref(note.as_ref()),
                        &repo_path,
                        cx,
                    );
                    return;
                }
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(i18n::op::op_plan_failed_contract(i18n::Op::Skip, e)),
                    cx,
                );
                return;
            }
        };
        let Some(guard) = self.reserve_write("conflict-skip", &repo_path, cx) else {
            return;
        };

        let owner = owner.session;
        let visit = self.app_sessions.visit(owner);
        let buffer = mode.buffer.clone();
        let before = plan.plan.current.clone();
        let abandonment = guard.abandonment();
        let supervision = abandonment.supervision();
        let bg_path = repo_path.clone();
        let bg_before = before.clone();
        #[cfg(feature = "gui-e2e")]
        let hold = CONFLICT_SKIP_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_CONFLICT_SKIP.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = std::panic::catch_unwind(|| panic!("injected conflict skip panic"));
                return None;
            }
            let (result, ref_moves) = match kagi_git::Backend::open(&bg_path) {
                Ok(backend) => {
                    backend.observe_ref_moves(|b| b.execute_conflict_skip(&plan, &buffer))
                }
                Err(error) => (Err(error), None),
            };
            let settlement = skip_settlement(&result);
            let unknown = app::settle_conflict_write(guard, &settlement, bg_before);
            Some((result, ref_moves, unknown))
        });
        cx.spawn(async move |this, acx| {
            let (result, ref_moves, unknown) = task.fallible().await.flatten().unwrap_or_else(|| {
                let error = abandonment.into_unknown();
                let unknown = if let kagi_git::GitError::TerminationUnknown(reason) = &error {
                    Some(OpOutcome::Unknown {
                        after: before.clone(),
                        evidence: format!("{}; process termination is unconfirmed — do not retry this operation", reason),
                    })
                } else {
                    None
                };
                (Err(error), None, unknown)
            });
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                let current = app.active_session() == Some(owner) && app.app_sessions.visit(owner) == visit;
                let refusal_display = match &result {
                    Err(kagi_git::GitError::Blocked(note)) => Some(i18n::plan_note_text(note)),
                    _ => None,
                };
                let (outcome, failure, ran) = match result {
                    Ok(o) => {
                        let (outcome, failure) = skip_outcome(o);
                        (outcome, failure, true)
                    }
                    Err(e) => {
                        let err_msg = format!("{}", e);
                        let refused = e.blocker().map(|note| OpOutcome::Refused {
                            blockers: vec![note.message_en()],
                        });
                        (unknown.or(refused).unwrap_or_else(|| OpOutcome::Failed { error: err_msg.clone() }), Some(err_msg), matches!(e, kagi_git::GitError::TerminationUnknown(_)))
                    }
                };
                let termination_unknown_evidence = match &outcome {
                    OpOutcome::Unknown { evidence, .. } if evidence.contains("process termination is unconfirmed") => Some(evidence.clone()),
                    _ => None,
                };
                match &failure {
                    None => klog!("executed: {}", op_name),
                    Some(err_msg) => klog!("{} failed: {}", op_name, err_msg),
                }
                app.record_operation_completion(&op_name, before, outcome, ref_moves, &repo_path, current, false, refusal_display, cx);
                if current {
                    if let Some(evidence) = termination_unknown_evidence {
                        app.report_unknown_notice(&repo_path, evidence);
                    }
                    if ran {
                        app.reload(cx);
                        if let Some(ui) = app.ui_mut() {
                            ui.conflict_detected = false;
                        }
                        app.detect_conflict_mode(cx);
                    }
                }
                for (id, op, path) in app.app_sessions.drain_unaccounted() {
                    app.notice_reconcile_required(id, op, &path);
                }
                app.present_app_notice();
                cx.notify();
            });
        }).detach();
    }
}

#[cfg(test)]
#[path = "../../../tests/support/isolated.rs"]
mod isolated;
#[cfg(test)]
#[path = "../../../tests/recovery/conflict_skip_g.rs"]
mod tests;
