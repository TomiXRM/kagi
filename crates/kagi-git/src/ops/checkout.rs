//! This module owns existing-local-branch and detached-commit checkout plan/execute;
//! tracking checkout lives in `switch.rs`. [`preflight_check`] is the generic HEAD
//! guard for every non-stash `Backend::run` operation; narrowing it permits stale execution.
//!
use super::*;
use kagi_domain::plan_note::{
    CheckoutNote, CheckoutRecovery, CheckoutTitle, CommonNote, OpPhrase, WorktreeNote,
};
use kagi_domain::remote::shell_quote;

// ────────────────────────────────────────────────────────────
// plan_checkout
// ────────────────────────────────────────────────────────────

/// Analyse whether checking out `branch` is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0004 Guarded policy)
///
/// - Target branch does not exist in the repository.
/// - Target branch is already the current HEAD branch (no-op would be confusing).
/// - Target branch is checked out in another registered worktree.
/// - Repository is in a conflict state (`status.conflicted` is non-empty).
/// - Staged / unstaged changes overlap files the checkout must rewrite
///   (the HEAD-tree → target-tree diff).
/// - An untracked path obstructs a target file the checkout must write, including
///   directory/file collisions. Safe checkout refuses a newly added target file
///   even when the untracked file has identical contents.
///
/// Unrelated local changes and untracked files are carried over quietly.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_checkout(repo: &Repository, branch: &str) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD ──────────────────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Build current StateSummary ────────────────────────
    let head_display = head.display();

    let current = state_summary(head_display.clone(), &status);

    // ── 3. Check blockers ────────────────────────────────────
    // ADR-0129: typed notes (`CheckoutNote` / `CommonNote`), not English
    // prose. `message_en()` renders the exact legacy strings for
    // oplog/klog/EN display (golden-tested in kagi-domain).
    let mut blockers: Vec<PlanNote> = Vec::new();
    let warnings: Vec<PlanNote> = Vec::new();

    // Branch existence check.
    let branch_exists = repo.find_branch(branch, BranchType::Local).is_ok();

    if !branch_exists {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: branch.to_string(),
            in_repo: true,
        }));
    }

    // Already-HEAD check (only meaningful when HEAD is attached).
    if matches!(&head, Head::Attached { branch: current, .. } if current == branch) {
        blockers.push(PlanNote::Checkout(CheckoutNote::AlreadyCurrent {
            branch: branch.to_string(),
        }));
    } else if branch_exists {
        if let Some(path) = branch_checked_out_worktree_path(repo, branch)? {
            blockers.push(PlanNote::Worktree(WorktreeNote::BranchInOtherWorktree {
                branch: branch.to_string(),
                path: path.display().to_string(),
            }));
        }
    }

    // Conflict state check.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::SwitchingBranches,
        }));
    }

    // Both tracked edits and untracked obstructions can make safe checkout
    // refuse. Analyse only the HEAD → target delta: merely existing in the
    // target tree does not mean a path needs to be rewritten.
    if branch_exists && status.is_dirty() {
        let target_oid = repo
            .find_branch(branch, BranchType::Local)
            .ok()
            .and_then(|b| b.get().peel_to_commit().ok())
            .map(|c| c.id());
        if let Some(blocker) =
            target_oid.and_then(|oid| predict_checkout_conflict(repo, &head, oid, &status))
        {
            blockers.push(blocker);
        }
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD will point to the target branch; dirty is unchanged (we only update
    // the head description; working-tree state is preserved or unchanged).
    let predicted = StateSummary {
        head: format!("branch: {}", branch),
        dirty: current.dirty.clone(),
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let current_branch_name = match &head {
        Head::Attached { branch: b, .. } => b.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
        Head::Unborn { branch: b } => b.clone(),
    };
    let recovery = PlanRecovery {
        kind: RecoveryKind::Checkout(CheckoutRecovery::Checkout {
            previous: current_branch_name.clone(),
        }),
        commands: vec![
            format!("git checkout {}", shell_quote(&current_branch_name)),
            "git reflog".to_string(),
        ],
    };

    Ok(OperationPlan {
        rebase_scope: None,
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Checkout(CheckoutTitle::Checkout {
            branch: branch.to_string(),
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        // #353: faithful equivalent — `git checkout <branch>`.
        equivalent_command: (!cfg!(windows))
            .then(|| format!("git checkout {}", shell_quote(branch))),
    })
}

// ────────────────────────────────────────────────────────────
// preflight_check
// ────────────────────────────────────────────────────────────

/// Verify that HEAD has not changed since the plan was generated.
///
/// If the repository state has diverged (e.g. another process committed or
/// checked out a different branch), this returns an error so the caller can
/// abort and ask the user to re-plan.
///
/// # Errors
///
/// Returns [`GitError::Other`] when HEAD has changed or on unexpected failures.
pub fn preflight_check(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    let current_head = resolve_head(repo)?;
    if current_head != plan.head_at_plan {
        return Err(GitError::Other(format!(
            "Repository state changed since planning. \
             HEAD was {:?} at plan time but is now {:?}. \
             Please re-plan before proceeding.",
            plan.head_at_plan, current_head
        )));
    }

    // #295: HEAD alone misses the TOCTOU shifts that matter — a merge that
    // conflicts does not move HEAD, `git rm --cached` does not, an interrupting
    // edit does not. Ops whose blockers depend on the working tree carry a
    // classification digest; refuse if it no longer matches, because those
    // blockers were reasoned against a tree that has since changed.
    if let Some(planned) = plan.worktree_digest() {
        let now = working_tree_status(repo)?.digest();
        if now != planned {
            return Err(GitError::Other(
                "Working tree changed since planning (a file was staged,                  unstaged, deleted, or became conflicted).                  Please re-plan before proceeding."
                    .to_string(),
            ));
        }
    }
    super::remote_common::check_pull_identity(repo, plan)?;
    super::rebase::check_rebase_identity(repo, plan)?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// execute_checkout
// ────────────────────────────────────────────────────────────

/// Execute a branch checkout using **safe mode only**.
///
/// This function performs the two-step libgit2 checkout:
/// 1. `repo.checkout_tree(target_tree, Some(CheckoutBuilder::new().safe()))` —
///    update the working tree and index to match the target branch tip.
/// 2. `repo.set_head("refs/heads/<branch>")` — point HEAD at the target branch.
///
/// The order (checkout_tree **before** set_head) is intentional: updating the
/// working tree before moving HEAD ensures that if the checkout fails mid-way,
/// HEAD still points to the original branch.
///
/// **Force checkout and all reset/clean APIs are explicitly not used here.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure, including safe-mode
/// conflicts where an untracked file would be overwritten.
pub(crate) fn execute_checkout(repo: &Repository, branch: &str) -> Result<(), GitError> {
    // Recheck immediately before the first write. set_head may refuse an
    // occupied branch only after checkout_tree has already changed the files.
    if let Some(path) = branch_checked_out_worktree_path(repo, branch)? {
        return Err(GitError::Other(
            PlanNote::Worktree(WorktreeNote::BranchInOtherWorktree {
                branch: branch.to_string(),
                path: path.display().to_string(),
            })
            .message_en(),
        ));
    }

    // Locate the branch reference.
    let branch_ref = repo
        .find_branch(branch, BranchType::Local)
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", branch, e.message())))?;

    // Peel to the commit, then to the tree object for checkout_tree.
    let obj = branch_ref
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::Other(e.message().to_string()))?
        .into_object();

    // Safe-mode checkout: will NOT overwrite modified tracked files.
    // Force is intentionally absent.
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();

    repo.checkout_tree(&obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;

    // Update HEAD to point at the new branch.
    let refname = format!("refs/heads/{}", branch);
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head failed: {}", e.message())))?;

    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_checkout_commit / execute_checkout_commit
// ────────────────────────────────────────────────────────────

/// Analyse whether checking out `id` as detached HEAD is safe and return an
/// [`OperationPlan`].
///
/// Known tracked overlaps and untracked obstructions block just as they do for
/// branch checkout. Other dirty state retains the detached checkout's warning;
/// execution still uses `checkout_tree(...safe())` and never overwrites local
/// changes to force a checkout. The normal plan → confirm → preflight → execute
/// → verify pipeline remains intact.
pub fn plan_checkout_commit(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    // ADR-0129 appendix §G-1 sanctioned exception: this vec-init used to push
    // two Japanese strings unconditionally. `CheckoutNote::WillDetachHead` /
    // `RecommendCreateBranchHereFirst` give them NEW English `message_en()`
    // text while kagi-ui-core's JA rendering keeps the current Japanese
    // wording byte-for-byte (see plan_note/checkout.rs module docs).
    let mut warnings = vec![
        PlanNote::Checkout(CheckoutNote::WillDetachHead),
        PlanNote::Checkout(CheckoutNote::RecommendCreateBranchHereFirst),
    ];
    let mut blockers: Vec<PlanNote> = Vec::new();

    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;
    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if matches!(&head, Head::Attached { target, .. } | Head::Detached { target } if target == &target_oid.to_string())
    {
        blockers.push(PlanNote::Checkout(CheckoutNote::CommitAlreadyHead));
    }

    if status.is_dirty() {
        // Share branch checkout's tracked-overlap and untracked-obstruction
        // prediction so a known safe-checkout refusal blocks before admission.
        match predict_checkout_conflict(repo, &head, target_oid, &status) {
            Some(blocker) => blockers.push(blocker),
            None => {
                warnings.push(PlanNote::Checkout(CheckoutNote::DirtyMayFail {
                    display: dirty_display.clone(),
                }));
            }
        }
    }

    let target_short = id.short().to_string();
    let predicted = StateSummary {
        head: format!("detached: {}", target_short),
        dirty: dirty_display,
    };

    let current_ref = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
        Head::Unborn { branch } => branch.clone(),
    };
    let summary_line = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect::<String>();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Checkout(CheckoutRecovery::CheckoutCommit {
            previous: current_ref.clone(),
        }),
        commands: vec![
            format!("git checkout {}", shell_quote(&current_ref)),
            "git switch -c <name>".to_string(),
            "git reflog".to_string(),
        ],
    };

    Ok(OperationPlan {
        rebase_scope: None,
        tag_push_identity: None,
        approved_index_digest: None,
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Checkout(CheckoutTitle::CheckoutCommit {
            sha: target_short,
            summary: summary_line,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

/// Predict path collisions for **both** branch and detached-commit checkout.
///
/// Read-only analysis of the HEAD-tree → target-tree diff: tracked edits can
/// collide with rewritten paths, and untracked paths can obstruct target writes.
/// Component-wise matching also catches file/directory obstructions without
/// confusing `dir` with `directory`. Unchanged target paths and deletions alone
/// do not obstruct untracked files. On analysis failure return `None` rather
/// than inventing an unsubstantiated blocker; execution remains safe-mode only.
fn predict_checkout_conflict(
    repo: &Repository,
    head: &Head,
    target_oid: git2::Oid,
    status: &crate::status::WorkingTreeStatus,
) -> Option<PlanNote> {
    // Resolve the current HEAD tree (the baseline the checkout diffs against).
    let head_oid = match head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            git2::Oid::from_str(target).ok()?
        }
        Head::Unborn { .. } => return None,
    };
    let head_tree = repo.find_commit(head_oid).ok()?.tree().ok()?;
    let target_tree = repo.find_commit(target_oid).ok()?.tree().ok()?;

    // Paths the checkout would write (anything that differs between the two trees).
    let mut touched: std::collections::HashSet<String> = std::collections::HashSet::new();
    let diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&target_tree), None)
        .ok()?;
    for delta in diff.deltas() {
        if let Some(p) = delta.old_file().path() {
            touched.insert(p.to_string_lossy().into_owned());
        }
        if let Some(p) = delta.new_file().path() {
            touched.insert(p.to_string_lossy().into_owned());
        }
    }
    if touched.is_empty() {
        return None;
    }

    // Locally-modified tracked paths (staged + unstaged).
    let mut overlap: Vec<String> = Vec::new();
    for f in status.staged.iter().chain(status.unstaged.iter()) {
        let p = f.path.to_string_lossy().into_owned();
        if touched.contains(&p) && !overlap.contains(&p) {
            overlap.push(p);
        }
    }
    for local in &status.untracked {
        let obstructs_target = diff.deltas().any(|delta| {
            let target = delta.new_file();
            target.exists()
                && target
                    .path()
                    .is_some_and(|path| local.starts_with(path) || path.starts_with(local))
        });
        if obstructs_target {
            let path = local.to_string_lossy().into_owned();
            if !overlap.contains(&path) {
                overlap.push(path);
            }
        }
    }
    if overlap.is_empty() {
        return None;
    }
    overlap.sort();

    Some(PlanNote::Checkout(CheckoutNote::CheckoutOverlap {
        count: overlap.len(),
        files: overlap,
    }))
}

/// Execute a detached commit checkout using **safe mode only**.
///
/// Order matters: this checks out the target tree while HEAD still points at
/// the old baseline, then detaches HEAD at the target commit. Moving HEAD first
/// would make safe checkout compare the target tree to itself and risk a no-op.
pub(crate) fn execute_checkout_commit(repo: &Repository, id: &CommitId) -> Result<(), GitError> {
    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;
    let obj = commit.into_object();

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(&obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;

    repo.set_head_detached(target_oid)
        .map_err(|e| GitError::Other(format!("set_head_detached failed: {}", e.message())))?;

    Ok(())
}
