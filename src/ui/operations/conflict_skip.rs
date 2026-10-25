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

fn skip_failure(outcome: &OpOutcome) -> Option<String> {
    match outcome {
        OpOutcome::Failed { error } => Some(error.clone()),
        OpOutcome::Unknown { evidence, .. } => Some(evidence.clone()),
        _ => None,
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
        self.flush_conflict_drafts(owner.clone(), "skip", cx, move |app, cx| {
            app.conflict_skip_flushed(owner, cx);
        });
    }

    fn conflict_skip_flushed(&mut self, owner: crate::app::Attachment, cx: &mut Context<Self>) {
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
                if let Some(note) = e.blocker() {
                    self.record_refused(
                        &op_name,
                        StateSummary {
                            head: String::new(),
                            dirty: String::new(),
                        },
                        std::slice::from_ref(note),
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
        let Some(guard) = self.reserve_write_for(
            "conflict-skip",
            &op_name,
            &repo_path,
            self.active_session(),
            cx,
        ) else {
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
        let bg_op = op_name.clone();
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
            let report =
                kagi_git::Backend::run_recorded_conflict_skip(&bg_path, &plan, &buffer, &bg_op);
            let settlement = skip_settlement(&report.result);
            app::settle_conflict_write(guard, &settlement, bg_before);
            Some((report.result, report.recording))
        });
        cx.spawn(async move |this, acx| {
            let (result, recording) = task.fallible().await.flatten().unwrap_or_else(|| {
                let error = abandonment.into_unknown();
                let recording = super::conflict::conflict_unfinished_recording(
                    &op_name, &repo_path, before, &error,
                );
                (Err(error), recording)
            });
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                let current =
                    app.active_session() == Some(owner) && app.app_sessions.visit(owner) == visit;
                let refusal_display = result
                    .as_ref()
                    .err()
                    .and_then(|error| error.blocker())
                    .map(i18n::plan_note_text);
                let outcome = &recording.entry().outcome;
                let (failure, ran) = match result {
                    Ok(_) => (skip_failure(outcome), true),
                    Err(e) => (
                        Some(e.to_string()),
                        matches!(e, kagi_git::GitError::TerminationUnknown(_)),
                    ),
                };
                let termination_unknown_evidence = match outcome {
                    OpOutcome::Unknown { evidence, .. }
                        if evidence.contains("process termination is unconfirmed") =>
                    {
                        Some(evidence.clone())
                    }
                    _ => None,
                };
                match &failure {
                    None => klog!("executed: {}", op_name),
                    Some(err_msg) => klog!("{} failed: {}", op_name, err_msg),
                }
                app.present_operation_completion(&recording, current, false, refusal_display, cx);
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
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "../../../tests/support/isolated.rs"]
mod isolated;
#[cfg(test)]
#[path = "../../../tests/recovery/conflict_skip_g.rs"]
mod tests;
