//! Set-upstream operation: point a local branch at a remote-tracking branch.
//! Config-only — no refs, index or working tree change. Split out of
//! `ops/push.rs`, whose Push menu it sits beside (LOC ratchet).

use super::*;
use kagi_domain::plan_note::{CommonNote, PushNote, PushRecovery, PushTitle, RecoveryKind};

pub fn plan_set_upstream(
    repo: &Repository,
    branch_name: &str,
    upstream: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    // ADR-0129 appendix §B-5 / §A15: `Branch '{}' does not exist.` is the
    // cross-op `CommonNote::BranchMissing { in_repo: false }` tail (shared in
    // English with rename); the rest are this op's own `PushNote` variants.
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if repo.find_branch(branch_name, BranchType::Local).is_err() {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: branch_name.to_string(),
            in_repo: false,
        }));
    }
    if upstream.trim().is_empty() || upstream.trim() != upstream {
        blockers.push(PlanNote::Push(PushNote::UpstreamFormatInvalid));
    } else if repo.find_branch(upstream, BranchType::Remote).is_err() {
        warnings.push(PlanNote::Push(PushNote::UpstreamNotPresentLocally {
            upstream: upstream.to_string(),
        }));
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Push(PushTitle::SetUpstream {
            branch: branch_name.to_string(),
            upstream: upstream.to_string(),
        }),
        current,
        predicted: StateSummary {
            head: format!("branch: {} -> {}", branch_name, upstream),
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Push(PushRecovery::SetUpstream {
                branch: branch_name.to_string(),
            }),
            commands: Vec::new(),
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

pub(crate) fn execute_set_upstream(
    repo: &Repository,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    let mut branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|e| {
            GitError::Other(format!(
                "branch '{}' not found: {}",
                branch_name,
                e.message()
            ))
        })?;
    branch
        .set_upstream(Some(upstream))
        .map_err(|e| GitError::Other(format!("set upstream failed: {}", e.message())))?;
    Ok(())
}
