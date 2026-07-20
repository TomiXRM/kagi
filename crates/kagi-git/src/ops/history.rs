use super::*;

/// Analyse whether undoing the current HEAD commit is safe and return an
/// [`OperationPlan`].
///
/// # Design (ADR-0011)
///
/// Undo commit is a **ref-only** operation: the branch tip is moved to
/// `HEAD~1`.  The index and working tree are **never touched**.  This means
/// the changes that were in the undone commit end up **staged** (identical
/// to `git reset --soft HEAD~1`), and nothing is lost.
///
/// # Blocker conditions
///
/// - HEAD is detached or unborn.
/// - Repository is in a conflict state.
/// - HEAD is a merge commit (parent count > 1) — MVP limitation.
/// - HEAD is a root commit (no parent) — nothing to go back to.
/// - HEAD commit is reachable from its upstream tracking branch
///   (`graph_descendant_of(upstream, head)`) — the commit has been pushed,
///   so rewriting would diverge history.  If there is no upstream configured,
///   this check is skipped (local-only branch is always safe to undo).
///
/// # Warnings
///
/// *(none)*
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_undo_commit(repo: &Repository) -> Result<OperationPlan, GitError> {
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
        dirty: dirty_display.clone(),
    };

    // ── 3. Early structural blockers ─────────────────────────
    let mut blockers: Vec<String> = Vec::new();

    // Detached HEAD: no branch ref to move.
    if let Head::Detached { .. } = &head {
        blockers.push("HEAD is detached. Undo commit requires HEAD to be on a branch.".to_string());
    }

    // Unborn HEAD: no commits to undo.
    if let Head::Unborn { .. } = &head {
        blockers.push("HEAD is unborn (no commits exist). There is nothing to undo.".to_string());
    }

    // Conflict state: refuse to operate on a repo mid-conflict.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before undoing a commit.",
            status.conflicted.len()
        ));
    }

    // ── 4. Resolve HEAD commit (only when attached) ──────────
    // We need the commit to check parent count, merge status, and push status.
    let (head_commit_opt, branch_name_opt) = match &head {
        Head::Attached { target, branch } => {
            let oid = git2::Oid::from_str(target)
                .map_err(|e| GitError::Other(format!("HEAD OID parse failed: {}", e.message())))?;
            let commit = repo.find_commit(oid).map_err(|e| {
                GitError::Other(format!("HEAD commit lookup failed: {}", e.message()))
            })?;
            (Some(commit), Some(branch.clone()))
        }
        _ => (None, None),
    };

    // Commit-level blockers (only when we have a commit to examine).
    let mut head_short = String::new();
    let mut head_summary = String::new();
    let mut parent_oid_opt: Option<git2::Oid> = None;

    if let Some(ref commit) = head_commit_opt {
        let head_oid_str = commit.id().to_string();
        head_short = head_oid_str.get(..8).unwrap_or(&head_oid_str).to_string();
        // summary() returns Result<Option<&str>, Error> in git2 0.21.
        head_summary = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no message)")
            .chars()
            .take(72)
            .collect();

        // Merge commit check.
        if commit.parent_count() > 1 {
            blockers.push(format!(
                "Commit {} is a merge commit ({} parents). \
                 Undoing merge commits is not supported in MVP.",
                head_short,
                commit.parent_count()
            ));
        }

        // Root commit check.
        if commit.parent_count() == 0 {
            blockers.push(format!(
                "Commit {} is the root commit (no parent). There is nothing to go back to.",
                head_short
            ));
        }

        // Collect the parent OID for use in the plan and execute.
        if commit.parent_count() == 1 {
            parent_oid_opt = Some(
                commit
                    .parent_id(0)
                    .map_err(|e| GitError::Other(format!("parent_id failed: {}", e.message())))?,
            );
        }

        // Push-status check: is HEAD reachable from the upstream?
        // graph_descendant_of(a, b) returns true when a is a descendant of b
        // (i.e., b is reachable FROM a).  We want to know whether the upstream
        // tip can reach HEAD — meaning HEAD is an ancestor of upstream (or equal).
        // Equivalently: upstream is a descendant-or-equal of HEAD.
        // We test: graph_descendant_of(upstream_oid, head_oid) OR upstream==head.
        if let Some(branch_name) = &branch_name_opt {
            if let Ok(branch) = repo.find_branch(branch_name, BranchType::Local) {
                if let Ok(upstream) = branch.upstream() {
                    if let Some(upstream_oid) = upstream.get().target() {
                        let head_oid = commit.id();
                        // upstream == head: HEAD has been pushed.
                        let pushed = if upstream_oid == head_oid {
                            true
                        } else {
                            // upstream is a descendant of HEAD → HEAD is reachable from upstream.
                            repo.graph_descendant_of(upstream_oid, head_oid)
                                .unwrap_or(false)
                        };
                        if pushed {
                            blockers.push(format!(
                                "Commit {} has been pushed to the upstream tracking branch. \
                                 Undoing a pushed commit would rewrite published history, which is \
                                 not allowed. Use `git revert` to create an inverse commit instead.",
                                head_short
                            ));
                        }
                    }
                }
                // No upstream configured → local-only branch → always allowed.
            }
        }
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After undo: HEAD moves to parent; the previously-committed changes are
    // staged (index unchanged by this operation, WT unchanged too).
    let parent_short = parent_oid_opt
        .map(|oid| {
            let s = oid.to_string();
            s.get(..8).unwrap_or(&s).to_string()
        })
        .unwrap_or_else(|| "(parent)".to_string());

    let predicted_head = match &branch_name_opt {
        Some(b) => format!("branch: {} (at {})", b, parent_short),
        None => head_display.clone(),
    };

    // After the ref move the previously-committed changes become staged.
    let predicted_dirty = if dirty_parts.is_empty() {
        "staged (undone commit changes restored to index)".to_string()
    } else {
        format!(
            "{}, staged (undone commit changes restored to index)",
            dirty_parts.join(", ")
        )
    };

    let predicted = StateSummary {
        head: predicted_head,
        dirty: predicted_dirty,
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = if head_short.is_empty() {
        "Undo commit cannot proceed (see blockers above).".to_string()
    } else {
        format!(
            "The undone commit is NOT deleted — it remains in the object store and reflog.\n\
             To fully restore (re-commit with the same SHA):\n  git reset --soft {}\n\
             Changes from the undone commit will be staged immediately after undo.\n\
             The reflog records every HEAD movement:\n  git reflog",
            head_short
        )
    };

    // ── 7. Title ───────────────────────────────────────────────
    let title = if head_short.is_empty() {
        "Undo last commit (cannot proceed — see blockers)".to_string()
    } else {
        format!(
            "Undo commit {} '{}' — changes will be staged",
            head_short, head_summary
        )
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::verbatim(title),
        current,
        predicted,
        warnings: Vec::new(),
        blockers: PlanNote::wrap_all(blockers),
        recovery: Some(PlanRecovery::verbatim(recovery)),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_undo_commit  (T-HT-009)
// ────────────────────────────────────────────────────────────

/// Execute the undo-commit operation: move the current branch ref to `HEAD~1`.
///
/// # Design (ADR-0011)
///
/// **Ref-only operation.**  This function performs a single ref update:
///
/// ```text
/// repo.reference("refs/heads/<branch>", parent_oid, true, msg)
/// ```
///
/// No index operations, no working-tree operations, no `checkout` calls of
/// any kind are performed.  HEAD (the symbolic ref) is left pointing at the
/// same branch — which now resolves to the parent commit.  The changes from
/// the undone commit remain in the index in staged form, identical to the
/// result of `git reset --soft HEAD~1`.
///
/// # Errors
///
/// Returns [`GitError::Other`] if:
/// - HEAD is not attached to a branch.
/// - HEAD commit has no parent (root commit — guard in plan phase).
/// - HEAD commit is a merge commit (guard in plan phase).
/// - Any libgit2 ref-update failure.
pub fn execute_undo_commit(repo: &Repository) -> Result<UndoOutcome, GitError> {
    // ── 1. Resolve HEAD branch + commit ───────────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;

    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is not on a branch. Undo commit requires an attached HEAD.".to_string(),
        ));
    }

    let branch_refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD ref name failed: {}", e.message())))?
        .to_string();

    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 2. Guard: root commit ─────────────────────────────────
    if head_commit.parent_count() == 0 {
        return Err(GitError::Other(
            "HEAD is the root commit (no parent). Cannot undo.".to_string(),
        ));
    }

    // ── 3. Guard: merge commit ────────────────────────────────
    if head_commit.parent_count() > 1 {
        return Err(GitError::Other(format!(
            "HEAD is a merge commit ({} parents). Undoing merge commits is not supported.",
            head_commit.parent_count()
        )));
    }

    // ── 4. Resolve the single parent ─────────────────────────
    let parent_oid = head_commit
        .parent_id(0)
        .map_err(|e| GitError::Other(format!("parent_id failed: {}", e.message())))?;

    // ── 5. Move the branch ref — ref-only, no index/WT touch ──
    // force=true overwrites the existing ref (safe: we just validated the
    // ancestry above).  HEAD (symbolic) is not touched — it still points to
    // the same branch name; the branch now resolves to the parent.
    let log_msg = format!(
        "undo-commit: move {} from {} to {}",
        branch_refname,
        &head_oid.to_string()[..8],
        &parent_oid.to_string()[..8],
    );
    repo.reference(&branch_refname, parent_oid, true, &log_msg)
        .map_err(|e| {
            GitError::Other(format!(
                "branch ref update (undo-commit) failed: {}",
                e.message()
            ))
        })?;

    Ok(UndoOutcome {
        undone: CommitId(head_oid.to_string()),
        now_at: CommitId(parent_oid.to_string()),
    })
}

