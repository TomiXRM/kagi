use super::*;

// ────────────────────────────────────────────────────────────
// plan_checkout
// ────────────────────────────────────────────────────────────

/// Analyse whether checking out `branch` is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0004 Guarded policy)
///
/// - Target branch does not exist in the repository.
/// - Target branch is already the current HEAD branch (no-op would be confusing).
/// - Repository is in a conflict state (`status.conflicted` is non-empty).
/// - Staged / unstaged changes exist **and** overlap the files the checkout must
///   rewrite (the HEAD-tree → target-tree diff). Only then would a safe-mode
///   checkout actually be refused; the user is pointed at stash. Non-overlapping
///   changes are carried over to the target branch and merely warned about.
///
/// # Warning conditions
///
/// - Staged / unstaged changes that do **not** overlap the checkout — git carries
///   them over to the target branch; the user is told so.
/// - Untracked files exist.  The checkout itself will not touch them but users
///   are reminded they remain after switching branches.
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

    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let dirty_display = if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    };

    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 3. Check blockers ────────────────────────────────────
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Branch existence check.
    let branch_exists = repo.find_branch(branch, BranchType::Local).is_ok();

    if !branch_exists {
        blockers.push(format!(
            "Branch '{}' does not exist in this repository.",
            branch
        ));
    }

    // Already-HEAD check (only meaningful when HEAD is attached).
    if let Head::Attached {
        branch: ref current_branch,
        ..
    } = head
    {
        if current_branch == branch {
            blockers.push(format!(
                "Branch '{}' is already the current HEAD branch.",
                branch
            ));
        }
    }

    // Conflict state check.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before switching branches.",
            status.conflicted.len()
        ));
    }

    // Staged / unstaged changes — Guarded policy, but only a *real* collision
    // blocks. A safe-mode `checkout_tree` carries local changes over to the
    // target branch and refuses only when a locally-modified tracked path
    // overlaps a path the checkout must rewrite (HEAD-tree → target-tree diff).
    // So switch freely when nothing collides, and point at stash only when it
    // does — mirroring `plan_checkout_commit` so the plan matches what execute
    // actually does (no forced stash-before-every-switch).
    if branch_exists && (!status.staged.is_empty() || !status.unstaged.is_empty()) {
        let target_oid = repo
            .find_branch(branch, BranchType::Local)
            .ok()
            .and_then(|b| b.get().peel_to_commit().ok())
            .map(|c| c.id());
        match target_oid.and_then(|oid| predict_checkout_conflict(repo, &head, oid, &status)) {
            Some(blocker) => blockers.push(blocker),
            None => {
                let mut parts = Vec::new();
                if !status.staged.is_empty() {
                    parts.push(format!("{} staged", status.staged.len()));
                }
                if !status.unstaged.is_empty() {
                    parts.push(format!("{} modified", status.unstaged.len()));
                }
                warnings.push(format!(
                    "{} will be carried over to '{}'.",
                    parts.join(", "),
                    branch
                ));
            }
        }
    }

    // Untracked files — warning only (safe checkout leaves them alone).
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain after switching branches.",
            status.untracked.len()
        ));
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
    let recovery = format!(
        "If anything goes wrong you can return to '{}' with:\n  git checkout {}\n\
         The reflog records every HEAD movement:\n  git reflog",
        current_branch_name, current_branch_name
    );

    Ok(OperationPlan {
        title: format!("Checkout branch '{}'", branch),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
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
pub fn execute_checkout(repo: &Repository, branch: &str) -> Result<(), GitError> {
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
/// Dirty working-tree state is surfaced as a warning, not a blocker: execution
/// still uses `checkout_tree(...safe())`, so libgit2 refuses rather than
/// overwriting local changes. This keeps the normal plan → confirm → preflight
/// → execute → verify pipeline intact while preserving the repository on safe
/// checkout failure.
pub fn plan_checkout_commit(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut warnings = vec![
        "detached HEAD になります。新しい作業を残す場合は branch を作成してください。".to_string(),
        "Create branch here を先に使うことを推奨します。".to_string(),
    ];
    let mut blockers = Vec::new();

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
        blockers.push("Commit is already HEAD.".to_string());
    }

    if status.is_dirty() {
        // BUG-2: in-memory dry-run. A safe-mode `checkout_tree` only fails when a
        // locally-modified tracked path overlaps a path the checkout would
        // rewrite (HEAD-tree → target-tree diff). If the dirty paths overlap that
        // set, the green "proceed" plan would error in the footer — promote the
        // warning to a blocker so the plan matches what execute actually does.
        match predict_checkout_conflict(repo, &head, target_oid, &status) {
            Some(blocker) => blockers.push(blocker),
            None => {
                warnings.push(format!(
                    "Working tree is dirty ({}). Safe checkout may fail; stash or commit first.",
                    dirty_display
                ));
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
    let recovery = format!(
        "If this was accidental, return with:\n  git checkout {}\n\
         To keep new work from the detached state, create a branch:\n  git switch -c <name>\n\
         The reflog records every HEAD movement:\n  git reflog",
        current_ref
    );

    Ok(OperationPlan {
        title: format!(
            "Checkout commit {} '{}' (detached HEAD)",
            target_short, summary_line
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// BUG-2 dry-run: predict whether a safe-mode checkout of `target_oid` would be
/// refused because a locally-modified tracked path overlaps a path the checkout
/// must rewrite. Used for **both** branch and commit checkout — the target is
/// just a commit oid in either case (a branch's tip).
///
/// Mirrors [`predict_stash_pop_conflict`] in spirit: pure in-memory analysis,
/// **never** touches the working tree or HEAD. Returns `Some(blocker)` when the
/// dirty (staged or unstaged) tracked paths intersect the HEAD-tree → target-tree
/// diff. Untracked files cannot conflict with a safe checkout, so they are
/// ignored here (libgit2 leaves them in place). On any analysis failure we return
/// `None` (fall back to the existing warning) — never invent a blocker we cannot
/// substantiate.
fn predict_checkout_conflict(
    repo: &Repository,
    head: &Head,
    target_oid: git2::Oid,
    status: &crate::git::status::WorkingTreeStatus,
) -> Option<String> {
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

    // Locally-modified tracked paths (staged + unstaged). Untracked excluded.
    let mut overlap: Vec<String> = Vec::new();
    for f in status.staged.iter().chain(status.unstaged.iter()) {
        let p = f.path.to_string_lossy().into_owned();
        if touched.contains(&p) && !overlap.contains(&p) {
            overlap.push(p);
        }
    }
    if overlap.is_empty() {
        return None;
    }
    overlap.sort();

    Some(format!(
        "Working tree has local changes to {} file(s) that the target also \
         modifies: {}. Safe checkout would be refused (the conflict prevents checkout). \
         Stash or commit these changes first.",
        overlap.len(),
        overlap.join(", ")
    ))
}

/// Execute a detached commit checkout using **safe mode only**.
///
/// Order matters: this checks out the target tree while HEAD still points at
/// the old baseline, then detaches HEAD at the target commit. Moving HEAD first
/// would make safe checkout compare the target tree to itself and risk a no-op.
pub fn execute_checkout_commit(repo: &Repository, id: &CommitId) -> Result<(), GitError> {
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
