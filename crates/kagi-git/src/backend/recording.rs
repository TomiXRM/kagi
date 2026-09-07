use kagi_domain::history::HistoryEntry;
use kagi_domain::plan_note::HistoryMoveDir;

use super::{ops, Backend, GitError, OperationOutcome, OperationPlan};
use crate::oplog::{append_oplog_receipt, recovery, OpLogEntry, RecoveryHandle};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Recording {
    Appended {
        path: PathBuf,
        entry: OpLogEntry,
    },
    Failed {
        attempted: OpLogEntry,
        error: String,
    },
}
impl Recording {
    pub fn entry(&self) -> &OpLogEntry {
        match self {
            Self::Appended { entry, .. } => entry,
            Self::Failed { attempted, .. } => attempted,
        }
    }
}

pub struct RunReport {
    pub result: Result<OperationOutcome, GitError>,
    pub recording: Recording,
    pub stash: Option<super::stash::StashEvidence>,
}

/// Single append implementation, shared with factories that fail before open
/// and with the transport boundaries outside this crate (`src/remote`, #501).
/// Every recorded mutation goes through here — never a second writer.
pub fn finalize(entry: OpLogEntry) -> Recording {
    match append_oplog_receipt(&entry) {
        Ok((path, entry)) => Recording::Appended { path, entry },
        Err(error) => Recording::Failed {
            attempted: entry,
            error: error.to_string(),
        },
    }
}

/// Map a `Backend::run` dispatch result into the oplog [`OpOutcome`] (ADR-0149).
///
/// A partially-applied discard (#281), or a supplied partial after-state from
/// dispatch, becomes [`OpOutcome::Partial`]; any other `Ok` becomes
/// [`OpOutcome::Success`] with the plan's predicted after-state; an `Err`
/// becomes [`OpOutcome::Failed`]. Pure + `pub` so the mapping (notably the
/// `is_partial` branch) is unit-testable without forcing a real repo failure.
pub fn oplog_outcome_from(
    result: &Result<OperationOutcome, GitError>,
    predicted: &ops::StateSummary,
    partial_after: Option<ops::StateSummary>,
) -> crate::oplog::OpOutcome {
    match (result, partial_after) {
        (Err(e), Some(after)) => crate::oplog::OpOutcome::Partial {
            after,
            error: e.to_string(),
        },
        (Ok(OperationOutcome::Discard(d)), _) if d.is_partial() => {
            crate::oplog::OpOutcome::Partial {
                after: predicted.clone(),
                error: d.error.clone().unwrap_or_default(),
            }
        }
        // #418: persist the restore's recovery handle (savepoint id) in `after`.
        (Ok(OperationOutcome::RestoreSnapshot { savepoint }), _) => {
            crate::oplog::OpOutcome::Success {
                after: ops::StateSummary {
                    head: predicted.head.clone(),
                    dirty: format!("savepoint {savepoint}"),
                },
            }
        }
        (
            Ok(OperationOutcome::DeleteBranch {
                name,
                tip,
                reference,
            }),
            _,
        ) => crate::oplog::OpOutcome::Success {
            after: ops::StateSummary {
                head: predicted.head.clone(),
                dirty: format!(
                    "branch '{name}' deleted (tip {tip}); restore: git branch {name} {reference}"
                ),
            },
        },
        (Ok(OperationOutcome::StashDrop { oid }), _) => crate::oplog::OpOutcome::Success {
            after: ops::StateSummary {
                head: predicted.head.clone(),
                dirty: format!("stash entry deleted (oid {oid})"),
            },
        },
        (Ok(_), _) => crate::oplog::OpOutcome::Success {
            after: predicted.clone(),
        },
        (Err(e), _) => crate::oplog::OpOutcome::Failed {
            error: e.to_string(),
        },
    }
}