// ────────────────────────────────────────────────────────────
// Amend  (T-COMMIT-010, ADR-0040 — MVP: unpushed only)
// ────────────────────────────────────────────────────────────

/// Analyse whether amending the current HEAD commit is safe and return an
/// [`OperationPlan`] (ADR-0040, MVP = **unpushed only**).
///
/// # Design (ADR-0040)
///
/// Amend never uses `commit.amend(...)`.  Instead [`execute_amend`] builds a new
/// commit object whose **parent is the old HEAD's parent** (HEAD is replaced, so
/// the parent is left in place) and then moves the branch ref last (ref-order
/// rule).  The working tree and index are not written to.  The new SHA always
/// differs from the old one; this is surfaced as `predicted` with an explicit
/// `旧 <short> → 新 <short>` line and `destructive: true` (ADR-0023 two-stage
/// confirm).
///
/// # Blocker conditions
///
/// - HEAD is detached or unborn.
/// - Repository is in a conflict state.
/// - HEAD is a **merge commit** (parent count > 1) — not supported.
/// - HEAD is a **root commit** (no parent) when folding staged changes is not
///   possible — root-commit amend keeps the single-parent invariant simple, so
///   it is refused in MVP.
/// - HEAD commit has been **pushed** to its upstream (ADR-0040 案B) — amending
///   published history is refused; commit a new fixup instead.
/// - `Staged` / `Both`: nothing is staged (nothing to fold in).
/// - `MessageOnly` / `Both`: the new message is empty.
/// - Checklist (ADR-0043) blockers, run over the staged contents.
///
/// `message` is required for `MessageOnly` / `Both` and ignored for `Staged`.
pub fn plan_amend(
    repo: &Repository,
    mode: AmendMode,
    message: Option<&str>,
) -> Result<OperationPlan, GitError> {
    // ── 1. Resolve HEAD + status ─────────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut blockers: Vec<String> = Vec::new();
    // `warnings` stays empty in MVP; the checklist lane (ADR-0043) will push into
    let mut warnings: Vec<String> = Vec::new();

    // ── 2. Structural blockers (HEAD shape) ──────────────────
    if let Head::Detached { .. } = &head {
        blockers.push("HEAD is detached. Amend requires HEAD to be on a branch.".to_string());
    }
    if let Head::Unborn { .. } = &head {
        blockers.push("HEAD is unborn (no commits exist). There is nothing to amend.".to_string());
    }
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before amending.",
            status.conflicted.len()
        ));
    }

    // ── 3. Resolve HEAD commit (only when attached) ──────────
    let (head_commit_opt, branch_name_opt) = match &head {
        Head::Attached { target, branch } => {
            let oid = git2::Oid::from_str(target)
                .map_err(|e| GitError::Other(format!("HEAD OID parse failed: {}", e.message())))?;
            let commit = repo.find_commit(oid).map_err(|e| {
                GitError::Other(format!("HEAD commit lookup failed: {}", e.message()))
            })?;
            (Some(commit), Some(branch.clone()))
        }
        _ => (None, None),
    };

    let mut old_short = String::new();
    let mut old_summary = String::new();

    if let Some(ref commit) = head_commit_opt {
        let old_oid_str = commit.id().to_string();
        old_short = old_oid_str.get(..8).unwrap_or(&old_oid_str).to_string();
        old_summary = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no message)")
            .chars()
            .take(72)
            .collect();

        // Merge commit: refuse.
        if commit.parent_count() > 1 {
            blockers.push(format!(
                "Commit {} is a merge commit ({} parents). Amending merge commits is not supported.",
                old_short,
                commit.parent_count()
            ));
        }

        // Root commit: refuse (keeps the single-parent invariant of MVP amend).
        if commit.parent_count() == 0 {
            blockers.push(format!(
                "Commit {} is the root commit (no parent). Amending the root commit is not supported in MVP.",
                old_short
            ));
        }

        // Pushed check (ADR-0040 案B): refuse if HEAD is reachable from upstream.
        // Mirrors plan_undo_commit's graph_descendant_of(upstream, head) test.
        if let Some(branch_name) = &branch_name_opt {
            if let Ok(branch) = repo.find_branch(branch_name, BranchType::Local) {
                if let Ok(upstream) = branch.upstream() {
                    if let Some(upstream_oid) = upstream.get().target() {
                        let head_oid = commit.id();
                        let pushed = upstream_oid == head_oid
                            || repo
                                .graph_descendant_of(upstream_oid, head_oid)
                                .unwrap_or(false);
                        if pushed {
                            blockers.push(format!(
                                "Commit {} has been pushed to its upstream tracking branch. \
                                 Amending published history is not allowed (ADR-0040). \
                                 Create a new commit to make the correction instead.",
                                old_short
                            ));
                        }
                    }
                }
                // No upstream → local-only branch → always allowed.
            }
        }
    }

    // ── 4. Mode-specific input blockers ──────────────────────
    let new_message = message.unwrap_or("");
    if mode.replaces_message() && new_message.trim().is_empty() {
        blockers.push("Commit message must not be empty.".to_string());
    }
    if mode.includes_staged() && status.staged.is_empty() {
        blockers.push(
            "Nothing staged to fold into the commit. Stage changes first, or use \
             message-only amend."
                .to_string(),
        );
    }

    // ── 5. Checklist (ADR-0039 / 0043) — same rules as plan_commit ────
    // Only meaningful when staged content is being folded in; message-only
    // amends keep the old tree, so there is nothing new to scan.
    if mode.includes_staged() {
        let (cl_blockers, cl_warnings) = crate::checklist::checklist(repo, &status)?;
        blockers.extend(cl_blockers);
        warnings.extend(cl_warnings);
    }

    // ── 6. Predicted state (SHA change is the headline) ──────
    let predicted_head = if old_short.is_empty() {
        current.head.clone()
    } else {
        // SHA is only known after execute; we predict "new" as a placeholder
        // because the new OID depends on tree+committer.  The旧→新 transition is
        // spelled out in the dirty line and recovery text.
        match &branch_name_opt {
            Some(b) => format!("branch: {} (amended commit, new SHA)", b),
            None => current.head.clone(),
        }
    };

    let predicted_dirty = if old_short.is_empty() {
        dirty_display.clone()
    } else {
        let mode_label = match mode {
            AmendMode::MessageOnly => "message rewritten",
            AmendMode::Staged => "staged changes folded in",
            AmendMode::Both => "staged changes folded in + message rewritten",
        };
        // ADR-0040: explicit旧 <short> → 新 <short> (new short is unknown pre-execute).
        format!("旧 {} → 新 <new> ({})", old_short, mode_label)
    };

    let predicted = StateSummary {
        head: predicted_head,
        dirty: predicted_dirty,
    };

    // ── 7. Recovery + title ──────────────────────────────────
    let recovery = if old_short.is_empty() {
        "Amend cannot proceed (see blockers above).".to_string()
    } else {
        format!(
            "Amend rewrites history: the new commit gets a NEW SHA and the old commit \
             {} becomes unreachable from the branch (but stays in the reflog).\n\
             To restore the original commit:\n  git reset --hard {}\n\
             The reflog records every HEAD movement:\n  git reflog",
            old_short, old_short
        )
    };

    let title = if old_short.is_empty() {
        "Amend last commit (cannot proceed — see blockers)".to_string()
    } else {
        let mode_label = match mode {
            AmendMode::MessageOnly => "message only",
            AmendMode::Staged => "fold staged",
            AmendMode::Both => "fold staged + message",
        };
        format!(
            "Amend commit {} '{}' ({}) — SHA will change",
            old_short, old_summary, mode_label
        )
    };

    // Preview the staged files that will be folded in (Staged / Both only).
    let preview_files: Vec<FileStatus> = if mode.includes_staged() {
        status.staged.clone()
    } else {
        Vec::new()
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::verbatim(title),
        current,
        predicted,
        warnings: PlanNote::wrap_all(warnings),
        blockers: PlanNote::wrap_all(blockers),
        recovery: Some(PlanRecovery::verbatim(recovery)),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: true,
    })
}

