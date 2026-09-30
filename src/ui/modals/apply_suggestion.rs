//! Confirmation payload for applying a PR review suggestion to the working
//! tree (#351, ADR-0210).

use gpui::SharedString;
use kagi_git::{Operation, OperationPlan};

/// State for an apply-suggestion confirmation: the planned operation (the
/// suggestion, the lines captured when it was opened and the PR head its line
/// numbers belong to) and its plan card.
#[derive(Clone)]
pub struct ApplySuggestionModal {
    pub op: Operation,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}
