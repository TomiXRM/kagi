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

    /// Hold from a caller that already knows the completion was indeterminate
    /// — a run-family `on_done` sees the typed outcome, not the receipt
    /// (ADR-0196 Wave 3). Same hold, same notice as [`Self::settle_transport`].
    pub(crate) fn hold_transport(&mut self, owner: &Path, operation: &str, evidence: &str) {
        self.transport_holds.hold(owner, operation);
        self.report_unknown_notice(
            owner,
            format!(
                "{operation}: {evidence}. {}",
                crate::ui::i18n::Msg::TransportRetryHeld.t()
            ),
        );
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