/// Execute an amend (ADR-0040): build a new commit and move the branch ref.
///
/// # Design (ADR-0040, in-memory + ref-order rule)
///
/// 1. parent = the old HEAD commit's **parent** (HEAD is replaced).
/// 2. tree:
///    - `MessageOnly` → the old HEAD's tree (unchanged).
///    - `Staged` / `Both` → `index.write_tree_to(repo)` — an **in-memory** tree
///      from the current index, without touching the working tree.
/// 3. `repo.commit(None, ...)` creates the commit object **without** moving any
///    ref.
/// 4. Only after the object exists, `repo.reference(...)` moves the branch ref
///    last (ref-order rule).  The working tree / index are never written.
///
/// The **author is preserved** from the old commit; the **committer is updated**
/// to the current signature/time (matching git's amend default).
///
/// The caller is responsible for recording the old HEAD SHA in the oplog
/// **before** calling this (ADR-0040).
pub fn execute_amend(
    repo: &Repository,
    mode: AmendMode,
    message: Option<&str>,
) -> Result<AmendOutcome, GitError> {
    // ── 1. Resolve HEAD branch ref + commit ──────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is not on a branch. Amend requires an attached HEAD.".to_string(),
        ));
    }
    let branch_refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD ref name failed: {}", e.message())))?
        .to_string();
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 2. Guards (defence — plan already checks these) ──────
    if head_commit.parent_count() > 1 {
        return Err(GitError::Other(format!(
            "HEAD is a merge commit ({} parents). Amending merge commits is not supported.",
            head_commit.parent_count()
        )));
    }
    if head_commit.parent_count() == 0 {
        return Err(GitError::Other(
            "HEAD is the root commit (no parent). Amending the root commit is not supported."
                .to_string(),
        ));
    }

    // ── 3. Parent stays put (amend replaces HEAD, not its parent) ──
    let parent_oid = head_commit
        .parent_id(0)
        .map_err(|e| GitError::Other(format!("parent_id failed: {}", e.message())))?;
    let parent_commit = repo
        .find_commit(parent_oid)
        .map_err(|e| GitError::Other(format!("parent commit lookup failed: {}", e.message())))?;

    // ── 4. Resolve the tree ──────────────────────────────────
    let tree = if mode.includes_staged() {
        // In-memory tree from the current index — no working-tree write.
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        if index.has_conflicts() {
            return Err(GitError::Other(
                "Index has conflicts; resolve them before amending.".to_string(),
            ));
        }
        let tree_oid = index
            .write_tree_to(repo)
            .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
        repo.find_tree(tree_oid)
            .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?
    } else {
        // Message-only amend keeps the old HEAD's tree verbatim.
        head_commit
            .tree()
            .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?
    };

    // ── 5. Author preserved / committer updated ──────────────
    let author = head_commit.author();
    let committer = build_signature(repo)?;

    // ── 6. Message ───────────────────────────────────────────
    let new_message: String = if mode.replaces_message() {
        match message {
            Some(m) if !m.trim().is_empty() => m.to_string(),
            _ => {
                return Err(GitError::Other(
                    "Commit message must not be empty.".to_string(),
                ))
            }
        }
    } else {
        head_commit.message().unwrap_or("(no message)").to_string()
    };

    // ── 7. Create the commit object WITHOUT moving any ref ───
    let new_oid = repo
        .commit(
            None,
            &author,
            &committer,
            &new_message,
            &tree,
            &[&parent_commit],
        )
        .map_err(|e| GitError::Other(format!("amend commit creation failed: {}", e.message())))?;

    // ── 8. Move the branch ref LAST (ref-order rule) ─────────
    let log_msg = format!(
        "amend: {} {} -> {}",
        branch_refname,
        &head_oid.to_string()[..8],
        &new_oid.to_string()[..8],
    );
    repo.reference(&branch_refname, new_oid, true, &log_msg)
        .map_err(|e| {
            GitError::Other(format!("branch ref update (amend) failed: {}", e.message()))
        })?;

    Ok(AmendOutcome {
        old: CommitId(head_oid.to_string()),
        new: CommitId(new_oid.to_string()),
    })
}

