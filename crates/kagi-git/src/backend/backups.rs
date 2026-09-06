//! Recovery reads and explicit, confirmed oplog retirement (#523).
use super::{recording, Backend};
use crate::oplog::{retention, OpLogEntry, OpOutcome};
use crate::{ops, GitError};
pub use retention::ForgetOplogPlan;

impl Backend {
    /// Read recovery bytes from the recorded ref, without overwriting any file.
    /// Equivalent to `git cat-file blob <backup-ref>`; bare OIDs are not accepted.
    pub fn read_backup(&self, reference: &str) -> Result<Vec<u8>, GitError> {
        ops::backup::read_blob(&self.repo, reference)
    }

    /// Preview explicit retirement. No default age/count expiration is applied.
    /// Confirm the entry and listed recovery roots before executing this plan.
    pub fn plan_forget_oplog_entry(&self, entry: &OpLogEntry) -> Result<ForgetOplogPlan, GitError> {
        retention::plan(&self.repo, entry)
    }

    /// Trust → frozen log/ref preflight → retire → cleanup/verify → one receipt.
    /// Cleanup errors never remove another retained entry's recovery roots.
    pub fn execute_forget_oplog_entry(&self, plan: &ForgetOplogPlan) -> recording::RunReport {
        let mut retired = false;
        let result = self.require_trust().and_then(|()| {
            if !crate::trust::evaluate(&self.path).is_trusted() {
                return Err(GitError::Untrusted(self.path.display().to_string()));
            }
            retention::execute(&self.repo, plan, &mut retired)
        });
        let after = ops::StateSummary {
            head: crate::resolve_head(&self.repo)
                .map(|head| head.display())
                .unwrap_or_else(|_| "unobserved".into()),
            dirty: format!(
                "oplog entry {} removed={retired}; cleanup candidates: {}",
                plan.entry().id,
                plan.backup_refs().collect::<Vec<_>>().join(", ")
            ),
        };
        let outcome = match &result {
            Ok(()) => OpOutcome::Success { after },
            Err(error) if retired => OpOutcome::Partial {
                after,
                error: error.to_string(),
            },
            Err(error) => OpOutcome::Failed {
                error: error.to_string(),
            },
        };
        let recording = self.record_run_oplog("forget-oplog-entry", &plan.entry().before, outcome);
        recording::RunReport {
            result: result.map(|()| super::OperationOutcome::Unit),
            recording,
            stash: None,
        }
    }
}
