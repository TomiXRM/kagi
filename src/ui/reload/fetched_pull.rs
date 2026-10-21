//! Retain only this visit's unchanged fetched strict-FF explanation or choice.

use std::sync::Arc;

use kagi_git::{Backend, OperationPlan, RepoSnapshot};

use crate::app::SessionId;
use crate::ui::modals::{BranchPlanKind, BranchPlanModal};

pub(super) fn retained_plan(
    modal: &BranchPlanModal,
    backend: &Backend,
    snap: &RepoSnapshot,
    owner: Option<(SessionId, u64)>,
) -> Option<Arc<OperationPlan>> {
    if modal.kind != BranchPlanKind::PullFfOnly
        || modal.error.is_some()
        || owner.is_none()
        || modal.fetch_owner != owner
        || modal.plan.head_at_plan != snap.head
        || modal.dirty_digest.as_ref() != Some(&snap.status.digest())
    {
        return None;
    }
    let fetched = modal.fetched_refs.as_ref()?;
    let (identity, upstream_target) = fetched;
    // Project only for the snapshot label; the live equality below still
    // requires the full ref and exact configured remote, including '/' or '.'.
    let upstream_name = identity
        .upstream_ref
        .strip_prefix("refs/heads/")
        .or_else(|| identity.upstream_ref.strip_prefix("refs/remotes/"))?;
    if identity.branch != modal.branch_name
        || !snap.branches.iter().any(|branch| {
            branch.name == modal.branch_name
                && branch.target == identity.local_oid
                && branch
                    .upstream
                    .as_ref()
                    .is_some_and(|upstream| upstream.remote_branch == upstream_name)
        })
    {
        return None;
    }
    let upstream_matches = if let Some(name) = identity.upstream_ref.strip_prefix("refs/heads/") {
        snap.branches
            .iter()
            .any(|branch| branch.name == name && &branch.target == upstream_target)
    } else {
        snap.remote_branches.iter().any(|branch| {
            upstream_name
                .strip_prefix(branch.remote.as_str())
                .and_then(|name| name.strip_prefix('/'))
                .is_some_and(|name| name == branch.name)
                && &branch.target == upstream_target
        })
    };
    if !upstream_matches || &backend.pull_branch_ref_state(&modal.branch_name).ok()? != fetched {
        return None;
    }
    if modal.plan.blockers.is_empty() {
        // Execution-ready choices retain the original exact identity and preflight.
        if modal.plan.pull_identity.as_ref() != Some(identity)
            || backend.preflight_check(&modal.plan).is_err()
            || modal.dirty_digest.as_ref() != Some(&backend.working_tree_status().ok()?.digest())
        {
            return None;
        }
        return Some(modal.plan.clone());
    }

    // A refusal is display-only: blocked plans deliberately have no executable
    // Pull identity and cannot pass preflight. Re-read its reason without ever
    // promoting this old request into an executable plan or automatic write.
    let (fresh, digest) = backend
        .plan_pull_branch_ff_preview(&modal.branch_name)
        .ok()?;
    if fresh.blockers.is_empty()
        || fresh.head_at_plan != snap.head
        || modal.dirty_digest.as_ref() != Some(&digest)
        || &backend.pull_branch_ref_state(&modal.branch_name).ok()? != fetched
    {
        return None;
    }
    Some(Arc::new(fresh))
}