// ────────────────────────────────────────────────────────────
// plan_delete_branch  (W2-DELETE, ADR-0014)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// Operation Undo / Redo  (T-UNDOREDO-001, ADR-0081)
// ────────────────────────────────────────────────────────────
//
// GitKraken-style Undo/Redo of ref-moving operations (commit, merge, …).
// Both directions reduce to a SAFE branch-ref move between two SHAs that stay
// reachable via the reflog/ODB — no commit is ever destroyed, and `reset
// --hard`/clean/force are never used (ADR-0023).
//
//   undo:  move `entry.branch` from `entry.after`  back to `entry.before`
//   redo:  move `entry.branch` from `entry.before` forward to `entry.after`
//
// The move is a MIXED-style reset: update the branch ref via libgit2
// `reference(...)`, then point the index at the target commit's tree
// (`index.read_tree`) WITHOUT touching the working tree. Any uncommitted
// working-tree edits survive unchanged. For merge-commit undo this still holds
// — the merge commit remains in the reflog, and the working tree is left as the
// user left it.

/// The outcome of an undo/redo ref move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMoveOutcome {
    /// The branch whose ref was moved.
    pub branch: String,
    /// The SHA the branch pointed at before the move.
    pub from: CommitId,
    /// The SHA the branch points at after the move (the target).
    pub to: CommitId,
}