/// The typed recovery handles a dispatch result carries (#500).
///
/// The `after.dirty` sentences built above stay exactly as they are — they are
/// what the UI shows. This is the same material recorded as data, so a recovery
/// consumer never parses display text. A failed attempt contributes nothing
/// here: its recovery roots, if any, are the mandatory `backup_refs`.
/// Pure + `pub` so every family is unit-testable without a real repo.
pub fn recovery_handles(result: &Result<OperationOutcome, GitError>) -> Vec<RecoveryHandle> {
    let Ok(outcome) = result else {
        return Vec::new();
    };
    match outcome {
        OperationOutcome::RestoreSnapshot { savepoint } => {
            vec![RecoveryHandle::oid(recovery::SAVEPOINT, savepoint)]
        }
        OperationOutcome::StashPush { oid } => vec![RecoveryHandle::oid(recovery::STASH, oid)],
        OperationOutcome::StashDrop { oid } => vec![RecoveryHandle::oid(recovery::STASH, oid)],
        OperationOutcome::DeleteBranch { tip, reference, .. } => {
            vec![RecoveryHandle::oid(recovery::BRANCH_TIP, tip).with_reference(reference)]
        }
        // Partial discards land here too: `is_partial` keeps the outcome `Ok`
        // precisely so the backups are never dropped with an `Err` (#281).
        OperationOutcome::Discard(discard) => discard
            .backups
            .iter()
            .map(|b| RecoveryHandle::file(&b.path, &b.blob, Some(b.reference.clone())))
            .collect(),
        OperationOutcome::Suggestion(s) => {
            vec![RecoveryHandle::file(&s.path, &s.backup_blob, None)]
        }
        _ => Vec::new(),
    }
}

impl Backend {
    /// Build and append the oplog entry for a completed backend attempt
    /// (ADR-0149). `before` comes from the plan; `actor`/`worktree` from
    /// this backend. Write failures are non-fatal (logged to stderr by
    /// `append_oplog`), mirroring the previous UI behaviour.
    pub(super) fn record_run_oplog(
        &self,
        op: &str,
        before: &ops::StateSummary,
        outcome: crate::oplog::OpOutcome,
    ) -> Recording {
        self.record_run_oplog_with_backups(op, before, outcome, Vec::new(), Vec::new())
    }

    pub(super) fn record_run_oplog_with_backups(
        &self,
        op: &str,
        before: &ops::StateSummary,
        outcome: crate::oplog::OpOutcome,
        backup_refs: Vec<String>,
        recovery: Vec<RecoveryHandle>,
    ) -> Recording {
        let repo = self.path.display().to_string();
        let mut entry = crate::oplog::OpLogEntry::new(op, repo.clone(), before.clone(), outcome)
            .with_actor(self.policy.actor)
            .with_worktree(Some(repo));
        entry.backup_refs = backup_refs;
        entry.recovery = recovery;
        finalize(entry)
    }

    /// Preflight, move the recorded branch ref, then persist one entry per attempt.
    pub fn run_history_move(
        &self,
        dir: HistoryMoveDir,
        plan: &OperationPlan,
        entry: &HistoryEntry,
    ) -> Result<ops::HistoryMoveOutcome, GitError> {
        let result = self
            .require_trust()
            .and_then(|()| {
                self.preflight_check(plan)
                    .map_err(|e| GitError::Preflight(Box::new(e)))
            })
            .and_then(|()| match dir {
                HistoryMoveDir::Undo => self.execute_undo(entry),
                HistoryMoveDir::Redo => self.execute_redo(entry),
            });
        let outcome = match &result {
            Ok(moved) => crate::oplog::OpOutcome::Success {
                after: ops::StateSummary {
                    head: format!("branch '{}' @ {}", moved.branch, moved.to.short()),
                    dirty: format!(
                        "moved from {} to {}; index and working tree preserved",
                        moved.from, moved.to
                    ),
                },
            },
            Err(error) => crate::oplog::OpOutcome::Failed {
                error: error.to_string(),
            },
        };
        // #500: the two ends of the move are the recovery material — `to` is
        // where the branch is now, `from` is where it came back from.
        let handles = match &result {
            Ok(moved) => vec![
                RecoveryHandle::oid(recovery::HISTORY_FROM, moved.from.to_string()),
                RecoveryHandle::oid(recovery::HISTORY_TO, moved.to.to_string()),
            ],
            Err(_) => Vec::new(),
        };
        let op_name = format!("{}-{}", dir.label_en_lower(), entry.kind.slug());
        self.record_run_oplog_with_backups(&op_name, &plan.current, outcome, Vec::new(), handles);
        result
    }
}
