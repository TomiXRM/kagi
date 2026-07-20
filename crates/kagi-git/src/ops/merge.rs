use super::*;

// ────────────────────────────────────────────────────────────
// plan_merge_branch / execute_merge_branch  (T-BCM-030 / W31-MERGE-INTO-CONFLICT)
// ────────────────────────────────────────────────────────────

/// Analyse whether merging `target` into the current branch is safe, returning
/// the [`OperationPlan`] paired with the [`MergeKind`] it will perform.
///
/// The dry run is entirely in-memory; no working-tree or ref writes occur during
/// planning. Real blockers (detached / unborn HEAD, already-contains, nothing to
/// merge, a pre-existing conflicted state) still populate `plan.blockers`. A
/// **predicted conflict is NOT a blocker** (W31): blockers stay empty, a warning
/// lists the conflicted file(s), `predicted.dirty` reflects the conflict count,
/// and the returned kind is [`MergeKind::Conflicts`].
pub fn plan_merge_branch(
    repo: &Repository,
    target: &str,
) -> Result<(OperationPlan, MergeKind), GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut warnings = merge_dirty_warnings(&status, "merging");
    let mut blockers = Vec::new();

    let (current_branch, head_oid) = match &head {
        Head::Attached { branch, target } => {
            let oid = git2::Oid::from_str(target)
                .map_err(|e| GitError::Other(format!("HEAD oid parse failed: {}", e.message())))?;
            (branch.clone(), oid)
        }
        Head::Detached { .. } => {
            blockers.push("HEAD is detached. Merge is only supported on a branch.".to_string());
            (String::new(), git2::Oid::ZERO_SHA1)
        }
        Head::Unborn { .. } => {
            blockers.push("HEAD is unborn. Cannot merge into an empty branch.".to_string());
            (String::new(), git2::Oid::ZERO_SHA1)
        }
    };

    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before merging.",
            status.conflicted.len()
        ));
    }

    // ADR-0105: dirty tracked working tree BLOCKS merge (mirrors cherry-pick /
    // revert). Merge writes conflict markers into the working files when a real
    // conflict occurs, which interleaves those markers with the user's
    // uncommitted edits — `git merge --abort` would then discard both. Untracked
    // files stay a warning (they don't participate in the merge).
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree has {} — stash or commit changes before merging.",
            parts.join(", ")
        ));
        // The dirty-WT warning is now redundant with the blocker; drop it so the
        // plan modal shows one clear message. Untracked-only warning survives.
        warnings.retain(|w| {
            !w.to_lowercase().contains("working tree has")
                && !w.to_lowercase().contains("suggested command: git stash")
        });
    }

    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();
    if !current_branch.is_empty() && target == current_branch {
        blockers.push(format!(
            "Branch '{}' is already the current branch.",
            target
        ));
    }
    if head_oid == target_oid {
        blockers.push(format!("{} is already HEAD. Nothing to merge.", target));
    } else if head_oid != git2::Oid::ZERO_SHA1
        && repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        blockers.push(format!(
            "Current branch '{}' already contains '{}'. Nothing to merge.",
            current_branch, target
        ));
    }

    let title = if current_branch.is_empty() {
        format!("Merge {} into current branch", target)
    } else {
        format!("Merge {} into {}", target, current_branch)
    };
    let recovery = "If this merge is not wanted after execution, use git reflog to find the previous HEAD.\n\
         Fast-forward merges can be undone by moving the branch back; merge commits can be reverted with git revert -m 1 <merge-commit>.".to_string();

    let blocked_plan =
        |blockers: Vec<String>, warnings: Vec<String>, current: StateSummary| OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            title: PlanTitle::verbatim(title.clone()),
            current: current.clone(),
            predicted: StateSummary {
                head: current.head.clone(),
                dirty: current.dirty.clone(),
            },
            warnings: PlanNote::wrap_all(warnings),
            blockers: PlanNote::wrap_all(blockers),
            recovery: Some(PlanRecovery::verbatim(recovery.clone())),
            head_at_plan: head.clone(),
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        };

    if !blockers.is_empty() {
        return Ok((
            blocked_plan(blockers, warnings, current),
            MergeKind::MergeCommit,
        ));
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;
    let target_tree = target_commit
        .tree()
        .map_err(|e| GitError::Other(format!("target tree lookup failed: {}", e.message())))?;
    let can_ff = repo
        .graph_descendant_of(target_oid, head_oid)
        .unwrap_or(false);

    // `kind` records what execution will do; `predicted_dirty` mirrors the
    // post-merge working-tree state shown in the plan card.
    let mut kind = MergeKind::MergeCommit;
    let mut predicted_dirty = "clean".to_string();

    let (preview_files, predicted_head) = if can_ff {
        kind = MergeKind::FastForward;
        (
            preview_files_between_trees(repo, &head_tree, &target_tree)?,
            format!(
                "branch: {} (fast-forward to {} {})",
                current_branch,
                target,
                short_oid(target_oid)
            ),
        )
    } else {
        let mut index = repo
            .merge_commits(&head_commit, &target_commit, None)
            .map_err(|e| {
                GitError::Other(format!("merge_commits in-memory failed: {}", e.message()))
            })?;
        if index.has_conflicts() {
            // W31: a predicted conflict is a WARNING + confirm, not a blocker.
            // We still produce a useful preview from the conflicted merge index
            // (it carries the merged-with-markers tree for the resolvable paths)
            // so the user sees the scope before confirming.
            let conflict_files = conflict_paths_from_index(&mut index)?;
            let files_label = if conflict_files.is_empty() {
                "(unknown files)".to_string()
            } else {
                conflict_files.join(", ")
            };
            warnings.push(format!(
                "Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode.",
                conflict_files.len(),
                files_label
            ));
            predicted_dirty = format!(
                "{} conflicted file(s) (resolve in Conflict Mode)",
                conflict_files.len()
            );
            kind = MergeKind::Conflicts(conflict_files);
            (
                // Preview from the pre-merge head tree to the target tree so the
                // card shows what is coming in (the merge itself is not written
                // here; execution does the real merge).
                preview_files_between_trees(repo, &head_tree, &target_tree)?,
                format!(
                    "branch: {} (merge {} {} — with conflicts)",
                    current_branch,
                    target,
                    short_oid(target_oid)
                ),
            )
        } else {
            let new_tree_oid = index.write_tree_to(repo).map_err(|e| {
                GitError::Other(format!("index.write_tree_to failed: {}", e.message()))
            })?;
            let new_tree = repo.find_tree(new_tree_oid).map_err(|e| {
                GitError::Other(format!("find_tree for preview failed: {}", e.message()))
            })?;
            (
                preview_files_between_trees(repo, &head_tree, &new_tree)?,
                format!(
                    "branch: {} (+1 merge commit from {} {})",
                    current_branch,
                    target,
                    short_oid(target_oid)
                ),
            )
        }
    };

    if preview_files.is_empty() && !matches!(kind, MergeKind::Conflicts(_)) {
        blockers.push(format!("Merging '{}' would produce no changes.", target));
        return Ok((
            blocked_plan(blockers, warnings, current),
            MergeKind::MergeCommit,
        ));
    }

    Ok((
        OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            title: PlanTitle::verbatim(title),
            current,
            predicted: StateSummary {
                head: predicted_head,
                dirty: predicted_dirty,
            },
            warnings: PlanNote::wrap_all(warnings),
            blockers: PlanNote::wrap_all(blockers),
            recovery: Some(PlanRecovery::verbatim(recovery)),
            head_at_plan: head,
            stash_count_at_plan: 0,
            preview_files,
            preview_commits: Vec::new(),
            destructive: false,
        },
        kind,
    ))
}

