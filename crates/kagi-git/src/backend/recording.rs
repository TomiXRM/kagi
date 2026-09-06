use super::{ops, Backend, GitError, Operation, OperationOutcome, OperationPlan};

/// Map a `Backend::run` dispatch result into the oplog [`OpOutcome`] (ADR-0149).
///
/// A partially-applied discard (#281) becomes [`OpOutcome::Partial`]; any other
/// `Ok` becomes [`OpOutcome::Success`] with the plan's predicted after-state;
/// an `Err` becomes [`OpOutcome::Failed`]. Pure + `pub` so the mapping (notably
/// the `is_partial` branch) is unit-testable without forcing a real repo
/// failure.
pub fn oplog_outcome_from(
    result: &Result<OperationOutcome, GitError>,
    predicted: &ops::StateSummary,
) -> crate::oplog::OpOutcome {
    match result {
        Ok(OperationOutcome::Discard(d)) if d.is_partial() => crate::oplog::OpOutcome::Partial {
            after: predicted.clone(),
            error: d.error.clone().unwrap_or_default(),
        },
        // #418: persist the restore's recovery handle (savepoint id) in `after`.
        Ok(OperationOutcome::RestoreSnapshot { savepoint }) => crate::oplog::OpOutcome::Success {
            after: ops::StateSummary {
                head: predicted.head.clone(),
                dirty: format!("savepoint {savepoint}"),
            },
        },
        Ok(_) => crate::oplog::OpOutcome::Success {
            after: predicted.clone(),
        },
        Err(e) => crate::oplog::OpOutcome::Failed {
            error: e.to_string(),
        },
    }
}

impl Backend {
    /// Build and append the oplog entry for an op run through [`Backend::run`]
    /// (ADR-0149). `before` comes from `plan.current`; `actor`/`worktree` from
    /// this backend. Write failures are non-fatal (logged to stderr by
    /// `append_oplog`), mirroring the previous UI behaviour.
    pub(super) fn record_run_oplog(
        &self,
        op: &Operation,
        plan: &OperationPlan,
        outcome: crate::oplog::OpOutcome,
    ) {
        let repo = self.path.display().to_string();
        let entry = crate::oplog::OpLogEntry::new(
            op.oplog_name(),
            repo.clone(),
            plan.current.clone(),
            outcome,
        )
        .with_actor(self.actor)
        .with_worktree(Some(repo));
        let _ = crate::oplog::append_oplog(&entry);
    }
}
