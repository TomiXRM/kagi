use super::*;

// ────────────────────────────────────────────────────────────
// plan_cherry_pick  (T016)
// ────────────────────────────────────────────────────────────

/// Analyse whether cherry-picking `id` onto HEAD is safe and return an
/// [`OperationPlan`] with a preview of the files that would change.
///
/// # Core design (ADR-0005)
///
/// Uses `repo.cherrypick_commit(&commit, &head_commit, 0, None)` to build an
/// **in-memory index** — the working tree and repository state are **never
/// modified** by this function.  The `mainline` argument `0` is correct for
/// non-merge commits; merge commits are rejected as a blocker before reaching
/// this call.
///
/// # Blocker conditions
///
/// - Working tree has staged or unstaged changes (dirty).
/// - Repository is in a conflict state.
/// - `id` is a merge commit (parent_count > 1).
/// - `id` is identical to the current HEAD commit.
/// - HEAD is unborn (no commits) or detached.
/// - The cherry-pick produces no changes (already applied).
/// - The in-memory merge predicts conflicts.
///
/// # Warnings
///
/// - Untracked files are present (they are not touched).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn plan_cherry_pick(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    // ── 1. Resolve HEAD ──────────────────────────────────────
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

    // ── 3. Early blockers (before touching git objects) ──────
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Unborn HEAD: no commits → cannot cherry-pick.
    if let Head::Unborn { .. } = &head {
        blockers.push(
            "HEAD is unborn (no commits exist). Cannot cherry-pick onto an empty branch."
                .to_string(),
        );
    }

    // Detached HEAD: MVP requires an attached branch.
    if let Head::Detached { .. } = &head {
        blockers.push(
            "HEAD is detached. Cherry-pick is only supported when HEAD is on a branch.".to_string(),
        );
    }

    // Conflict state in repo.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before cherry-picking.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged / unstaged).
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree has {} — stash or commit changes before cherry-picking.",
            parts.join(", ")
        ));
    }

    // Untracked files: warning only.
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched after cherry-pick.",
            status.untracked.len()
        ));
    }

    // ── 4. Resolve target commit ──────────────────────────────
    // Try both full and prefix match.
    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| {
            // Try short-sha prefix lookup via revparse.
            repo.revparse_single(&id.0).map(|obj| obj.id())
        })
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    // Merge commit check.
    if commit.parent_count() > 1 {
        blockers.push(format!(
            "Commit {} is a merge commit ({} parents). Cherry-picking merge commits \
             requires explicit mainline selection, which is not supported in MVP.",
            id.short(),
            commit.parent_count()
        ));
    }

    // HEAD-same check: resolve HEAD commit.
    let head_oid_opt = match &head {
        Head::Attached { target, .. } => git2::Oid::from_str(target).ok(),
        Head::Detached { target } => git2::Oid::from_str(target).ok(),
        Head::Unborn { .. } => None,
    };

    if let Some(head_oid) = head_oid_opt {
        if head_oid == target_oid {
            blockers.push(format!(
                "Commit {} is the current HEAD commit. Nothing to cherry-pick.",
                id.short()
            ));
        }
    }

    // ── 5. If early blockers, return without in-memory merge ─
    // (Prevents calling cherrypick_commit on unborn/detached/merge/HEAD-same)
    if !blockers.is_empty() {
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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
        });
    }

    // ── 6. Resolve HEAD commit (guaranteed to exist at this point) ─
    let head_commit = repo
        .find_commit(head_oid_opt.unwrap())
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 7. In-memory cherry-pick (core dry-run) ───────────────
    // repo.cherrypick_commit(&commit, &head_commit, mainline=0, None)
    // mainline=0 is correct for non-merge commits (already guarded above).
    // This does NOT modify the working tree or repo state.
    let mut index = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("cherry-pick in-memory failed: {}", e.message())))?;

    // ── 8. Conflict detection ─────────────────────────────────
    let mut conflict_files: Vec<String> = Vec::new();
    if index.has_conflicts() {
        // Collect conflicting paths.
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
        for conflict_result in conflicts {
            let conflict = conflict_result
                .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
            // Each of ours/theirs/ancestor may be Some; grab whichever has a path.
            // In git2, IndexEntry.path is a Vec<u8>.
            let path_bytes: Option<Vec<u8>> = conflict
                .our
                .as_ref()
                .map(|e| e.path.clone())
                .or_else(|| conflict.their.as_ref().map(|e| e.path.clone()))
                .or_else(|| conflict.ancestor.as_ref().map(|e| e.path.clone()));
            if let Some(p) = path_bytes {
                let path_str = String::from_utf8_lossy(&p).into_owned();
                conflict_files.push(path_str);
            }
        }
        blockers.push(format!(
            "Cherry-pick would produce {} conflict(s): {}. Resolve divergence before cherry-picking.",
            conflict_files.len(),
            conflict_files.join(", ")
        ));
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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
        });
    }

    // ── 9. Write in-memory tree and compute preview_files ─────
    // index.write_tree_to(repo) writes the in-memory tree without touching WT.
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;

    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree for preview failed: {}", e.message())))?;

    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("head tree lookup failed: {}", e.message())))?;

    // Diff head tree → cherry-picked tree to get preview files.
    let mut diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;

    // Enable rename detection (same as commit_changed_files).
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files: Vec<FileStatus> = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }

    // ── 10. Empty-result check (already applied) ─────────────
    if preview_files.is_empty() {
        blockers.push(format!(
            "Cherry-picking {} would produce no changes — it appears to have been applied already.",
            id.short()
        ));
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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
        });
    }

    // ── 11. Build plan ─────────────────────────────────────────
    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => "(unknown)".to_string(),
    };

    // summary() returns Result<Option<&str>, Error> in git2 0.21.
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();

    let predicted = StateSummary {
        head: format!(
            "branch: {} (+1 commit: '{}' applied)",
            branch_name, summary_line
        ),
        dirty: "clean".to_string(),
    };

    let recovery = "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
         The previous HEAD sha is recorded in the reflog:\n  git reflog"
        .to_string();

    Ok(OperationPlan {
        title: format!(
            "Cherry-pick {} '{}' onto {}",
            id.short(),
            summary_line,
            branch_name
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_cherry_pick  (T016)
// ────────────────────────────────────────────────────────────

/// Execute a cherry-pick of commit `id` onto HEAD using an **in-memory index**.
///
/// # Design (ADR-0005, T016)
///
/// 1. Calls `repo.cherrypick_commit(&commit, &head_commit, 0, None)` to build
///    an in-memory index — identical to [`plan_cherry_pick`].  This does NOT
///    set the repository into CHERRYPICK state (unlike `repo.cherrypick()`
///    which is **never called** in this codebase).
/// 2. Verifies there are no conflicts (preflight-style double-check).  If
///    conflicts are detected, returns an error without writing anything.
/// 3. Calls `index.write_tree_to(repo)` to write the result tree to the ODB.
/// 4. Creates a new commit via `repo.commit(Some("HEAD"), original_author,
///    committer_from_config, original_message, &tree, &[&head_commit])`.
///    Author and message are preserved from the source commit; committer is
///    read from repo config (falls back to `"kagi <kagi@local>"`).
/// 5. Syncs the working tree to the new HEAD with
///    `repo.checkout_head(Some(CheckoutBuilder::new().safe()))`.
///
/// Returns the new commit's [`CommitId`].
///
/// **`repo.cherrypick()` (the working-tree variant) is never called.**
/// **No reset/force/clean APIs are used.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure.
pub fn execute_cherry_pick(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    // ── 1. Resolve target commit ──────────────────────────────
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

    // ── 2. Resolve HEAD commit ────────────────────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 3. In-memory cherry-pick (no WT, no repo state change) ─
    // mainline=0 is correct for non-merge commits.
    let mut index = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("cherry-pick in-memory failed: {}", e.message())))?;

    // ── 4. Conflict preflight double-check ───────────────────
    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Cherry-pick of {} would produce conflicts. Re-plan before executing.",
            id.short()
        )));
    }

    // ── 5. Write in-memory tree to ODB ───────────────────────
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    // ── 6. Build committer signature ──────────────────────────
    let committer = build_signature(repo)?;

    // ── 7. Preserve author and message from source commit ────
    let original_author = commit.author();
    // message() returns Result<&str, Error> in git2 0.21.
    let original_message = commit
        .message()
        .unwrap_or("(cherry-picked commit)")
        .to_string();

    // ── 8. Create the new commit WITHOUT moving any ref ──────
    // ORDER MATTERS (same pitfall as the pull FF/merge paths): the WT/index
    // must be checked out while HEAD still points at the OLD tree so that
    // safe checkout sees old→new as the change set and updates modified
    // files.  Moving HEAD first turns the checkout into a no-op and leaves
    // stale WT content for files the picked commit modified.
    let new_oid = repo
        .commit(
            None,
            &original_author,
            &committer,
            &original_message,
            &new_tree,
            &[&head_commit],
        )
        .map_err(|e| GitError::Other(format!("commit creation failed: {}", e.message())))?;

    // ── 9. Sync WT + index to the new tree (old baseline) ────
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!(
                "checkout_tree after cherry-pick failed: {}",
                e.message()
            ))
        })?;

    // ── 10. Advance the branch ref to the new commit ─────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    repo.reference(
        &refname,
        new_oid,
        true,
        &format!("cherry-pick: {}", &new_oid.to_string()[..8]),
    )
    .map_err(|e| {
        GitError::Other(format!(
            "branch ref update (cherry-pick) failed: {}",
            e.message()
        ))
    })?;

    Ok(CommitId(new_oid.to_string()))
}