/// Execute a branch merge planned by [`plan_merge_branch`].
///
/// Fast-forward execution checks out the target tree before moving the branch
/// ref. Non-fast-forward execution creates the merge commit without moving any
/// ref, checks out the merge tree, then advances the current branch.
pub fn execute_merge_branch(repo: &Repository, target: &str) -> Result<CommitId, GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is detached. Merge is only supported on a branch.".to_string(),
        ));
    }
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    let current_branch = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();

    if head_oid == target_oid
        || repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        return Err(GitError::Other(format!(
            "Current branch '{}' already contains '{}'. Re-plan before executing.",
            current_branch, target
        )));
    }

    if repo
        .graph_descendant_of(target_oid, head_oid)
        .unwrap_or(false)
    {
        let obj = target_commit.as_object();
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.safe();
        repo.checkout_tree(obj, Some(&mut cb)).map_err(|e| {
            GitError::Other(format!("checkout_tree (merge FF) failed: {}", e.message()))
        })?;
        let mut branch_ref = repo
            .find_reference(&refname)
            .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
        branch_ref
            .set_target(
                target_oid,
                &format!("merge: fast-forward {} into {}", target, current_branch),
            )
            .map_err(|e| {
                GitError::Other(format!(
                    "branch ref update (merge FF) failed: {}",
                    e.message()
                ))
            })?;
        repo.set_head(&refname)
            .map_err(|e| GitError::Other(format!("set_head (merge FF) failed: {}", e.message())))?;
        return Ok(CommitId(target_oid.to_string()));
    }

    let mut index = repo
        .merge_commits(&head_commit, &target_commit, None)
        .map_err(|e| GitError::Other(format!("merge_commits in-memory failed: {}", e.message())))?;
    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Merge of '{}' would produce conflicts. Re-plan before executing.",
            target
        )));
    }
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;
    let committer = build_signature(repo)?;
    let author = committer.clone();
    let merge_message = format!("Merge branch '{}' into {}", target, current_branch);
    let new_oid = repo
        .commit(
            None,
            &author,
            &committer,
            &merge_message,
            &new_tree,
            &[&head_commit, &target_commit],
        )
        .map_err(|e| GitError::Other(format!("merge commit creation failed: {}", e.message())))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!("checkout_tree after merge failed: {}", e.message()))
        })?;
    let mut branch_ref = repo
        .find_reference(&refname)
        .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
    branch_ref
        .set_target(
            new_oid,
            &format!("merge: {} into {}", target, current_branch),
        )
        .map_err(|e| {
            GitError::Other(format!("branch ref update (merge) failed: {}", e.message()))
        })?;
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head (merge) failed: {}", e.message())))?;

    Ok(CommitId(new_oid.to_string()))
}

