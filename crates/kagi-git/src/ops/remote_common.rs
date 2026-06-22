//! Shared helpers for the pull / push / fetch operation pipelines (Wave 3 split,
//! ADR-0116 / T-SPLIT-PULLPUSH-001).
//!
//! These functions used to live in the monolithic `ops/pull_push.rs`. They are
//! moved here verbatim (behaviour-preserving) because they are used by more than
//! one of the sibling `pull.rs` / `push.rs` / `fetch.rs` modules. Visibility is
//! `pub(super)` so the siblings can call them while they remain crate-internal.

use super::*;

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
