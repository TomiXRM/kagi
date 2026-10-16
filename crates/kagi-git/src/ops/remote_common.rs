//! Shared helpers for the pull / push / fetch operation pipelines (Wave 3 split,
//! ADR-0116 / T-SPLIT-PULLPUSH-001).
//!
//! These functions used to live in the monolithic `ops/pull_push.rs`. They are
//! moved here verbatim (behaviour-preserving) because they are used by more than
//! one of the sibling `pull.rs` / `push.rs` / `fetch.rs` modules. Visibility is
//! `pub(super)` so the siblings can call them while they remain crate-internal.

use super::*;
use kagi_domain::plan::PullIdentity;

/// Resolve upstream info for a local branch.
///
/// Returns `(branch_name, remote_name, behind_count)`.
pub(super) fn resolve_upstream_info(
    repo: &Repository,
    branch_name: &str,
) -> Result<(String, String, usize), GitError> {
    // Open the branch config to find the remote name.
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|e| {
            GitError::Other(format!(
                "branch '{}' not found: {}",
                branch_name,
                e.message()
            ))
        })?;

    let upstream = branch.upstream().map_err(|e| {
        GitError::Other(format!(
            "no upstream for '{}': {}",
            branch_name,
            e.message()
        ))
    })?;

    // upstream.name() returns Result<Option<&str>>.
    let upstream_name = upstream
        .name()
        .map_err(|e| GitError::Other(format!("upstream name error: {}", e.message())))?
        .ok_or_else(|| GitError::Other("upstream has no name".to_string()))?
        .to_string();

    // Parse "origin/branchname" → remote name is everything before the first '/'.
    let remote_name = upstream_name
        .split('/')
        .next()
        .unwrap_or("origin")
        .to_string();

    // Compute behind count (local info only).
    let head_oid = branch
        .get()
        .target()
        .ok_or_else(|| GitError::Other("branch has no target".to_string()))?;

    let upstream_oid = upstream
        .get()
        .target()
        .ok_or_else(|| GitError::Other("upstream has no target".to_string()))?;

    let (_, behind) = repo
        .graph_ahead_behind(head_oid, upstream_oid)
        .unwrap_or((0, 0));

    Ok((branch_name.to_string(), remote_name, behind))
}

/// Resolve the OID of the upstream tracking branch tip.
pub(super) fn resolve_upstream_oid(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<git2::Oid, GitError> {
    // Try "refs/remotes/<remote>/<branch>" first.
    let refname = format!("refs/remotes/{}/{}", remote_name, branch_name);
    if let Ok(r) = repo.find_reference(&refname) {
        if let Some(oid) = r.target() {
            return Ok(oid);
        }
    }

    // Fall back to following the upstream ref from the branch config.
    resolve_configured_upstream_oid(repo, branch_name)
}

/// Resolve exactly the configured upstream tip, without preferring a same-name branch.
pub(super) fn resolve_configured_upstream_oid(
    repo: &Repository,
    branch_name: &str,
) -> Result<git2::Oid, GitError> {
    let branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|e| {
            GitError::Other(format!(
                "branch '{}' not found: {}",
                branch_name,
                e.message()
            ))
        })?;
    let upstream = branch.upstream().map_err(|e| {
        GitError::Other(format!(
            "no upstream for '{}': {}",
            branch_name,
            e.message()
        ))
    })?;
    upstream
        .get()
        .target()
        .ok_or_else(|| GitError::Other("upstream ref has no target OID".to_string()))
}

pub(super) fn short_oid_string(oid: git2::Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

pub(super) fn local_branch_oid(
    repo: &Repository,
    branch_name: &str,
) -> Result<git2::Oid, GitError> {
    repo.find_branch(branch_name, BranchType::Local)
        .map_err(|e| {
            GitError::Other(format!(
                "branch '{}' not found: {}",
                branch_name,
                e.message()
            ))
        })?
        .get()
        .target()
        .ok_or_else(|| GitError::Other(format!("branch '{}' has no target OID", branch_name)))
}

pub(super) fn capture_pull_identity(
    repo: &Repository,
    branch_name: &str,
) -> Result<PullIdentity, GitError> {
    let branch = repo
        .find_branch(branch_name, git2::BranchType::Local)
        .map_err(|error| GitError::Other(error.to_string()))?;
    let upstream = branch
        .upstream()
        .map_err(|error| GitError::Other(error.to_string()))?;
    let local_oid = branch
        .get()
        .target()
        .ok_or_else(|| GitError::Other("pull branch has no target".into()))?;
    let upstream_ref = upstream
        .get()
        .name()
        .map_err(|error| GitError::Other(error.to_string()))?;
    let remote = repo
        .config()
        .map_err(|error| GitError::Other(error.to_string()))?
        .get_string(&format!("branch.{branch_name}.remote"))
        .map_err(|error| GitError::Other(error.to_string()))?;
    Ok(PullIdentity {
        branch: branch_name.to_owned(),
        local_oid: CommitId(local_oid.to_string()),
        remote,
        upstream_ref: upstream_ref.to_owned(),
    })
}

pub(super) fn check_pull_identity(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    let Some(expected) = plan.pull_identity.as_ref() else {
        return Ok(());
    };
    let branch = repo
        .find_branch(&expected.branch, git2::BranchType::Local)
        .map_err(|error| GitError::Other(error.to_string()))?;
    let upstream = branch
        .upstream()
        .map_err(|error| GitError::Other(error.to_string()))?;
    let local_oid = git2::Oid::from_str(&expected.local_oid.0)
        .map_err(|error| GitError::Other(error.to_string()))?;
    let config = repo
        .config()
        .map_err(|error| GitError::Other(error.to_string()))?;
    let remote = config
        .get_entry(&format!("branch.{}.remote", expected.branch))
        .map_err(|error| GitError::Other(error.to_string()))?;
    let upstream_ref = upstream
        .get()
        .name()
        .map_err(|error| GitError::Other(error.to_string()))?;
    if branch.get().target() != Some(local_oid)
        || upstream_ref != expected.upstream_ref
        || !remote.has_value()
        || remote
            .value()
            .map_err(|error| GitError::Other(error.to_string()))?
            != expected.remote
    {
        return Err(GitError::Other(
            "Pull branch or upstream changed since planning. Fetch and re-plan before proceeding."
                .into(),
        ));
    }
    Ok(())
}