/// Build a `current` [`StateSummary`] plus the dirty parts for plan rendering.
fn undo_redo_state(repo: &Repository) -> Result<(StateSummary, Vec<String>), GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();
    let dirty = if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    };
    Ok((
        StateSummary {
            head: head.display(),
            dirty,
        },
        dirty_parts,
    ))
}

/// Shared planner for undo/redo: plan a move of `branch` from `from` → `to`.
///
/// `label` is a human verb ("Undo"/"Redo"); `kind_slug` is the operation kind.
/// Blockers are raised when the move cannot be performed safely:
/// - the branch is not the current HEAD branch (MVP: only the checked-out branch),
/// - the branch ref is no longer at `from` (stale entry — external change),
/// - the target `to` is unknown / unreachable in the ODB,
/// - the repo is mid-conflict.
///
/// A WARNING (not a blocker) is surfaced when the working tree is dirty: those
/// changes are preserved (mixed reset) but the user should know the move happens
/// underneath them.
fn plan_history_move(
    repo: &Repository,
    label: &str,
    kind_slug: &str,
    branch: &str,
    from: &CommitId,
    to: &CommitId,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let (current, _dirty_parts) = undo_redo_state(repo)?;
    let status = working_tree_status(repo)?;

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Only support the currently checked-out branch in MVP (the ref move must
    // not strand a different branch's working tree).
    match &head {
        Head::Attached { branch: cur, .. } if cur == branch => {}
        Head::Attached { branch: cur, .. } => {
            blockers.push(format!(
                "Operation was on branch '{}', but the current branch is '{}'. \
                 Switch back to '{}' to {} it.",
                branch,
                cur,
                branch,
                label.to_lowercase()
            ));
        }
        _ => {
            blockers.push(format!(
                "HEAD is not on a branch. {} requires the operation's branch to be checked out.",
                label
            ));
        }
    }

    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before {}.",
            status.conflicted.len(),
            label.to_lowercase()
        ));
    }

    // Stale check: branch must currently be at `from`.
    let from_oid = git2::Oid::from_str(&from.0)
        .map_err(|e| GitError::Other(format!("bad 'from' SHA {}: {}", from.0, e.message())))?;
    let to_oid = git2::Oid::from_str(&to.0)
        .map_err(|e| GitError::Other(format!("bad 'to' SHA {}: {}", to.0, e.message())))?;

    if let Ok(branch_ref) = repo.find_branch(branch, BranchType::Local) {
        match branch_ref.get().target() {
            Some(cur_oid) if cur_oid == from_oid => {}
            Some(cur_oid) => {
                blockers.push(format!(
                    "Branch '{}' has moved since this operation (now at {}, expected {}). \
                     This history entry is stale and will be skipped.",
                    branch,
                    &cur_oid.to_string()[..8],
                    &from_oid.to_string()[..8],
                ));
            }
            None => blockers.push(format!("Branch '{}' has no target commit.", branch)),
        }
    } else {
        blockers.push(format!("Branch '{}' no longer exists.", branch));
    }

    // Target must be reachable in the ODB.
    if repo.find_commit(to_oid).is_err() {
        blockers.push(format!(
            "Target commit {} is no longer reachable in the object store. \
             This history entry is stale and will be skipped.",
            &to_oid.to_string()[..8],
        ));
    }

    // Dirty working tree → preserved, but warn.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        warnings.push(
            "You have uncommitted changes. They will be preserved verbatim; \
             only the branch ref moves (soft reset — index and working tree untouched)."
                .to_string(),
        );
    }

    let from_short = from.short();
    let to_short = to.short();

    let predicted = StateSummary {
        head: match &head {
            Head::Attached { branch: b, .. } => format!("branch: {} (at {})", b, to_short),
            other => other.display(),
        },
        dirty: format!(
            "soft move to {} (index untouched → the move's diff shows STAGED; \
             working-tree changes preserved)",
            to_short
        ),
    };

    let recovery = format!(
        "{} moves branch '{}' from {} to {} via a safe ref move (no reset --hard, no clean). \
         The {} commit is NOT deleted — it stays in the object store and reflog:\n  git reflog\n\
         To restore manually:\n  git update-ref refs/heads/{} {}",
        label, branch, from_short, to_short, kind_slug, branch, from.0
    );

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::verbatim(format!(
            "{} {} on '{}' — {} → {}",
            label, kind_slug, branch, from_short, to_short
        )),
        current,
        predicted,
        warnings: PlanNote::wrap_all(warnings),
        blockers: PlanNote::wrap_all(blockers),
        recovery: Some(PlanRecovery::verbatim(recovery)),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Plan an **undo** of a recorded operation: move `branch` back from `after`
