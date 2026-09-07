//! Failure-only delivery for the synchronous staging adapters (#490).
//! Success recording remains the full staging-family migration, not a new
//! parallel executor. All append attempts use the existing receipt finalizer.
use crate::{app, ui::*};
use kagi_git::{
    backend::recording::{self, Recording},
    GitError,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
pub(super) enum StageAction {
    Stage,
    Unstage,
    StageAll,
    UnstageAll,
}
impl StageAction {
    fn name(self) -> &'static str {
        match self {
            Self::Stage => "stage",
            Self::Unstage => "unstage",
            Self::StageAll => "stage-all",
            Self::UnstageAll => "unstage-all",
        }
    }
    fn label(self) -> i18n::Op {
        match self {
            Self::Stage => i18n::Op::Stage,
            Self::Unstage => i18n::Op::Unstage,
            Self::StageAll => i18n::Op::StageAll,
            Self::UnstageAll => i18n::Op::UnstageAll,
        }
    }
}

struct StageFailure {
    recording: Recording,
    footer: String,
    notice: bool,
}
impl StageFailure {
    fn record(
        action: StageAction,
        repo: &Path,
        paths: &[PathBuf],
        error: &str,
        refused: bool,
    ) -> Self {
        let paths = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let detail = format!("{}: {paths}: {error}", repo.display());
        let before = StateSummary {
            head: "unobserved".into(),
            dirty: format!("paths: {paths}"),
        };
        let outcome = if refused {
            OpOutcome::Refused {
                blockers: vec![detail.clone()],
            }
        } else {
            OpOutcome::Failed {
                error: detail.clone(),
            }
        };
        let entry = OpLogEntry::new(action.name(), repo.display().to_string(), before, outcome)
            .with_worktree(Some(repo.display().to_string()));
        let recording = recording::finalize(entry);
        let mut footer = i18n::op_failed(action.label(), detail);
        if let Recording::Failed { error, .. } = &recording {
            // A failed stage is not evidence of a completed mutation. Preserve
            // Failed/Refused while making the missing durable record explicit.
            footer.push_str(&format!(
                "; {}",
                i18n::op_failed(i18n::Op::RecordOperation, error)
            ));
        }
        Self {
            recording,
            footer,
            notice: !refused,
        }
    }
}

impl KagiApp {
    pub(super) fn stage_failure(
        &mut self,
        action: StageAction,
        repo: &Path,
        paths: &[PathBuf],
        error: &GitError,
        cx: &mut Context<Self>,
    ) {
        self.deliver_stage_failure(
            StageFailure::record(
                action,
                repo,
                paths,
                &error.to_string(),
                matches!(error, GitError::Untrusted(_)),
            ),
            cx,
        );
    }
    fn deliver_stage_failure(&mut self, failure: StageFailure, cx: &mut Context<Self>) {
        // The synchronous lease has ended and the receipt has settled before
        // presentation. Never append a second time or turn this into success.
        self.present_recorded(&failure.recording, cx);
        self.status_footer = FooterStatus::Failed(failure.footer.clone().into());
        if failure.notice {
            self.app_notices.push_back(failure.footer.into());
            self.present_app_notice();
        }
        cx.notify();
    }
    pub(super) fn reserve_stage_write(
        &mut self,
        action: StageAction,
        repo: &Path,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> Option<app::WriteGuard> {
        self.refresh_write_busy();
        match app::admit(
            &mut self.reads,
            self.app_sessions
                .write_lease(repo, app::LegacyBusy(self.busy_op.is_some())),
        ) {
            Ok(guard) => {
                self.busy_op = Some("app-writer");
                Some(guard)
            }
            Err(error) => {
                self.deliver_stage_failure(
                    StageFailure::record(action, repo, paths, &error.to_string(), true),
                    cx,
                );
                None
            }
        }
    }
    /// Keep the session fast path but preserve the actual repo-open error.
    /// The outer Result is open failure; the closure's Result is execution.
    pub(super) fn with_staging_repo<R>(
        &self,
        path: &Path,
        f: impl FnOnce(&kagi_git::Backend) -> R,
    ) -> Result<R, GitError> {
        if self.repo_path.as_deref() == Some(path) {
            if let Some(session) = self.repo_session.as_ref() {
                return Ok(f(session.backend()));
            }
        }
        match kagi_git::Backend::open(path) {
            Ok(backend) => Ok(f(&backend)),
            Err(error) => {
                if self.repo_path.as_deref() != Some(path) {
                    klog!("commit-panel: repo open error: {}", error);
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/isolated.rs"]
mod isolated;
#[cfg(test)]
#[path = "../../../tests/recovery/staging_failure_g.rs"]
mod tests;
