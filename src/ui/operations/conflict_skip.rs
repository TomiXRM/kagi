//! Sequencer conflict Skip adapter, split from the conflict editor module.

use crate::{app, ui::*};

impl KagiApp {
    /// Skip the current sequencer step (rebase / cherry-pick / revert) through
    /// the plan pipeline (T-042, ADR-0067): `plan_conflict_skip` → execute →
    /// oplog → re-detect. Merge has no skip.
    pub fn conflict_skip(&mut self, cx: &mut Context<Self>) {
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
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, "session unavailable")),
                    cx,
                );
                return;
            }
        };
        let plan = match repo.plan_conflict_skip(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(i18n::op_plan_failed(i18n::Op::Skip, e)),
                    cx,
                );
                return;
            }
        };
        let op_name = format!("{}-skip", mode.session.op.slug());
        let Some(guard) = self.reserve_write(&repo_path, cx) else {
            return;
        };

        // #540: repository state, not exit status alone, decides progress.
        let result = self
            .repo_session
            .as_ref()
            .expect("repo session existed while planning conflict skip")
            .backend()
            .execute_conflict_skip(&mode.session, &mode.buffer);
        let unknown = app::settle_conflict_write(guard, &result, plan.current.clone());
        self.refresh_write_busy();
        let (outcome, failure, ran) = match result {
            Ok(o) => {
                // Preserve GitError's display payload (#567 P2).
                let git_said = o.error.map(|e| format!("{}", e)).unwrap_or_default();
                let (outcome, failure) = match o.progress {
                    SkipProgress::Finished | SkipProgress::Advanced => {
                        (OpOutcome::Success { after: o.after }, None)
                    }
                    SkipProgress::NoProgress => (
                        OpOutcome::Failed {
                            error: git_said.clone(),
                        },
                        Some(git_said),
                    ),
                    SkipProgress::Unclear => (
                        OpOutcome::Unknown {
                            after: o.after,
                            evidence: git_said.clone(),
                        },
                        Some(git_said),
                    ),
                };
                (outcome, failure, true)
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                (
                    unknown.unwrap_or_else(|| OpOutcome::Failed {
                        error: err_msg.clone(),
                    }),
                    Some(err_msg),
                    matches!(e, kagi_git::GitError::TerminationUnknown(_)),
                )
            }
        };
        let termination_unknown = matches!(
            &outcome,
            OpOutcome::Unknown { evidence, .. }
                if evidence.contains("process termination is unconfirmed")
        );
        match &failure {
            None => klog!("executed: {}", op_name),
            Some(err_msg) if termination_unknown => {
                klog!("{} outcome unknown: {}", op_name, err_msg)
            }
            Some(err_msg) => klog!("{} failed: {}", op_name, err_msg),
        }
        self.record_op_persist(&op_name, plan.current.clone(), outcome, &repo_path, cx);
        if ran {
            // The repository may have moved even without a clean success.
            self.reload(cx);
            self.conflict_detected_for = None;
            self.detect_conflict_mode(cx);
        }
        if let Some(err_msg) = failure {
            self.push_toast(ToastKind::Error, SharedString::from(err_msg), cx);
        }
        cx.notify();
    }
}