/// to `before`. See [`plan_history_move`].
pub fn plan_undo(
    repo: &Repository,
    kind_slug: &str,
    branch: &str,
    before: &CommitId,
    after: &CommitId,
) -> Result<OperationPlan, GitError> {
    plan_history_move(repo, "Undo", kind_slug, branch, after, before)
}

/// Plan a **redo** of a recorded operation: move `branch` forward from
/// `before` to `after`. See [`plan_history_move`].
pub fn plan_redo(
    repo: &Repository,
    kind_slug: &str,
    branch: &str,
    before: &CommitId,
    after: &CommitId,
) -> Result<OperationPlan, GitError> {
    plan_history_move(repo, "Redo", kind_slug, branch, before, after)
}

/// Perform the safe ref move shared by undo and redo.
///
/// PREFLIGHT: re-verifies `branch` is the current HEAD branch, is still at
/// `from`, and `to` is reachable — stale entries are rejected with a clear
/// message rather than corrupting state.
///
/// MOVE (soft, never `--hard`, never `--mixed`):
/// 1. `repo.reference(refs/heads/<branch>, to, true, msg)` — move the branch ref.
///
/// The index and working tree are **never** touched (ADR-0084, `git reset
/// --soft` semantics). After undoing a commit, HEAD is at the parent but the
/// index still holds the commit's tree, so the commit's changes reappear
/// **staged**. Uncommitted working-tree edits are always preserved.
///
/// VERIFY: HEAD now resolves to `to`.
fn execute_history_move(
    repo: &Repository,
    label: &str,
    branch: &str,
    from: &CommitId,
    to: &CommitId,
) -> Result<HistoryMoveOutcome, GitError> {
    let from_oid = git2::Oid::from_str(&from.0)
        .map_err(|e| GitError::Other(format!("bad 'from' SHA {}: {}", from.0, e.message())))?;
    let to_oid = git2::Oid::from_str(&to.0)
        .map_err(|e| GitError::Other(format!("bad 'to' SHA {}: {}", to.0, e.message())))?;

    // ── PREFLIGHT ────────────────────────────────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(format!(
            "HEAD is not on a branch. {} requires the operation's branch to be checked out.",
            label
        )));
    }
    let head_branch = head_ref
        .shorthand()
        .ok()
        .ok_or_else(|| GitError::Other("HEAD has no branch name".to_string()))?;
    if head_branch != branch {
        return Err(GitError::Other(format!(
            "Stale history entry: operation was on '{}', current branch is '{}'. Skipped.",
            branch, head_branch
        )));
    }

    let branch_ref = repo
        .find_branch(branch, BranchType::Local)
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", branch, e.message())))?;
    let cur_oid = branch_ref
        .get()
        .target()
        .ok_or_else(|| GitError::Other(format!("branch '{}' has no target", branch)))?;
    if cur_oid != from_oid {
        return Err(GitError::Other(format!(
            "Stale history entry: branch '{}' is at {} but expected {}. Skipped.",
            branch,
            &cur_oid.to_string()[..8],
            &from_oid.to_string()[..8],
        )));
    }

    // Reachability check only — the target commit must exist. We never read its
    // tree into the index (soft semantics: the index is left untouched).
    repo.find_commit(to_oid).map_err(|e| {
        GitError::Other(format!(
            "Stale history entry: target {} unreachable: {}",
            &to_oid.to_string()[..8],
            e.message()
        ))
    })?;

    // ── MOVE: branch ref only (soft — never --mixed, never --hard) ─
    let branch_refname = format!("refs/heads/{}", branch);
    let log_msg = format!(
        "{}: move {} from {} to {}",
        label.to_lowercase(),
        branch,
        &from_oid.to_string()[..8],
        &to_oid.to_string()[..8],
    );
    repo.reference(&branch_refname, to_oid, true, &log_msg)
        .map_err(|e| {
            GitError::Other(format!(
                "branch ref move ({}) failed: {}",
                label,
                e.message()
            ))
        })?;

    // NOTE: the index and working tree are intentionally left untouched (soft
    // reset). After undoing a commit, the index still holds that commit's tree,
    // so its changes show up as STAGED — exactly `git reset --soft HEAD@{1}`.

    // ── VERIFY: HEAD resolves to the target ──────────────────
    let new_head = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .ok_or_else(|| GitError::Other("HEAD lookup after move failed".to_string()))?;
    if new_head != to_oid {
        return Err(GitError::Other(format!(
            "{} verify failed: HEAD is {} but expected {}.",
            label,
            &new_head.to_string()[..8],
            &to_oid.to_string()[..8],
        )));
    }

    Ok(HistoryMoveOutcome {
        branch: branch.to_string(),
        from: from.clone(),
        to: to.clone(),
    })
}

