//! Merge one branch into another **without checking either of them out**
//! (ADR-0144).
//!
//! [`super::merge`] merges into HEAD, so the branch you want to update has to
//! be the branch you are standing on. This module updates a branch you are not
//! standing on: the merge is computed in memory and only that branch's ref
//! moves. The working tree, the index and HEAD are all left exactly as they
//! were — which is the entire point, and also what makes it safe to offer as a
//! drag-and-drop gesture.
//!
//! The one thing it cannot do is resolve conflicts. Conflict resolution happens
//! in the working tree, and the working tree belongs to the current branch, so
//! a conflicting merge is a blocker here rather than an entry into Conflict
//! Mode. `super::merge` remains the path for that.

use super::*;

use kagi_domain::plan_note::{CommonNote, MergeNote, MergeRecovery, MergeTitle};

/// What [`execute_merge_into_branch`] will do, decided at plan time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeIntoKind {
    /// `target` has no commits of its own: its ref moves to `source`.
    FastForward,
    /// A two-parent merge commit is written and `target` moves to it.
    MergeCommit,
}

/// Resolve a local branch's tip, or `None` if it is not a local branch.
fn local_branch_oid(repo: &Repository, name: &str) -> Option<git2::Oid> {
    repo.find_branch(name, BranchType::Local)
        .ok()
        .and_then(|b| b.get().target())
}

/// How a drop target name resolves to a local branch.
///
/// A graph badge may be a remote-tracking chip (`origin/feature`), which has no
/// local ref to move. Rather than refusing the drop, the destination becomes
/// the local branch of that name — created at the remote tip if it does not
/// exist yet. Nothing is ever pushed: the remote ref is read, never written.
struct ResolvedTarget {
    /// The local branch the merge lands on.
    local: String,
    /// Set when the drop target was a remote-tracking ref.
    remote_ref: Option<String>,
    /// The local branch does not exist yet and must be created at `create_at`.
    create_at: Option<git2::Oid>,
}

fn resolve_target(repo: &Repository, target: &str) -> Result<ResolvedTarget, GitError> {
    if local_branch_oid(repo, target).is_some() {
        return Ok(ResolvedTarget {
            local: target.to_string(),
            remote_ref: None,
            create_at: None,
        });
    }
    // Not a local branch — try it as a remote-tracking ref.
    if let Ok(rb) = repo.find_branch(target, BranchType::Remote) {
        // `origin/feature` → `feature`. `split_once` (not `rsplit_once`): a
        // branch name may itself contain slashes, the remote name may not.
        let local = target
            .split_once('/')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or_else(|| target.to_string());
        let tip = rb.get().target();
        return Ok(ResolvedTarget {
            create_at: local_branch_oid(repo, &local)
                .is_none()
                .then_some(tip)
                .flatten(),
            local,
            remote_ref: Some(target.to_string()),
        });
    }
    Ok(ResolvedTarget {
        local: target.to_string(),
        remote_ref: None,
        create_at: None,
    })
}

