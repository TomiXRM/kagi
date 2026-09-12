//! Admission for legacy transports whose completion can be indeterminate.
//! Holds survive tab switches and notice dismissal; a snapshot alone does not
//! prove that a remote process stopped. Retained for this application lifetime.
use crate::ui::KagiApp;
use kagi_git::oplog::OpOutcome;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct TransportHolds(HashSet<(PathBuf, String)>);

impl TransportHolds {
    pub(crate) fn settle(&mut self, owner: &Path, operation: &str, outcome: &OpOutcome) -> bool {
        let hold = matches!(
            outcome,
            OpOutcome::Unknown { .. } | OpOutcome::Partial { .. }
        );
        if hold {
            self.0.insert((owner.to_path_buf(), operation.into()));
        }
        hold
    }
    pub(crate) fn contains(&self, owner: &Path, operation: &str) -> bool {
        self.0.contains(&(owner.to_path_buf(), operation.into()))
    }
    fn hold(&mut self, owner: &Path, operation: &str) {
        self.0.insert((owner.to_path_buf(), operation.into()));
    }
}
impl KagiApp {
    pub(crate) fn settle_transport(&mut self, owner: &Path, operation: &str, outcome: &OpOutcome) {
        if self.transport_holds.settle(owner, operation, outcome) {
            let evidence = match outcome {
                OpOutcome::Unknown { evidence, .. } => evidence.as_str(),
                OpOutcome::Partial { error, .. } => error.as_str(),
                _ => "",
            };
            self.report_unknown_notice(
                owner,
                format!(
                    "{operation}: {evidence}. {}",
                    crate::ui::i18n::Msg::TransportRetryHeld.t()
                ),
            );
        }
    }

    /// Settlement for one run-family receipt, before the stale-tab guard: what
    /// the execution boundary made durable is delivered whatever the tab is
    /// doing now (#501, ADR-0196 Wave 3).
    ///
    /// A PR merge that landed but did not finish (`confirmed: false` —
    /// `Partial`) is the one outcome nothing else guards: `apply` releases its
    /// lease and parks no reconcile entry, because the merge *is* done. Only
    /// this hold stops the button offering it again, so it cannot live in the
    /// presentation half. `Unknown` deliberately does **not** hold: the Run
    /// reconcile owns that scope until it is acknowledged, and `TransportHolds`
    /// has no clear API — a hold there would outlive the acknowledgement.
    pub(crate) fn settle_run_receipt(
        &mut self,
        op: &str,
        report: &kagi_git::backend::recording::RunReport,
        repo: &Path,
    ) {
        self.notice_recording_failure(op, &report.recording, repo);
        if let Ok(kagi_git::OperationOutcome::PrMerge {
            number,
            detail,
            confirmed: false,
        }) = &report.result
        {
            let operation = format!("{op} #{number}");
            self.transport_holds.hold(repo, &operation);
            self.report_unknown_notice(
                repo,
                format!(
                    "{operation}: {detail}. {}",
                    crate::ui::i18n::Msg::TransportRetryHeld.t()
                ),
            );
        }
    }

    pub(crate) fn reject_transport_hold(&mut self, owner: &Path, operation: &str) -> bool {
        if !self.transport_holds.contains(owner, operation) {
            return false;
        }
        self.report_unknown_notice(
            owner,
            format!(
                "{operation}: {}",
                crate::ui::i18n::Msg::TransportRetryHeld.t()
            ),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::StateSummary;
    #[test]
    fn uncertain_completion_holds_owner_operation_even_without_presentation() {
        let mut holds = TransportHolds::default();
        let owner = Path::new("host:/repo");
        let after = StateSummary {
            head: "unknown".into(),
            dirty: "unknown".into(),
        };
        for op in ["pull", "pr-merge #1"] {
            assert!(holds.settle(
                owner,
                op,
                &OpOutcome::Unknown {
                    after: after.clone(),
                    evidence: "lost connection".into()
                }
            ));
            assert!(holds.contains(owner, op));
            assert!(!holds.contains(Path::new("other:/repo"), op));
        }
        assert!(!holds.contains(owner, "pr-merge #2"));
        assert!(holds.settle(
            owner,
            "partial",
            &OpOutcome::Partial {
                after,
                error: "changed".into()
            }
        ));
        assert!(holds.contains(owner, "partial"));
        assert!(!holds.settle(
            owner,
            "failed",
            &OpOutcome::Failed {
                error: "never started".into()
            }
        ));
        assert!(!holds.contains(owner, "failed"));
        // A later failed observation must not release an earlier uncertain execution.
        holds.settle(
            owner,
            "pull",
            &OpOutcome::Failed {
                error: "offline".into(),
            },
        );
        assert!(holds.contains(owner, "pull"));
    }
}