/// Execute an **undo**: move `branch` back from `after` to `before`.
pub fn execute_undo(
    repo: &Repository,
    branch: &str,
    before: &CommitId,
    after: &CommitId,
) -> Result<HistoryMoveOutcome, GitError> {
    execute_history_move(repo, "Undo", branch, after, before)
}

/// Execute a **redo**: move `branch` forward from `before` to `after`.
pub fn execute_redo(
    repo: &Repository,
    branch: &str,
    before: &CommitId,
    after: &CommitId,
) -> Result<HistoryMoveOutcome, GitError> {
    execute_history_move(repo, "Redo", branch, before, after)
}

/// Maximum number of reflog entries to seed the undo/redo history with.
const REFLOG_SEED_MAX: usize = 50;

/// Infer the [`OperationKind`] of a ref move from its reflog message prefix
/// (ADR-0084). git writes a stable prefix per operation (e.g. `"commit:"`,
/// `"commit (amend):"`, `"merge feature:"`). The undo/redo mechanics are
/// identical for every kind — this only tailors the preview label.
fn infer_kind_from_reflog(msg: &str) -> kagi_domain::history::OperationKind {
    use kagi_domain::history::OperationKind;
    // Order matters: the more-specific "commit (...)" prefixes must be checked
    // before the bare "commit" prefix.
    if msg.starts_with("commit (amend)") {
        OperationKind::Amend
    } else if msg.starts_with("commit (merge)") || msg.starts_with("merge") {
        OperationKind::Merge
    } else if msg.starts_with("revert") {
        OperationKind::Revert
    } else if msg.starts_with("cherry-pick") {
        OperationKind::CherryPick
    } else if msg.starts_with("commit") {
        OperationKind::Commit
    } else if msg.starts_with("reset") {
        // A reset (incl. our own soft undo) is a generic ref move.
        OperationKind::UndoCommit
    } else {
        OperationKind::Commit
    }
}

