//! Rebase-current-onto operation pipeline (branch-menu "Integrate" group,
//! "Rebase current branch onto <target>").
//!
//! Starts the rebase via the real `git` CLI (`run_git`), same as every other
//! CLI-driven op in this codebase (push/pull/fetch) — reimplementing git's
//! rebase step machine on top of libgit2's lower-level `Rebase` API would
//! duplicate real git's own sequencer for no benefit. A conflict is not a
//! failure: [`RebaseOutcome::Conflicted`] lets the existing conflict-mode
//! detection (`conflicts::detect_conflict_session`) and the fixed-in-this-PR
//! `execute_conflict_continue` sequencer-advance take over from there.

use super::*;
use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};
use kagi_domain::remote::shell_quote;

// ────────────────────────────────────────────────────────────
// plan_rebase_current_onto
// ────────────────────────────────────────────────────────────

/// Analyse whether rebasing the current branch onto `onto` is safe and
/// return an [`OperationPlan`]. Guarded-class (ADR-0004): dirty tree and
/// detached HEAD block; a predicted conflict is not knowable ahead of time
/// for a multi-commit rebase (unlike merge's single in-memory merge), so it
/// is always surfaced as an unconditional warning instead.
///
/// # Blocker conditions
///
/// - HEAD is detached or unborn.
/// - The working tree has uncommitted changes.
/// - `onto` does not resolve to a valid ref or commit.
/// - `onto` already equals the branch's current tip (nothing to replay).
pub fn plan_rebase_current_onto(repo: &Repository, onto: &str) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let head_display = head.display();
    let dirty_display = if status.is_dirty() {
        "dirty".to_string()
    } else {
        "clean".to_string()
    };
    let mut current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display.clone(),
    };
    let mut predicted = StateSummary {
        head: head_display,
        dirty: dirty_display,
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    let warnings: Vec<PlanNote> = vec![PlanNote::Rebase(RebaseNote::MayConflict)];

    let branch_name = match &head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    if branch_name.is_none() {
        blockers.push(PlanNote::Rebase(RebaseNote::DetachedHead));
    }
    if status.is_dirty() {
        blockers.push(PlanNote::Rebase(RebaseNote::DirtyWorkingTree));
    }

    let onto_oid = repo
        .revparse_single(onto)
        .ok()
        .map(|obj| obj.id())
        .and_then(|oid| repo.find_commit(oid).ok().map(|_| oid));
    if onto_oid.is_none() {
        blockers.push(PlanNote::Rebase(RebaseNote::InvalidOnto {
            onto: onto.to_string(),
        }));
    }

    let mut recovery = None;
    if let (Some(branch), Some(onto_oid)) = (branch_name.as_ref(), onto_oid) {
        if let Ok(branch_ref) = repo.find_branch(branch, BranchType::Local) {
            if let Some(head_oid) = branch_ref.get().target() {
                if head_oid == onto_oid {
                    blockers.push(PlanNote::Rebase(RebaseNote::AlreadyUpToDate {
                        branch: branch.clone(),
                        onto: onto.to_string(),
                    }));
                }
                recovery = Some(PlanRecovery {
                    kind: RecoveryKind::Rebase(RebaseRecovery::RebaseCurrentOnto {
                        branch: branch.clone(),
                        from: head_oid.to_string(),
                    }),
                    commands: vec![format!(
                        "git update-ref {} {}",
                        shell_quote(&format!("refs/heads/{branch}")),
                        shell_quote(&head_oid.to_string())
                    )],
                });
            }
        }
    }

    let mut preview_commits = Vec::new();
    let mut rebase_scope = None;
    if let (Some(branch), Some(onto_oid)) = (branch_name.as_ref(), onto_oid) {
        let scope_error =
            |e: git2::Error| GitError::Other(format!("rebase scope read failed: {}", e.message()));
        let head_oid = repo
            .head()
            .map_err(scope_error)?
            .target()
            .ok_or_else(|| GitError::Other("HEAD has no target".into()))?;
        let range = format!("{onto_oid}..{head_oid}");
        let repo_path = repo.workdir().unwrap_or_else(|| repo.path());
        let count = |filter: &str| -> Result<usize, GitError> {
            let out = run_git(repo_path, &["rev-list", "--count", filter, &range])
                .map_err(|e| crate::cli::context("rebase scope read failed", e))?;
            if out.status != 0 {
                return Err(GitError::Other(format!(
                    "rebase scope read failed: {}",
                    out.stderr
                )));
            }
            out.stdout
                .trim()
                .parse()
                .map_err(|_| GitError::Other("invalid rebase scope count".into()))
        };
        let mut total_commits = count("--no-merges")?;
        let is_linear_noop = (head_oid == onto_oid
            || repo
                .graph_descendant_of(head_oid, onto_oid)
                .map_err(scope_error)?)
            && count("--min-parents=2")? == 0;
        if is_linear_noop {
            total_commits = 0;
            if head_oid != onto_oid {
                blockers.push(PlanNote::Rebase(RebaseNote::AlreadyUpToDate {
                    branch: branch.clone(),
                    onto: onto.to_string(),
                }));
            }
        } else {
            let mut walk = repo.revwalk().map_err(scope_error)?;
            walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)
                .map_err(scope_error)?;
            walk.push(head_oid).map_err(scope_error)?;
            walk.hide(onto_oid).map_err(scope_error)?;
            for oid in walk {
                let commit = repo
                    .find_commit(oid.map_err(scope_error)?)
                    .map_err(scope_error)?;
                if commit.parent_count() > 1 {
                    continue;
                }
                preview_commits.push(format!(
                    "{}  {}",
                    &commit.id().to_string()[..8],
                    kagi_domain::text_safety::display_subject(
                        commit.summary().ok().flatten().unwrap_or("(no subject)")
                    )
                ));
                if preview_commits.len() == 100 {
                    break;
                }
            }
        }
        current.head = format!("refs/heads/{branch} @ {head_oid}");
        // The UI localizes this structured scope; never predict a final SHA.
        predicted.dirty = "Result OID is not yet known".into();
        predicted.head = format!("refs/heads/{branch} onto {onto} @ {onto_oid}");
        rebase_scope = Some(kagi_domain::plan::RebaseScope {
            current_ref: format!("refs/heads/{branch}"),
            current_oid: head_oid.to_string(),
            onto_ref: onto.to_string(),
            onto_oid: onto_oid.to_string(),
            total_commits,
        });
    }

    Ok(OperationPlan {
        rebase_scope,
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Rebase(RebaseTitle::RebaseCurrentOnto {
            branch: branch_name.unwrap_or_default(),
            onto: onto.to_string(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits,
        destructive: false,
        equivalent_command: None,
    })
}

pub(crate) fn check_rebase_identity(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    let Some(scope) = &plan.rebase_scope else {
        return Ok(());
    };
    if !repo
        .head()
        .is_ok_and(|head| head.name().is_ok_and(|name| name == scope.current_ref))
    {
        return Err(GitError::Other(
            "Rebase current branch changed since planning; please re-plan".into(),
        ));
    }
    let current = repo
        .revparse_single(&scope.onto_ref)
        .and_then(|object| object.peel_to_commit())
        .map_err(|_| {
            GitError::Other("Rebase target changed since planning; please re-plan".into())
        })?;
    let expected = git2::Oid::from_str(&scope.onto_oid)
        .map_err(|_| GitError::Other("Invalid approved Rebase target OID".into()))?;
    if current.id() != expected {
        return Err(GitError::Other(
            "Rebase target changed since planning; please re-plan".into(),
        ));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// execute_rebase_current_onto
// ────────────────────────────────────────────────────────────

/// Run `git rebase <onto>`. A conflict (non-zero exit, repo left in
/// `RepositoryState::Rebase*`) is reported as [`RebaseOutcome::Conflicted`],
/// not an `Err` — see the module doc.
pub(crate) fn execute_rebase_current_onto(
    repo: &Repository,
    repo_path: &Path,
    onto: &str,
) -> Result<RebaseOutcome, GitError> {
    check_operand("upstream", onto)?;

    let out = run_git(repo_path, &["rebase", "--", onto])
        .map_err(|error| crate::cli::context("rebase failed to start", error))?;

    if !matches!(repo.state(), git2::RepositoryState::Clean) {
        return Ok(RebaseOutcome::Conflicted);
    }

    if out.status != 0 {
        if out.repo_dynamic_settings_disabled {
            return Err(GitError::RebaseCannotStartWithRepoSettingsDisabled(
                out.stderr.trim().to_owned(),
            ));
        }
        return Err(GitError::Other(format!(
            "rebase failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    let head_oid = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| GitError::Other("HEAD has no target after rebase".to_string()))?;
    Ok(RebaseOutcome::Completed {
        head: CommitId(head_oid.to_string()),
    })
}
