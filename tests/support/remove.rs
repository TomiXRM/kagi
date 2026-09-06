use super::*;
use kagi_git::backend::remove::RemovePlan;
use kagi_git::oplog::{Actor, OpOutcome};
pub struct PlannedRemove {
    plan: RemovePlan,
    name: String,
    delete_branch: bool,
}
impl std::ops::Deref for PlannedRemove {
    type Target = OperationPlan;
    fn deref(&self) -> &OperationPlan {
        &self.plan.preview
    }
}
pub fn plan_remove_worktree(
    repo: &Repository,
    name: &str,
    delete_branch: bool,
) -> Result<PlannedRemove, GitError> {
    Ok(PlannedRemove {
        plan: Backend::plan_recorded_remove(
            repo.workdir().unwrap_or(repo.path()),
            name,
            delete_branch,
        )?,
        name: name.into(),
        delete_branch,
    })
}
pub fn execute_remove_worktree(
    repo: &Repository,
    plan: &PlannedRemove,
    name: &str,
    delete_branch: bool,
) -> Result<DiscardOutcome, GitError> {
    assert_eq!(
        (&plan.name, plan.delete_branch),
        (&name.to_string(), delete_branch),
        "fixture must execute its approved target"
    );
    let report = Backend::run_recorded_remove(&plan.plan, Actor::Human, None);
    let result = match &report.recording.entry().outcome {
        OpOutcome::Success { .. } => Ok(DiscardOutcome::complete(report.progress.backups)),
        OpOutcome::Partial { error, .. } => Ok(DiscardOutcome {
            backups: report.progress.backups,
            unverified: vec![plan.plan.target.display().to_string()],
            error: Some(error.clone()),
        }),
        OpOutcome::Refused { blockers } => Err(GitError::Other(blockers.join("; "))),
        OpOutcome::Failed { error }
        | OpOutcome::Unknown {
            evidence: error, ..
        } => Err(GitError::Other(error.clone())),
    };
    refresh_fixture_index(repo, result)
}