/// Analyse whether reverting `id` on the current branch is safe and return an
/// [`OperationPlan`] built from an in-memory revert index.
///
/// The working tree and refs are not modified by this function. Merge commits
/// are refused even if callers failed to disable them in the menu.
pub fn plan_revert(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
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

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if let Head::Unborn { .. } = &head {
        blockers.push(
            "HEAD is unborn (no commits exist). Cannot revert on an empty branch.".to_string(),
        );
    }
    if let Head::Detached { .. } = &head {
        blockers.push(
            "HEAD is detached. Revert is only supported when HEAD is on a branch.".to_string(),
        );
    }
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before reverting.",
            status.conflicted.len()
        ));
    }
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        warnings.push(format!(
            "Working tree has {}. Safe checkout may refuse if those files overlap the revert.",
            parts.join(", ")
        ));
    }
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched after revert.",
            status.untracked.len()
        ));
    }

    let target_oid = if id.0.len() == 40 {
        git2::Oid::from_str(&id.0).or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
    } else {
        repo.revparse_single(&id.0).map(|obj| obj.id())
    }
    .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if commit.parent_count() > 1 {
        blockers.push(format!(
            "Commit {} is a merge commit ({} parents). Reverting merge commits requires explicit mainline selection, which is not supported in MVP.",
            id.short(),
            commit.parent_count()
        ));
    }

    let head_oid_opt = match &head {
        Head::Attached { target, .. } => git2::Oid::from_str(target).ok(),
        Head::Detached { target } => git2::Oid::from_str(target).ok(),
        Head::Unborn { .. } => None,
    };

    if let Some(head_oid) = head_oid_opt {
        let is_on_current_branch = head_oid == target_oid
            || repo
                .graph_descendant_of(head_oid, target_oid)
                .unwrap_or(false);
        if !is_on_current_branch {
            blockers.push(format!(
                "Commit {} is not contained in the current branch. Revert only operates on current-branch commits.",
                id.short()
            ));
        }
    }

    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => "(unknown)".to_string(),
    };
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();
    let recovery = "To undo this revert after execution, revert the new revert commit:\n  git revert <new-revert-commit-sha>\n\
         The previous HEAD sha is recorded in the reflog:\n  git reflog".to_string();

    let blocked_plan =
        |blockers: Vec<String>, warnings: Vec<String>, current: StateSummary| OperationPlan {
            title: format!(
                "Revert {} '{}' on {}",
                id.short(),
                summary_line,
                branch_name
            ),
            current: current.clone(),
            predicted: StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            },
            warnings,
            blockers,
            recovery: recovery.clone(),
            head_at_plan: head.clone(),
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        };

    if !blockers.is_empty() {
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let head_oid =
        head_oid_opt.ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    let mut index = repo
        .revert_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("revert in-memory failed: {}", e.message())))?;

    if index.has_conflicts() {
        let mut conflict_files = Vec::new();
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
        for conflict_result in conflicts {
            let conflict = conflict_result
                .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
            let path_bytes: Option<Vec<u8>> = conflict
                .our
                .as_ref()
                .map(|e| e.path.clone())
                .or_else(|| conflict.their.as_ref().map(|e| e.path.clone()))
                .or_else(|| conflict.ancestor.as_ref().map(|e| e.path.clone()));
            if let Some(p) = path_bytes {
                conflict_files.push(String::from_utf8_lossy(&p).into_owned());
            }
        }
        blockers.push(format!(
            "Revert would produce {} conflict(s): {}. Resolve divergence before reverting.",
            conflict_files.len(),
            conflict_files.join(", ")
        ));
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree for preview failed: {}", e.message())))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("head tree lookup failed: {}", e.message())))?;

    let mut diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }

    if preview_files.is_empty() {
        blockers.push(format!(
            "Reverting {} would produce no changes.",
            id.short()
        ));
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let predicted = StateSummary {
        head: format!(
            "branch: {} (+1 revert commit が新規作成されます: Revert \"{}\")",
            branch_name, summary_line
        ),
        dirty: if current.dirty == "clean" {
            "clean".to_string()
        } else {
            current.dirty.clone()
        },
    };

    Ok(OperationPlan {
        title: format!(
            "Revert {} '{}' on {}",
            id.short(),
            summary_line,
            branch_name
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute a revert of `id` on HEAD using an in-memory index.
///
/// The ref-order rule is deliberately preserved: create the commit object,
/// safe-checkout the new tree while HEAD still points at the old baseline, then
/// move the current branch ref to the new commit.
pub fn execute_revert(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    let target_oid = if id.0.len() == 40 {
        git2::Oid::from_str(&id.0).or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
    } else {
        repo.revparse_single(&id.0).map(|obj| obj.id())
    }
    .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if commit.parent_count() > 1 {
        return Err(GitError::Other(format!(
            "Commit {} is a merge commit. Re-plan before executing.",
            id.short()
        )));
    }

    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    if !refname.starts_with("refs/heads/") {
        return Err(GitError::Other(
            "HEAD is detached. Re-plan before executing.".to_string(),
        ));
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    let mut index = repo
        .revert_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("revert in-memory failed: {}", e.message())))?;

    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Revert of {} would produce conflicts. Re-plan before executing.",
            id.short()
        )));
    }

    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    let committer = build_signature(repo)?;
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();
    let message = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        summary_line,
        commit.id()
    );

    let new_oid = repo
        .commit(
            None,
            &committer,
            &committer,
            &message,
            &new_tree,
            &[&head_commit],
        )
        .map_err(|e| GitError::Other(format!("commit creation failed: {}", e.message())))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!(
                "checkout_tree after revert failed: {}",
                e.message()
            ))
        })?;

    repo.reference(
        &refname,
        new_oid,
        true,
        &format!("revert: {}", &new_oid.to_string()[..8]),
    )
    .map_err(|e| {
        GitError::Other(format!(
            "branch ref update (revert) failed: {}",
            e.message()
        ))
    })?;

    Ok(CommitId(new_oid.to_string()))
}

// ────────────────────────────────────────────────────────────
// PullOutcome  (T-HT-003)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// plan_pull  (T-HT-003)
// ────────────────────────────────────────────────────────────