/// Read the current branch's reflog and build an undo/redo history seed
/// (ADR-0084 §2). Returns entries **oldest → newest** (the order
/// [`kagi_domain::history::OperationHistory::seeded`] expects: cursor = len so
/// `peek_undo` targets the most-recent ref move).
///
/// The reflog of `refs/heads/<branch>` records every ref move as
/// `(old_oid, new_oid, message)`, newest-first. We:
/// - skip no-op entries where `old == new`,
/// - keep only the **leading chained run** (`entry[i].before == entry[i+1].after`
///   in newest-first order) so unrelated branch noise / GC boundaries can't leak
///   in, stopping at the first break or after [`REFLOG_SEED_MAX`] entries,
/// - then reverse into oldest→newest for `seeded`.
///
/// On a detached HEAD (no branch) this returns an empty Vec.
pub fn history_from_reflog(
    repo: &Repository,
) -> Result<Vec<kagi_domain::history::HistoryEntry>, GitError> {
    use kagi_domain::history::HistoryEntry;

    // Resolve the current branch short name; bail (empty) if HEAD is detached.
    let head_ref = match repo.head() {
        Ok(h) => h,
        Err(_) => return Ok(Vec::new()),
    };
    if !head_ref.is_branch() {
        return Ok(Vec::new());
    }
    let branch = match head_ref.shorthand() {
        Ok(b) => b.to_string(),
        Err(_) => return Ok(Vec::new()),
    };

    let refname = format!("refs/heads/{}", branch);
    let reflog = match repo.reflog(&refname) {
        Ok(r) => r,
        // No reflog (e.g. brand-new branch) → nothing to seed.
        Err(_) => return Ok(Vec::new()),
    };

    // Collect chained entries newest-first.
    let mut newest_first: Vec<HistoryEntry> = Vec::new();
    let mut expected_after: Option<git2::Oid> = None;
    for i in 0..reflog.len() {
        if newest_first.len() >= REFLOG_SEED_MAX {
            break;
        }
        let entry = match reflog.get(i) {
            Some(e) => e,
            None => break,
        };
        let old = entry.id_old();
        let new = entry.id_new();
        // Skip no-op moves (old == new); they carry no undoable diff.
        if old == new {
            continue;
        }
        // Enforce the chain: each entry's `after` must equal the previous
        // (newer) entry's `before`. Stop at the first break.
        if let Some(exp) = expected_after {
            if new != exp {
                break;
            }
        }
        let msg = entry.message().ok().flatten().unwrap_or("");
        let after_short = new.to_string();
        let after_short = &after_short[..after_short.len().min(8)];
        newest_first.push(HistoryEntry {
            kind: infer_kind_from_reflog(msg),
            branch: branch.clone(),
            before: CommitId(old.to_string()),
            after: CommitId(new.to_string()),
            summary: if msg.is_empty() {
                format!("{} {}", branch, after_short)
            } else {
                format!("{} {}", after_short, msg)
            },
        });
        expected_after = Some(old);
    }

    // `seeded` wants oldest → newest (cursor = len).
    newest_first.reverse();
    Ok(newest_first)
}
