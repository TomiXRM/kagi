//! Ordinary Push approvals freeze their branch and effective push transports.

use super::*;
use kagi_domain::plan::PushIdentity;

pub(super) fn identity_changed() -> GitError {
    GitError::Blocked(Box::new(PlanNote::Push(PushNote::PushIdentityChanged)))
}

pub(super) fn capture_push_identity(
    repo: &Repository,
    branch: &str,
    remote: &str,
    set_upstream: bool,
) -> Result<PushIdentity, GitError> {
    // Read-only lookup separates even option-like configured names with `--`.
    // The push executor retains its stricter operand guard before any write.
    let transport = run_git(
        repo.workdir().unwrap_or(repo.path()),
        &["remote", "get-url", "--push", "--all", "--", remote],
    )
    .map_err(|error| crate::cli::context("resolve push destination", error))?;
    if transport.status != 0 {
        return Err(GitError::Other(transport.stderr.trim().to_string()));
    }
    let upstream_ref = repo
        .find_branch(branch, BranchType::Local)
        .and_then(|branch| branch.upstream())
        .ok()
        .and_then(|upstream| upstream.get().name().ok().map(str::to_owned));
    Ok(PushIdentity {
        branch: branch.to_owned(),
        local_oid: CommitId(local_branch_oid(repo, branch)?.to_string()),
        remote: remote.to_owned(),
        push_urls: transport.stdout.lines().map(str::to_owned).collect(),
        set_upstream,
        upstream_ref,
    })
}

/// Refuse changed selection, transports, source or upstream before any write.
/// Re-resolving here only validates approval; execution never chooses a remote.
pub(crate) fn preflight_push(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    let approved = plan.push_identity.as_ref().ok_or_else(identity_changed)?;
    let title_matches = match &plan.title {
        PlanTitle::Push(
            PushTitle::Push {
                branch,
                remote,
                set_upstream,
            }
            | PushTitle::PushBranch {
                branch,
                remote,
                set_upstream,
            },
        ) => {
            branch == &approved.branch
                && remote == &approved.remote
                && *set_upstream == approved.set_upstream
        }
        _ => false,
    };
    let selected_remote = if approved.set_upstream {
        choose_push_remote(repo, None).ok().flatten()
    } else {
        resolve_upstream_info(repo, &approved.branch)
            .ok()
            .map(|(_, remote, _)| remote)
    };
    if !title_matches
        || selected_remote.as_deref() != Some(approved.remote.as_str())
        || capture_push_identity(
            repo,
            &approved.branch,
            &approved.remote,
            approved.set_upstream,
        )
        .ok()
        .as_ref()
            != Some(approved)
    {
        return Err(identity_changed());
    }
    Ok(())
}
