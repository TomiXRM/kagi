//! Confirmation payload for a repository-health fix planned from Analyze
//! (#358, ADR-0205). One variant serves both fixes; `fix` says which.

use gpui::SharedString;
use kagi_domain::repo_health::HealthFix;
use kagi_git::OperationPlan;

/// State for a write-commit-graph / enable-fsmonitor confirmation.
#[derive(Clone)]
pub struct RepoHealthModal {
    pub fix: HealthFix,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}