/// Perform a **real** merge of `target` into the current branch that is expected
/// to conflict, leaving the repository in the standard git "merging with
/// conflicts" state (W31-MERGE-INTO-CONFLICT).
///
/// Unlike [`execute_merge_branch`], this does NOT create a commit. It uses
/// git2's real `Repository::merge` so the working tree gets conflict markers,
/// the index gets stage 1/2/3 entries, and `.git/MERGE_HEAD` is written — the
/// exact state [`crate::detect_conflict_session`] recognises and that
/// `plan_conflict_abort` / `execute_conflict_abort` can roll back.
///
/// `ORIG_HEAD` is written to the pre-merge HEAD so the conflict abort can
/// restore it (git2's `merge` does not write `ORIG_HEAD` itself). No force /
/// `reset --hard` / `clean` is used; the checkout is the default git merge
/// checkout. Returns the conflicted file paths from the index conflict iterator.
pub fn execute_merge_into_conflict(
    repo: &Repository,
    target: &str,
) -> Result<Vec<String>, GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is detached. Merge is only supported on a branch.".to_string(),
        ));
    }
    let current_branch = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;

    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();

    if head_oid == target_oid
        || repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        return Err(GitError::Other(format!(
            "Current branch '{}' already contains '{}'. Re-plan before executing.",
            current_branch, target
        )));
    }

    // Record the pre-merge HEAD so abort can restore it (git2::merge does not).
    repo.reference(
        "ORIG_HEAD",
        head_oid,
        true,
        &format!("merge {} into {}: ORIG_HEAD", target, current_branch),
    )
    .map_err(|e| GitError::Other(format!("write ORIG_HEAD failed: {}", e.message())))?;

    // Resolve the target as an annotated commit for the real merge.
    let annotated = repo
        .find_annotated_commit(target_oid)
        .map_err(|e| GitError::Other(format!("find_annotated_commit failed: {}", e.message())))?;

    // Default merge checkout: writes conflict markers + stage 1/2/3 index
    // entries + .git/MERGE_HEAD. No force / reset / clean.
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.safe();
    repo.merge(&[&annotated], None, Some(&mut checkout_opts))
        .map_err(|e| GitError::Other(format!("merge into conflict failed: {}", e.message())))?;

    // Collect the conflicted paths from the now-conflicted index.
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    let conflict_files = conflict_paths_from_index(&mut index)?;
    Ok(conflict_files)
}
