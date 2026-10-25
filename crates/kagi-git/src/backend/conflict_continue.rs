//! Continue and Skip own their receipt before returning to the lease owner.
use super::*;
use kagi_domain::conflict_family::ConflictRevision;

pub struct ConflictCompletion<T> {
    pub result: Result<T, GitError>,
    pub recording: recording::Recording,
}

/// Preserve uncertainty separately from execution errors and recording errors.
pub fn conflict_completion_error(
    error: &GitError,
    before: &ops::StateSummary,
) -> crate::oplog::OpOutcome {
    if let Some(note) = error.blocker() {
        return crate::oplog::OpOutcome::Refused {
            blockers: vec![note.message_en()],
        };
    }
    match error {
        GitError::TerminationUnknown(reason) => crate::oplog::OpOutcome::Unknown {
            after: before.clone(),
            evidence: format!(
                "{reason}; process termination is unconfirmed — do not retry this operation"
            ),
        },
        _ => crate::oplog::OpOutcome::Failed {
            error: error.to_string(),
        },
    }
}

pub fn conflict_skip_outcome(outcome: &conflicts::SkipOutcome) -> crate::oplog::OpOutcome {
    use crate::oplog::OpOutcome;
    let error = || {
        outcome
            .error
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    };
    match outcome.progress {
        conflicts::SkipProgress::Finished | conflicts::SkipProgress::Advanced => {
            OpOutcome::Success {
                after: outcome.after.clone(),
            }
        }
        conflicts::SkipProgress::NoProgress => OpOutcome::Failed { error: error() },
        conflicts::SkipProgress::Unclear => OpOutcome::Unknown {
            after: outcome.after.clone(),
            evidence: error(),
        },
    }
}

impl Backend {
    fn run_recorded_conflict_completion<T>(
        path: &Path,
        op: &str,
        before: ops::StateSummary,
        execute: impl FnOnce(&Backend) -> Result<T, GitError>,
        success: impl FnOnce(&T) -> crate::oplog::OpOutcome,
    ) -> ConflictCompletion<T> {
        let (result, ref_moves) = match Self::open(path) {
            Ok(backend) => backend.observe_ref_moves(execute),
            Err(error) => (Err(error), Some(Vec::new())),
        };
        let outcome = match &result {
            Ok(value) => success(value),
            Err(error) => conflict_completion_error(error, &before),
        };
        let recording = recording::finalize(
            crate::oplog::OpLogEntry::new(op, path.display().to_string(), before, outcome)
                .with_worktree(Some(path.display().to_string()))
                .with_ref_moves(ref_moves),
        );
        ConflictCompletion { result, recording }
    }

    pub fn run_recorded_conflict_continue(
        path: &Path,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
        expected: &ConflictRevision,
        before: ops::StateSummary,
    ) -> ConflictCompletion<conflicts::ContinueResult> {
        Self::run_recorded_conflict_completion(
            path,
            &format!("{}-continue", session.op.slug()),
            before,
            |backend| backend.execute_conflict_continue(session, buffer, expected),
            |result| crate::oplog::OpOutcome::Success {
                after: result.after.clone(),
            },
        )
    }

    pub fn run_recorded_conflict_handoff(
        path: &Path,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
        expected: &ConflictRevision,
        before: ops::StateSummary,
    ) -> ConflictCompletion<()> {
        let completion = Self::run_recorded_conflict_completion(
            path,
            &format!("{}-continue", session.op.slug()),
            before,
            |backend| {
                backend.stage_conflict_resolution(session, buffer, expected)?;
                conflicts::current_state_summary(&backend.repo).map_err(|error| {
                    GitError::TerminationUnknown(crate::Termination::stopped(format!(
                        "merge resolutions staged but state unreadable: {error}"
                    )))
                })
            },
            |after| crate::oplog::OpOutcome::Success {
                after: after.clone(),
            },
        );
        ConflictCompletion {
            result: completion.result.map(|_| ()),
            recording: completion.recording,
        }
    }

    pub fn run_recorded_conflict_skip(
        path: &Path,
        approved: &conflicts::ConflictSkipPlan,
        buffer: &ResolutionBuffer,
        op: &str,
    ) -> ConflictCompletion<conflicts::SkipOutcome> {
        Self::run_recorded_conflict_completion(
            path,
            op,
            approved.plan.current.clone(),
            |backend| backend.execute_conflict_skip(approved, buffer),
            conflict_skip_outcome,
        )
    }
}