/// Plan merging `source` into `target`, where `target` is **not** the current
/// branch.
///
/// Returns the plan and the kind of merge it would perform. Nothing is written.
pub fn plan_merge_into_branch(
    repo: &Repository,
    source: &str,
    target: &str,
) -> Result<(OperationPlan, MergeIntoKind), GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut warnings: Vec<PlanNote> = Vec::new();
    let mut blockers: Vec<PlanNote> = Vec::new();

    let current_branch = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => String::new(),
    };

    let title = PlanTitle::Merge(MergeTitle::Into {
        target: source.to_string(),
        current: Some(target.to_string()),
    });

    // A remote chip resolves to the local branch of that name, created at the
    // remote tip if it does not exist yet (ADR-0144).
    let resolved = resolve_target(repo, target)?;
    let target = resolved.local.as_str();

    // The target's tip before anything moves — the recovery command needs it,
    // and it is the value preflight re-checks. For a branch about to be
    // created, that is the remote tip it will be created at.
    let target_oid_opt = local_branch_oid(repo, target).or(resolved.create_at);
    let previous_sha = target_oid_opt.map(|o| o.to_string()).unwrap_or_default();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Merge(MergeRecovery::AfterMergeIntoBranch {
            target: target.to_string(),
            previous_sha: previous_sha.clone(),
        }),
        commands: vec![format!("git branch -f {target} {previous_sha}")],
    };

    let blocked = |blockers: Vec<PlanNote>, warnings: Vec<PlanNote>| OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: title.clone(),
        current: current.clone(),
        predicted: StateSummary {
            head: current.head.clone(),
            dirty: current.dirty.clone(),
        },
        warnings,
        blockers,
        recovery: Some(recovery.clone()),
        head_at_plan: head.clone(),
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    };

    // ── 1. Both sides must be real, distinct local branches ──────────────
    if source == target {
        blockers.push(PlanNote::Merge(MergeNote::TargetIsCurrent {
            target: target.to_string(),
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    }
    let Some(source_oid) = local_branch_oid(repo, source) else {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: source.to_string(),
            in_repo: true,
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    };
    let Some(target_oid) = target_oid_opt else {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: target.to_string(),
            in_repo: true,
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    };
    if let Some(remote_ref) = resolved.remote_ref.as_ref() {
        if resolved.create_at.is_some() {
            warnings.push(PlanNote::Merge(MergeNote::IntoCreatesLocalBranch {
                local: target.to_string(),
                remote_ref: remote_ref.clone(),
            }));
        } else if repo
            .find_branch(remote_ref, BranchType::Remote)
            .ok()
            .and_then(|b| b.get().target())
            != Some(target_oid)
        {
            warnings.push(PlanNote::Merge(MergeNote::IntoLocalDiffersFromRemote {
                local: target.to_string(),
                remote_ref: remote_ref.clone(),
            }));
        }
    }

    // ── 2. The target must not be checked out anywhere ───────────────────
    // Here: `super::merge` owns that case and can also enter Conflict Mode.
    if !current_branch.is_empty() && current_branch == target {
        blockers.push(PlanNote::Merge(MergeNote::TargetIsCurrent {
            target: target.to_string(),
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    }
    // …and not in a linked worktree, whose index and files would be left
    // describing a commit its HEAD no longer points at.
    if let Some(wt) = super::branch::worktree_checkout_of(repo, target) {
        blockers.push(PlanNote::Merge(MergeNote::IntoCheckedOutElsewhere {
            target: target.to_string(),
            worktree: wt.name,
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    }

    // ── 3. Nothing to do ─────────────────────────────────────────────────
    if target_oid == source_oid
        || repo
            .graph_descendant_of(target_oid, source_oid)
            .unwrap_or(false)
    {
        blockers.push(PlanNote::Merge(MergeNote::IntoAlreadyContains {
            target: target.to_string(),
            source: source.to_string(),
        }));
        return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
    }

    let target_commit = repo
        .find_commit(target_oid)
        .map_err(|e| GitError::Other(format!("target commit lookup failed: {}", e.message())))?;
    let source_commit = repo
        .find_commit(source_oid)
        .map_err(|e| GitError::Other(format!("source commit lookup failed: {}", e.message())))?;

    // ── 4. Fast-forward or real merge? ───────────────────────────────────
    let kind = if repo
        .graph_descendant_of(source_oid, target_oid)
        .unwrap_or(false)
    {
        warnings.push(PlanNote::Merge(MergeNote::IntoFastForward {
            target: target.to_string(),
            source: source.to_string(),
        }));
        MergeIntoKind::FastForward
    } else {
        // In-memory only: no index or working-tree write happens here.
        let index = repo
            .merge_commits(&target_commit, &source_commit, None)
            .map_err(|e| {
                GitError::Other(format!("merge_commits in-memory failed: {}", e.message()))
            })?;
        if index.has_conflicts() {
            let count = index.conflicts().map(|c| c.count()).unwrap_or(0);
            blockers.push(PlanNote::Merge(MergeNote::IntoWouldConflict {
                target: target.to_string(),
                source: source.to_string(),
                count,
            }));
            return Ok((blocked(blockers, warnings), MergeIntoKind::MergeCommit));
        }
        MergeIntoKind::MergeCommit
    };

    if !current_branch.is_empty() {
        warnings.push(PlanNote::Merge(MergeNote::IntoWorkingTreeUntouched {
            current: current_branch.clone(),
        }));
    }

    Ok((
        OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            title,
            current: current.clone(),
            // HEAD does not move, so the predicted state is the current one.
            predicted: current,
            warnings,
            blockers,
            recovery: Some(recovery),
            head_at_plan: head,
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        },
        kind,
    ))
}

/// Execute a merge planned by [`plan_merge_into_branch`].
///
/// Only `refs/heads/<target>` moves. `checkout_tree` and `set_head` — the two
/// calls in [`super::merge::execute_merge_branch`] that touch the working tree
/// — are deliberately absent: the files on disk belong to the current branch,
/// which this operation is not changing.
pub fn execute_merge_into_branch(
    repo: &Repository,
    source: &str,
    target: &str,
) -> Result<CommitId, GitError> {
    // Re-derive rather than trusting the plan: between plan and execute either
    // branch may have moved.
    let (plan, kind) = plan_merge_into_branch(repo, source, target)?;
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(format!(
            "Merging '{}' into '{}' is blocked. Re-plan before executing.",
            source, target
        )));
    }

    let source_oid = local_branch_oid(repo, source)
        .ok_or_else(|| GitError::Other(format!("branch '{source}' not found")))?;

    // A remote chip was dropped onto: materialise the local branch first, at
    // the remote's tip and tracking it. Creating it before the merge (rather
    // than committing first and pointing a new ref at the result) keeps the
    // ref-order rule — the branch exists, then it moves — and leaves a
    // recoverable state if the merge fails.
    let resolved = resolve_target(repo, target)?;
    let target = resolved.local.as_str();
    if let Some(at) = resolved.create_at {
        let commit = repo
            .find_commit(at)
            .map_err(|e| GitError::Other(format!("remote tip lookup failed: {}", e.message())))?;
        let mut branch = repo
            .branch(target, &commit, false)
            .map_err(|e| GitError::Other(format!("branch create failed: {}", e.message())))?;
        if let Some(remote_ref) = resolved.remote_ref.as_deref() {
            branch.set_upstream(Some(remote_ref)).ok();
        }
    }

    let target_oid = local_branch_oid(repo, target)
        .ok_or_else(|| GitError::Other(format!("branch '{target}' not found")))?;
    let refname = format!("refs/heads/{target}");

    let new_oid = match kind {
        MergeIntoKind::FastForward => source_oid,
        MergeIntoKind::MergeCommit => {
            let target_commit = repo.find_commit(target_oid).map_err(|e| {
                GitError::Other(format!("target commit lookup failed: {}", e.message()))
            })?;
            let source_commit = repo.find_commit(source_oid).map_err(|e| {
                GitError::Other(format!("source commit lookup failed: {}", e.message()))
            })?;
            let mut index = repo
                .merge_commits(&target_commit, &source_commit, None)
                .map_err(|e| {
                    GitError::Other(format!("merge_commits in-memory failed: {}", e.message()))
                })?;
            if index.has_conflicts() {
                return Err(GitError::Other(format!(
                    "Merging '{}' into '{}' would conflict. Re-plan before executing.",
                    source, target
                )));
            }
            let tree_oid = index.write_tree_to(repo).map_err(|e| {
                GitError::Other(format!("index.write_tree_to failed: {}", e.message()))
            })?;
            let tree = repo
                .find_tree(tree_oid)
                .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;
            let sig = build_signature(repo)?;
            repo.commit(
                // No ref update here: the ref moves last, on its own.
                None,
                &sig,
                &sig,
                &format!("Merge branch '{source}' into {target}"),
                &tree,
                &[&target_commit, &source_commit],
            )
            .map_err(|e| {
                GitError::Other(format!("merge commit creation failed: {}", e.message()))
            })?
        }
    };

    let reflog = match kind {
        MergeIntoKind::FastForward => {
            format!("merge: fast-forward {source} into {target} (off-branch)")
        }
        MergeIntoKind::MergeCommit => format!("merge: {source} into {target} (off-branch)"),
    };
    let mut branch_ref = repo
        .find_reference(&refname)
        .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
    branch_ref
        .set_target(new_oid, &reflog)
        .map_err(|e| GitError::Other(format!("branch ref update failed: {}", e.message())))?;

    Ok(CommitId(new_oid.to_string()))
}
