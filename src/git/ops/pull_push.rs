use super::*;

// ────────────────────────────────────────────────────────────
// plan_checkout_tracking_branch / execute_checkout_tracking_branch (T-BCM-061)
// ────────────────────────────────────────────────────────────

/// Default local branch name for a remote-tracking branch display name.
pub fn default_tracking_branch_name(remote_branch: &str) -> String {
    remote_branch
        .split_once('/')
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| remote_branch.to_string())
}

/// Plan creation of a local tracking branch from `remote_branch`, followed by
/// checking it out as one confirmed operation.
pub fn plan_checkout_tracking_branch(
    repo: &Repository,
    remote_branch: &str,
    local_branch: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if local_branch.trim().is_empty() {
        blockers.push("Local branch name is empty.".to_string());
    }
    if repo.find_branch(local_branch, BranchType::Local).is_ok() {
        blockers.push(format!("Local branch '{}' already exists.", local_branch));
    }
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before checkout.",
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
        blockers.push(format!(
            "Working tree has {} — stash or commit changes before checkout.",
            parts.join(", ")
        ));
        warnings.push("Suggested command: git stash push -u".to_string());
    }
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain after checkout.",
            status.untracked.len()
        ));
    }

    let remote_commit = resolve_branch_commit(repo, remote_branch)?;
    let predicted = StateSummary {
        head: format!("branch: {} (tracks {})", local_branch, remote_branch),
        dirty: current.dirty.clone(),
    };
    let recovery = format!(
        "If checkout succeeds but you do not want the branch, switch back and delete it:\n  git checkout -\n  git branch -d {}",
        local_branch
    );

    Ok(OperationPlan {
        title: format!(
            "Checkout {} as local branch {}",
            remote_branch, local_branch
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: vec![format!(
            "{}  {}",
            short_oid(remote_commit.id()),
            remote_branch
        )],
        destructive: false,
    })
}

/// Create a local branch tracking `remote_branch` and check it out.
pub fn execute_checkout_tracking_branch(
    repo: &Repository,
    remote_branch: &str,
    local_branch: &str,
) -> Result<(), GitError> {
    if repo.find_branch(local_branch, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "Local branch '{}' already exists.",
            local_branch
        )));
    }
    let remote_commit = resolve_branch_commit(repo, remote_branch)?;
    let mut branch = repo
        .branch(local_branch, &remote_commit, false)
        .map_err(|e| GitError::Other(format!("branch create failed: {}", e.message())))?;
    branch
        .set_upstream(Some(remote_branch))
        .map_err(|e| GitError::Other(format!("set upstream failed: {}", e.message())))?;

    let obj = remote_commit.as_object();
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;
    let refname = format!("refs/heads/{}", local_branch);
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head failed: {}", e.message())))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_revert / execute_revert  (T-CM-034)
// ────────────────────────────────────────────────────────────

/// Analyse whether a pull is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0009 Guarded policy)
///
/// - HEAD is detached or unborn.
/// - No upstream is configured for the current branch.
/// - Repository is in a conflict state.
/// - Working tree has staged or unstaged changes (dirty).
/// - (Plan-time) in-memory merge with the current upstream tip predicts a
///   conflict — shown as a **warning** at plan time (fetch may change things)
///   but still allows execution (the execute phase re-checks after fetch).
///
/// # Warnings
///
/// - The behind count shown is local knowledge; fetch may reveal more commits.
/// - Untracked files exist (they are not touched by merge/FF).
/// - Plan-time in-memory merge predicts a conflict (warning, not blocker —
///   re-evaluated after fetch).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_pull(repo: &Repository) -> Result<OperationPlan, GitError> {
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

    // Detached HEAD: no branch to advance.
    if let Head::Detached { .. } = &head {
        blockers
            .push("HEAD is detached. Pull is only supported when HEAD is on a branch.".to_string());
    }

    // Unborn HEAD: no commits exist yet.
    if let Head::Unborn { .. } = &head {
        blockers.push(
            "HEAD is unborn (no commits exist). Cannot pull onto an empty branch.".to_string(),
        );
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before pulling.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged / unstaged) — Guarded policy.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree has {} — stash your changes before pulling.",
            parts.join(", ")
        ));
    }

    // Untracked files — warning only.
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched after pull.",
            status.untracked.len()
        ));
    }

    // ── 4. Resolve upstream (only when HEAD is attached) ─────
    let (branch_name, remote_name, behind_count) = if let Head::Attached { branch, .. } = &head {
        match resolve_upstream_info(repo, branch) {
            Ok(info) => info,
            Err(e) => {
                blockers.push(format!(
                    "No upstream configured for branch '{}': {}. \
                     Set one with `git branch --set-upstream-to=<remote>/<branch>`.",
                    branch, e
                ));
                (branch.clone(), String::new(), 0usize)
            }
        }
    } else {
        // Blockers already added above; use dummy values.
        (String::new(), String::new(), 0usize)
    };

    // ── 5. Plan-time in-memory conflict prediction ───────────
    // Only if we have no blockers yet and upstream is resolvable.
    if blockers.is_empty() && !branch_name.is_empty() {
        if let Ok(conflict_warning) = predict_merge_conflict(repo, &branch_name, &remote_name) {
            if let Some(w) = conflict_warning {
                warnings.push(w);
            }
        }
    }

    // ── 6. Predicted StateSummary ─────────────────────────────
    let behind_label = if behind_count == 0 {
        "up to date (local knowledge; fetch may reveal more)".to_string()
    } else {
        format!(
            "{} behind upstream (local knowledge; fetch may reveal more)",
            behind_count
        )
    };

    let predicted = StateSummary {
        head: format!("branch: {}", branch_name),
        dirty: "clean".to_string(),
    };

    // ── 7. Recovery guidance ──────────────────────────────────
    let recovery = "Pull is non-destructive: fast-forward and clean merges do not lose work.\n\
         If the merge would conflict, execute is blocked and the repo remains untouched.\n\
         To undo a merge commit after execution:\n  git reset --hard HEAD~1\n\
         The reflog records every HEAD movement:\n  git reflog"
        .to_string();

    Ok(OperationPlan {
        title: format!(
            "Pull '{}' from '{}'  ({})",
            branch_name, remote_name, behind_label
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

// ────────────────────────────────────────────────────────────
// execute_pull  (T-HT-003)
// ────────────────────────────────────────────────────────────

/// Execute a pull: `git fetch <remote>` (CLI) then merge or fast-forward
/// (in-memory, never sets MERGING state).
///
/// # Steps
///
/// 1. Resolve the upstream remote name from the current branch config.
/// 2. Run `git fetch <remote>` via the CLI wrapper (60 s timeout).
///    Failure → `GitError::Other` with the full stderr.
/// 3. Re-resolve the upstream tip after fetch.
///    If HEAD OID == upstream tip or HEAD is a descendant → `UpToDate`.
/// 4. If HEAD is an ancestor of upstream tip (fast-forward possible):
///    - Advance the branch ref to the upstream tip.
///    - `checkout_tree` (safe) + `set_head` to sync the WT.
///    → `FastForward { to }`.
/// 5. Otherwise (diverged):
///    - `repo.merge_commits(&head_commit, &upstream_commit, None)` — in-memory.
///    - If the index has conflicts → `GitError::Other("merge would conflict: …")`.
///      **No MERGING state is set.  The repo is left completely untouched.**
///    - Clean: `index.write_tree_to` → `repo.commit(…, parents=[head, upstream])`
///      → `index.read_tree` + `index.write` → `checkout_head(safe, recreate_missing)`.
///    → `Merged { commit }`.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure.  The repo is **never** left in a
/// partial state: conflicts are detected before any write occurs.
pub fn execute_pull(repo: &Repository, repo_path: &Path) -> Result<PullOutcome, GitError> {
    // ── 1. Resolve current branch + upstream ─────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;

    let branch_name = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();

    let (_, remote_name, _) = resolve_upstream_info(repo, &branch_name)?;

    // ── 2. git fetch <remote> via CLI ─────────────────────────
    let fetch_out = run_git(repo_path, &["fetch", &remote_name])
        .map_err(|e| GitError::Other(format!("fetch failed: {}", e)))?;

    if fetch_out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            fetch_out.status,
            fetch_out.stderr.trim()
        )));
    }

    // ── 3. Re-resolve upstream tip after fetch ────────────────
    let upstream_oid = resolve_upstream_oid(repo, &branch_name, &remote_name)?;

    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;

    // HEAD == upstream → UpToDate.
    if head_oid == upstream_oid {
        return Ok(PullOutcome::UpToDate);
    }

    // HEAD is a descendant of upstream (already ahead) → UpToDate.
    // graph_descendant_of(a, b) returns true if a is a descendant of b.
    if repo
        .graph_descendant_of(head_oid, upstream_oid)
        .unwrap_or(false)
    {
        return Ok(PullOutcome::UpToDate);
    }

    // ── 4. Fast-forward check ─────────────────────────────────
    // HEAD is an ancestor of upstream if upstream is a descendant of HEAD.
    let can_ff = repo
        .graph_descendant_of(upstream_oid, head_oid)
        .unwrap_or(false);

    if can_ff {
        let upstream_commit = repo.find_commit(upstream_oid).map_err(|e| {
            GitError::Other(format!("upstream commit lookup failed: {}", e.message()))
        })?;

        // ORDER MATTERS: check out the upstream tree while HEAD/index still
        // point at the OLD tree.  Safe checkout then sees old→new as the
        // change set (updates modified files, creates new ones, writes the
        // index).  Moving the branch ref first makes the baseline equal the
        // target — checkout becomes a no-op and the WT silently goes stale
        // (caught by pull tests).
        let obj = upstream_commit.into_object();
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.safe();
        repo.checkout_tree(&obj, Some(&mut cb))
            .map_err(|e| GitError::Other(format!("checkout_tree (FF) failed: {}", e.message())))?;

        // Now advance the branch ref to the upstream tip (force=true only
        // overwrites the ref we just validated as an ancestor — a safe FF).
        let refname = format!("refs/heads/{}", branch_name);
        repo.reference(
            &refname,
            upstream_oid,
            true,
            &format!(
                "pull: fast-forward {} to {}",
                branch_name,
                &upstream_oid.to_string()[..8]
            ),
        )
        .map_err(|e| GitError::Other(format!("branch ref update failed: {}", e.message())))?;

        repo.set_head(&refname)
            .map_err(|e| GitError::Other(format!("set_head (FF) failed: {}", e.message())))?;

        return Ok(PullOutcome::FastForward {
            to: CommitId(upstream_oid.to_string()),
        });
    }

    // ── 5. True merge (diverged) ──────────────────────────────
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let upstream_commit = repo
        .find_commit(upstream_oid)
        .map_err(|e| GitError::Other(format!("upstream commit lookup failed: {}", e.message())))?;

    // In-memory merge — does NOT set MERGING state, does NOT touch WT.
    let mut index = repo
        .merge_commits(&head_commit, &upstream_commit, None)
        .map_err(|e| GitError::Other(format!("merge_commits in-memory failed: {}", e.message())))?;

    // Conflict detection — if any conflict, return error with file list.
    // **Nothing has been written to the repo at this point.**
    if index.has_conflicts() {
        let mut conflict_files: Vec<String> = Vec::new();
        if let Ok(conflicts) = index.conflicts() {
            for conflict in conflicts.flatten() {
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
        }
        return Err(GitError::Other(format!(
            "merge would conflict: {}",
            if conflict_files.is_empty() {
                "(unknown files)".to_string()
            } else {
                conflict_files.join(", ")
            }
        )));
    }

    // ── 6. Write in-memory tree to ODB ───────────────────────
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    // ── 7. Build merge commit ─────────────────────────────────
    let committer = build_signature(repo)?;
    let author = committer.clone();

    // Upstream tracking branch name for the commit message.
    let upstream_ref_name = format!("refs/remotes/{}/{}", remote_name, branch_name);
    let merge_message = format!("Merge remote-tracking branch '{}'", upstream_ref_name);

    // Create the merge commit WITHOUT moving any ref yet (update_ref=None):
    // the WT/index must be synced while HEAD still points at the old tree so
    // safe checkout sees old→new as the change set (see FF path note).
    let new_oid = repo
        .commit(
            None,
            &author,
            &committer,
            &merge_message,
            &new_tree,
            &[&head_commit, &upstream_commit],
        )
        .map_err(|e| GitError::Other(format!("merge commit creation failed: {}", e.message())))?;

    // ── 8. Sync WT + index to the merge tree (old baseline) ──
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(
        repo.find_tree(new_tree_oid).unwrap().as_object(),
        Some(&mut cb),
    )
    .map_err(|e| GitError::Other(format!("checkout_tree after merge failed: {}", e.message())))?;

    // ── 9. Move the branch ref to the merge commit ────────────
    let refname = format!("refs/heads/{}", branch_name);
    repo.reference(
        &refname,
        new_oid,
        true,
        &format!("pull: merge {} into {}", remote_name, branch_name),
    )
    .map_err(|e| GitError::Other(format!("branch ref update (merge) failed: {}", e.message())))?;
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head (merge) failed: {}", e.message())))?;

    Ok(PullOutcome::Merged {
        commit: CommitId(new_oid.to_string()),
    })
}

// ────────────────────────────────────────────────────────────
// Fetch (W5-MENU) — download remote objects, never merge
// ────────────────────────────────────────────────────────────

/// Run `git fetch` for the repository at `repo_path`.
///
/// This is **fetch-only**: it downloads remote objects and updates the
/// remote-tracking refs, but it **never merges, fast-forwards, or moves the
/// current branch**.  It is the safe sibling of [`execute_pull`] and is wired
/// to the Repository → Fetch menu command (W5-MENU / ADR-0029).
///
/// The remote is resolved from the current branch's upstream when possible;
/// otherwise `git fetch --all` is used so a detached / no-upstream repo still
/// gets its remote-tracking refs updated.  The CLI wrapper ([`run_git`]) is
/// reused (60 s timeout, `GIT_TERMINAL_PROMPT=0`).
///
/// # Errors
///
/// Returns [`GitError::Other`] when the git CLI fails to start or exits
/// non-zero.
pub fn fetch_remote(repo: &Repository, repo_path: &Path) -> Result<FetchOutcome, GitError> {
    // Resolve the upstream remote for the current branch, falling back to
    // fetching every remote when no single upstream can be determined.
    let remote = resolve_fetch_remote(repo);

    let args: Vec<&str> = match remote.as_deref() {
        Some(name) => vec!["fetch", name],
        None => vec!["fetch", "--all"],
    };

    let out =
        run_git(repo_path, &args).map_err(|e| GitError::Other(format!("fetch failed: {}", e)))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    Ok(FetchOutcome {
        remote: remote.unwrap_or_else(|| "--all".to_string()),
    })
}

/// Best-effort resolution of the remote to fetch: the current branch's
/// configured upstream remote, else the sole configured remote, else `None`
/// (caller fetches `--all`).
fn resolve_fetch_remote(repo: &Repository) -> Option<String> {
    // Prefer the current branch's upstream remote.
    if let Ok(head_ref) = repo.head() {
        if let Ok(branch_name) = head_ref.shorthand() {
            if let Ok((_, remote_name, _)) = resolve_upstream_info(repo, branch_name) {
                return Some(remote_name);
            }
        }
    }
    // Otherwise, if exactly one remote is configured, use it.
    if let Ok(remotes) = repo.remotes() {
        if remotes.len() == 1 {
            if let Some(Ok(Some(name))) = remotes.iter().next() {
                return Some(name.to_string());
            }
        }
    }
    None
}

// ────────────────────────────────────────────────────────────
// Internal helpers (pull)
// ────────────────────────────────────────────────────────────

/// Resolve upstream info for a local branch.
///
/// Returns `(branch_name, remote_name, behind_count)`.
fn resolve_upstream_info(
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
fn resolve_upstream_oid(
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

/// Attempt an in-memory merge with the current upstream tip to predict conflicts.
///
/// Returns `Ok(Some(warning_string))` if a conflict is predicted,
/// `Ok(None)` if the merge would be clean (or fast-forward), or
/// `Err(...)` if the prediction itself failed (non-fatal — caller ignores).
fn predict_merge_conflict(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<Option<String>, GitError> {
    let head_oid = repo.head().ok().and_then(|r| r.target());
    let upstream_oid = resolve_upstream_oid(repo, branch_name, remote_name).ok();

    let (head_oid, upstream_oid) = match (head_oid, upstream_oid) {
        (Some(h), Some(u)) => (h, u),
        _ => return Ok(None),
    };

    // If already fast-forward or up-to-date, no conflict possible.
    if head_oid == upstream_oid {
        return Ok(None);
    }
    if repo
        .graph_descendant_of(head_oid, upstream_oid)
        .unwrap_or(false)
        || repo
            .graph_descendant_of(upstream_oid, head_oid)
            .unwrap_or(false)
    {
        return Ok(None);
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    let upstream_commit = repo
        .find_commit(upstream_oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let index = repo
        .merge_commits(&head_commit, &upstream_commit, None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    if index.has_conflicts() {
        Ok(Some(
            "Plan-time merge prediction: the current upstream tip would conflict with HEAD. \
             Execute is NOT blocked (fetch may change things), but be aware that if the \
             upstream has not changed, execute will fail safely leaving the repo untouched."
                .to_string(),
        ))
    } else {
        Ok(None)
    }
}

// ────────────────────────────────────────────────────────────
// PushOutcome  (T-HT-004)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// plan_push  (T-HT-004)
// ────────────────────────────────────────────────────────────

/// Analyse whether a push is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0009 Guarded policy)
///
/// - HEAD is detached or unborn.
/// - Upstream is configured **and** ahead count is 0 (nothing to push).
/// - No upstream configured **and** no remote exists anywhere in the repo.
///
/// # Set-upstream flow
///
/// If no upstream is configured but at least one remote exists (prefer
/// `origin`; fall back to the only remote), the push is **not** blocked.
/// The title is set to `"Push '<branch>' to '<remote>' (set upstream)"` and
/// `execute_push` will pass `-u` to set the upstream automatically.
///
/// # Preview commits
///
/// - Upstream configured: revwalk from HEAD, hiding the upstream tip.
/// - Set-upstream flow: revwalk from HEAD (no hide — all commits are "new").
/// Both paths are capped at 100 commits.
///
/// # Warning
///
/// - `"Non-fast-forward pushes will fail — force is not used."` (always shown).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_push(repo: &Repository) -> Result<OperationPlan, GitError> {
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
    let warnings: Vec<String> =
        vec!["Non-fast-forward pushes will fail — force is not used.".to_string()];

    // Detached HEAD.
    if let Head::Detached { .. } = &head {
        blockers
            .push("HEAD is detached. Push is only supported when HEAD is on a branch.".to_string());
    }

    // Unborn HEAD.
    if let Head::Unborn { .. } = &head {
        blockers
            .push("HEAD is unborn (no commits exist). Cannot push an empty branch.".to_string());
    }

    // ── 4. Only proceed with upstream/remote analysis for Attached HEAD ──
    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => {
            // Blockers already set; build minimal plan.
            let predicted = StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            };
            let recovery =
                "Push requires a branch. Use `git checkout <branch>` to attach HEAD.".to_string();
            return Ok(OperationPlan {
                title: "Push (blocked)".to_string(),
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
    };

    // ── 5. Upstream check ────────────────────────────────────
    // Try to resolve upstream info; Ok → upstream configured,
    // Err → no upstream (set-upstream flow or hard blocker).
    let upstream_info = resolve_upstream_info(repo, &branch_name);

    let (has_upstream, remote_name, ahead_count) = match upstream_info {
        Ok((_, remote, _behind)) => {
            // Compute ahead count (HEAD vs upstream tip).
            let branch_ref = repo
                .find_branch(&branch_name, BranchType::Local)
                .map_err(|e| {
                    GitError::Other(format!(
                        "branch '{}' not found: {}",
                        branch_name,
                        e.message()
                    ))
                })?;
            let upstream_ref = branch_ref
                .upstream()
                .map_err(|e| GitError::Other(format!("upstream lookup failed: {}", e.message())))?;
            let head_oid = branch_ref
                .get()
                .target()
                .ok_or_else(|| GitError::Other("branch has no target".to_string()))?;
            let upstream_oid = upstream_ref
                .get()
                .target()
                .ok_or_else(|| GitError::Other("upstream has no target".to_string()))?;
            let (ahead, _) = repo
                .graph_ahead_behind(head_oid, upstream_oid)
                .unwrap_or((0, 0));
            (true, remote, ahead)
        }
        Err(_) => {
            // No upstream configured — find a remote to use (set-upstream flow).
            let remotes = repo
                .remotes()
                .map_err(|e| GitError::Other(format!("failed to list remotes: {}", e.message())))?;
            let remote_names: Vec<String> = remotes
                .iter()
                .filter_map(|s| s.ok().flatten())
                .map(|s| s.to_owned())
                .collect();

            if remote_names.is_empty() {
                blockers.push(format!(
                    "No upstream configured for branch '{}' and no remotes exist. \
                     Add a remote with `git remote add origin <url>`.",
                    branch_name
                ));
                (false, String::new(), 0usize)
            } else {
                // Prefer "origin"; fall back to the only remote.
                let chosen = if remote_names.iter().any(|r| r == "origin") {
                    "origin".to_string()
                } else {
                    remote_names[0].clone()
                };
                (false, chosen, usize::MAX) // MAX sentinel: compute below
            }
        }
    };

    // ── 6. Upstream-configured but nothing to push ───────────
    if has_upstream && ahead_count == 0 {
        blockers.push(format!(
            "Branch '{}' is already up to date with its upstream — nothing to push.",
            branch_name
        ));
    }

    // ── 7. Determine title ────────────────────────────────────
    let is_set_upstream_flow = !has_upstream && blockers.is_empty();
    let title = if is_set_upstream_flow {
        format!("Push '{}' to '{}' (set upstream)", branch_name, remote_name)
    } else if has_upstream {
        format!("Push '{}' to '{}'", branch_name, remote_name)
    } else {
        "Push (blocked)".to_string()
    };

    // ── 8. Build preview_commits (revwalk) ───────────────────
    // Only collect when no blockers (pointless otherwise).
    let preview_commits: Vec<String> = if blockers.is_empty() {
        build_push_preview(repo, &branch_name, &remote_name, has_upstream).unwrap_or_default()
    } else {
        Vec::new()
    };

    // For set-upstream flow: use actual commit count as ahead_count substitute.
    let effective_ahead = if is_set_upstream_flow {
        preview_commits.len()
    } else {
        ahead_count
    };

    // ── 9. Predicted StateSummary ─────────────────────────────
    let predicted = StateSummary {
        head: format!(
            "branch: {} (pushed {} commit(s))",
            branch_name, effective_ahead
        ),
        dirty: current.dirty.clone(),
    };

    // ── 10. Recovery guidance ─────────────────────────────────
    let recovery =
        "Push only sends commits to the remote — the local repository is never modified.\n\
         If the push is rejected (non-fast-forward), pull first and re-plan:\n  \
         git pull\n  git push\n\
         The reflog records every HEAD movement:\n  git reflog"
            .to_string();

    Ok(OperationPlan {
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits,
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_push  (T-HT-004)
// ────────────────────────────────────────────────────────────

/// Execute a push.
///
/// - If the current branch has an upstream configured:
///   `git push <remote> <branch>`
/// - If no upstream is configured (set-upstream flow):
///   `git push -u <remote> <branch>`
///
/// **force / --force / --force-with-lease are never used.**
///
/// Non-fast-forward pushes are rejected by the remote and returned as
/// `GitError::Other` with the full stderr.  The local repository is left
/// completely untouched on failure.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure.
pub fn execute_push(repo: &Repository, repo_path: &Path) -> Result<PushOutcome, GitError> {
    // ── 1. Resolve current branch ─────────────────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let branch_name = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();

    // ── 2. Check for upstream ─────────────────────────────────
    let upstream_result = resolve_upstream_info(repo, &branch_name);
    let (has_upstream, remote_name) = match upstream_result {
        Ok((_, remote, _)) => (true, remote),
        Err(_) => {
            // No upstream — pick a remote (prefer origin).
            let remotes = repo
                .remotes()
                .map_err(|e| GitError::Other(format!("failed to list remotes: {}", e.message())))?;
            let remote_names: Vec<String> = remotes
                .iter()
                .filter_map(|s| s.ok().flatten())
                .map(|s| s.to_owned())
                .collect();
            if remote_names.is_empty() {
                return Err(GitError::Other(
                    "No remote configured. Cannot push.".to_string(),
                ));
            }
            let chosen = if remote_names.iter().any(|r| r == "origin") {
                "origin".to_string()
            } else {
                remote_names[0].clone()
            };
            (false, chosen)
        }
    };

    // ── 3. Compute ahead count for PushOutcome.pushed ────────
    let pushed_count = if has_upstream {
        let branch_ref2 = repo
            .find_branch(&branch_name, BranchType::Local)
            .map_err(|e| {
                GitError::Other(format!(
                    "branch '{}' not found: {}",
                    branch_name,
                    e.message()
                ))
            })?;
        let upstream_ref = branch_ref2
            .upstream()
            .map_err(|e| GitError::Other(format!("upstream lookup failed: {}", e.message())))?;
        let head_oid2 = branch_ref2
            .get()
            .target()
            .ok_or_else(|| GitError::Other("branch has no target".to_string()))?;
        let upstream_oid2 = upstream_ref
            .get()
            .target()
            .ok_or_else(|| GitError::Other("upstream has no target".to_string()))?;
        let (ahead, _) = repo
            .graph_ahead_behind(head_oid2, upstream_oid2)
            .unwrap_or((0, 0));
        ahead
    } else {
        // Set-upstream flow: use revwalk count.
        build_push_preview(repo, &branch_name, &remote_name, false)
            .map(|v| v.len())
            .unwrap_or(0)
    };

    // ── 4. Build git args (no --force, ever) ─────────────────
    let args: Vec<&str> = if has_upstream {
        vec!["push", &remote_name, &branch_name]
    } else {
        vec!["push", "-u", &remote_name, &branch_name]
    };

    // ── 5. Run git push via CLI ───────────────────────────────
    let out =
        run_git(repo_path, &args).map_err(|e| GitError::Other(format!("push failed: {}", e)))?;

    if out.status != 0 {
        return Err(GitError::Other(format!(
            "push failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    Ok(PushOutcome {
        pushed: pushed_count,
        set_upstream: !has_upstream,
    })
}

// ────────────────────────────────────────────────────────────
// Internal helpers (push)
// ────────────────────────────────────────────────────────────

/// Build the preview_commits list for a push plan.
///
/// - `has_upstream=true`:  walk HEAD, hide the upstream OID  (`upstream..HEAD`).
/// - `has_upstream=false`: walk all commits reachable from HEAD (set-upstream flow).
/// Both paths are capped at 100 commits, newest first.
///
/// Returns an empty Vec on any error (non-fatal — preview is best-effort).
fn build_push_preview(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
    has_upstream: bool,
) -> Result<Vec<String>, GitError> {
    const MAX_PREVIEW: usize = 100;

    let head_oid = repo
        .head()
        .ok()
        .and_then(|r| r.target())
        .ok_or_else(|| GitError::Other("HEAD has no target".to_string()))?;

    let mut walk = repo
        .revwalk()
        .map_err(|e| GitError::Other(format!("revwalk init failed: {}", e.message())))?;

    walk.push(head_oid)
        .map_err(|e| GitError::Other(format!("revwalk push failed: {}", e.message())))?;

    // Hide the upstream tip so we only see commits not yet on the remote.
    if has_upstream {
        if let Ok(upstream_oid) = resolve_upstream_oid(repo, branch_name, remote_name) {
            let _ = walk.hide(upstream_oid);
        }
    }

    // Topological sort, newest first.
    walk.set_sorting(git2::Sort::TOPOLOGICAL)
        .map_err(|e| GitError::Other(format!("revwalk sort failed: {}", e.message())))?;

    let mut result: Vec<String> = Vec::new();
    for oid_result in walk {
        if result.len() >= MAX_PREVIEW {
            break;
        }
        let oid = oid_result
            .map_err(|e| GitError::Other(format!("revwalk iter failed: {}", e.message())))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;

        let short = &oid.to_string()[..8];
        let summary: String = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no message)")
            .chars()
            .take(72)
            .collect();
        result.push(format!("{}  {}", short, summary));
    }

    Ok(result)
}

fn short_oid_string(oid: git2::Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

fn local_branch_oid(repo: &Repository, branch_name: &str) -> Result<git2::Oid, GitError> {
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

fn choose_push_remote(repo: &Repository) -> Result<String, GitError> {
    let remotes = repo
        .remotes()
        .map_err(|e| GitError::Other(format!("failed to list remotes: {}", e.message())))?;
    let remote_names: Vec<String> = remotes
        .iter()
        .filter_map(|s| s.ok().flatten())
        .map(|s| s.to_owned())
        .collect();
    if remote_names.is_empty() {
        return Err(GitError::Other(
            "No remote configured. Cannot push.".to_string(),
        ));
    }
    if remote_names.iter().any(|r| r == "origin") {
        Ok("origin".to_string())
    } else {
        Ok(remote_names[0].clone())
    }
}

fn build_push_preview_for_oid(
    repo: &Repository,
    head_oid: git2::Oid,
    upstream_oid: Option<git2::Oid>,
) -> Result<Vec<String>, GitError> {
    const MAX_PREVIEW: usize = 100;

    let mut walk = repo
        .revwalk()
        .map_err(|e| GitError::Other(format!("revwalk init failed: {}", e.message())))?;
    walk.push(head_oid)
        .map_err(|e| GitError::Other(format!("revwalk push failed: {}", e.message())))?;
    if let Some(upstream_oid) = upstream_oid {
        let _ = walk.hide(upstream_oid);
    }
    walk.set_sorting(git2::Sort::TOPOLOGICAL)
        .map_err(|e| GitError::Other(format!("revwalk sort failed: {}", e.message())))?;

    let mut result = Vec::new();
    for oid_result in walk {
        if result.len() >= MAX_PREVIEW {
            break;
        }
        let oid = oid_result
            .map_err(|e| GitError::Other(format!("revwalk iter failed: {}", e.message())))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;
        let short = short_oid_string(oid);
        let summary: String = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no message)")
            .chars()
            .take(72)
            .collect();
        result.push(format!("{}  {}", short, summary));
    }
    Ok(result)
}

/// Plan a fast-forward-only pull for a non-current local branch.
///
/// This is a ref-only operation: execution fetches the branch's upstream remote
/// and advances `refs/heads/<branch>` only if the upstream tip is a descendant
/// of the local branch tip. The working tree and HEAD are never changed.
pub fn plan_pull_branch_ff(
    repo: &Repository,
    branch_name: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();

    if !status.conflicted.is_empty() {
        warnings.push(format!(
            "Repository has {} conflicted file(s); this ref-only pull will not touch the working tree.",
            status.conflicted.len()
        ));
    } else if status.is_dirty() {
        warnings.push(
            "Working tree is dirty; this ref-only pull will not touch the working tree."
                .to_string(),
        );
    }

    let local_oid = match local_branch_oid(repo, branch_name) {
        Ok(oid) => oid,
        Err(e) => {
            blockers.push(format!("{}", e));
            git2::Oid::ZERO_SHA1
        }
    };

    let (remote_name, upstream_oid, behind_count) = match resolve_upstream_info(repo, branch_name) {
        Ok((_, remote, behind)) => {
            let oid = resolve_upstream_oid(repo, branch_name, &remote).ok();
            (remote, oid, behind)
        }
        Err(e) => {
            blockers.push(format!(
                "No upstream configured for branch '{}': {}.",
                branch_name, e
            ));
            (String::new(), None, 0)
        }
    };

    if blockers.is_empty() {
        if let Some(upstream_oid) = upstream_oid {
            if local_oid == upstream_oid
                || repo
                    .graph_descendant_of(local_oid, upstream_oid)
                    .unwrap_or(false)
            {
                blockers.push(format!(
                    "Branch '{}' is already up to date with its upstream.",
                    branch_name
                ));
            } else if !repo
                .graph_descendant_of(upstream_oid, local_oid)
                .unwrap_or(false)
            {
                blockers.push(format!(
                    "Branch '{}' cannot be fast-forwarded to its upstream; pull it while checked out to merge.",
                    branch_name
                ));
            }
        }
    }

    let predicted_head = if blockers.is_empty() {
        format!(
            "branch: {} -> {}",
            branch_name,
            upstream_oid
                .map(short_oid_string)
                .unwrap_or_else(|| "upstream tip after fetch".to_string())
        )
    } else {
        current.head.clone()
    };

    Ok(OperationPlan {
        title: format!(
            "Pull '{}' from '{}' (ff-only, ref-only, {} behind)",
            branch_name, remote_name, behind_count
        ),
        current,
        predicted: StateSummary {
            head: predicted_head,
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: format!(
            "This updates only refs/heads/{} after verifying a fast-forward. \
             The working tree is not changed. If needed, restore the old tip with git branch -f {} <old-sha>.",
            branch_name, branch_name
        ),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

pub fn execute_pull_branch_ff(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
) -> Result<PullOutcome, GitError> {
    preflight_check(repo, plan)?;
    let local_oid = local_branch_oid(repo, branch_name)?;
    let (_, remote_name, _) = resolve_upstream_info(repo, branch_name)?;

    let fetch_out = run_git(repo_path, &["fetch", &remote_name])
        .map_err(|e| GitError::Other(format!("fetch failed: {}", e)))?;
    if fetch_out.status != 0 {
        return Err(GitError::Other(format!(
            "fetch failed (exit {}): {}",
            fetch_out.status,
            fetch_out.stderr.trim()
        )));
    }

    let upstream_oid = resolve_upstream_oid(repo, branch_name, &remote_name)?;
    if local_oid == upstream_oid
        || repo
            .graph_descendant_of(local_oid, upstream_oid)
            .unwrap_or(false)
    {
        return Ok(PullOutcome::UpToDate);
    }
    if !repo
        .graph_descendant_of(upstream_oid, local_oid)
        .unwrap_or(false)
    {
        return Err(GitError::Other(format!(
            "branch '{}' is not fast-forwardable to upstream",
            branch_name
        )));
    }

    let refname = format!("refs/heads/{}", branch_name);
    repo.reference(
        &refname,
        upstream_oid,
        true,
        &format!(
            "pull: fast-forward {} to {}",
            branch_name,
            short_oid_string(upstream_oid)
        ),
    )
    .map_err(|e| GitError::Other(format!("branch ref update failed: {}", e.message())))?;

    Ok(PullOutcome::FastForward {
        to: CommitId(upstream_oid.to_string()),
    })
}

/// Plan a push for a specified local branch. Unlike [`plan_push`], this does
/// not require the branch to be checked out.
pub fn plan_push_branch(
    repo: &Repository,
    branch_name: &str,
    set_upstream: bool,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers = Vec::new();
    let warnings = vec!["Non-fast-forward pushes will fail; force is not used.".to_string()];

    let local_oid = match local_branch_oid(repo, branch_name) {
        Ok(oid) => oid,
        Err(e) => {
            blockers.push(format!("{}", e));
            git2::Oid::ZERO_SHA1
        }
    };

    let (remote_name, upstream_oid, has_upstream) = if set_upstream {
        match choose_push_remote(repo) {
            Ok(remote) => (remote, None, false),
            Err(e) => {
                blockers.push(format!("{}", e));
                (String::new(), None, false)
            }
        }
    } else {
        match resolve_upstream_info(repo, branch_name) {
            Ok((_, remote, _)) => {
                let oid = resolve_upstream_oid(repo, branch_name, &remote).ok();
                (remote, oid, true)
            }
            Err(e) => {
                blockers.push(format!(
                    "No upstream configured for branch '{}': {}.",
                    branch_name, e
                ));
                (String::new(), None, false)
            }
        }
    };

    let ahead_count = if blockers.is_empty() {
        if let Some(upstream_oid) = upstream_oid {
            repo.graph_ahead_behind(local_oid, upstream_oid)
                .map(|(ahead, _)| ahead)
                .unwrap_or(0)
        } else {
            build_push_preview_for_oid(repo, local_oid, None)
                .map(|commits| commits.len())
                .unwrap_or(0)
        }
    } else {
        0
    };

    if blockers.is_empty() && has_upstream && ahead_count == 0 {
        blockers.push(format!(
            "Branch '{}' is already up to date with its upstream; nothing to push.",
            branch_name
        ));
    }

    let preview_commits = if blockers.is_empty() {
        build_push_preview_for_oid(repo, local_oid, upstream_oid).unwrap_or_default()
    } else {
        Vec::new()
    };

    let title = if set_upstream {
        format!(
            "Push '{}' to '{}/{}' (set upstream)",
            branch_name, remote_name, branch_name
        )
    } else {
        format!("Push '{}' to '{}'", branch_name, remote_name)
    };

    Ok(OperationPlan {
        title,
        current,
        predicted: StateSummary {
            head: format!("branch: {} (pushed {} commit(s))", branch_name, ahead_count),
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: "Push sends commits to the remote and does not modify the working tree. If the push is rejected, fetch or pull first and re-plan.".to_string(),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits,
        destructive: false,
    })
}

pub fn execute_push_branch(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
    set_upstream: bool,
) -> Result<PushOutcome, GitError> {
    preflight_check(repo, plan)?;
    let local_oid = local_branch_oid(repo, branch_name)?;

    let (remote_name, upstream_oid) = if set_upstream {
        (choose_push_remote(repo)?, None)
    } else {
        let (_, remote, _) = resolve_upstream_info(repo, branch_name)?;
        let upstream_oid = resolve_upstream_oid(repo, branch_name, &remote).ok();
        (remote, upstream_oid)
    };

    let pushed = if let Some(upstream_oid) = upstream_oid {
        repo.graph_ahead_behind(local_oid, upstream_oid)
            .map(|(ahead, _)| ahead)
            .unwrap_or(0)
    } else {
        build_push_preview_for_oid(repo, local_oid, None)
            .map(|commits| commits.len())
            .unwrap_or(0)
    };

    let args: Vec<&str> = if set_upstream {
        vec!["push", "-u", &remote_name, branch_name]
    } else {
        vec!["push", &remote_name, branch_name]
    };
    let out =
        run_git(repo_path, &args).map_err(|e| GitError::Other(format!("push failed: {}", e)))?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "push failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }

    Ok(PushOutcome {
        pushed,
        set_upstream,
    })
}

pub fn plan_set_upstream(
    repo: &Repository,
    branch_name: &str,
    upstream: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if repo.find_branch(branch_name, BranchType::Local).is_err() {
        blockers.push(format!("Branch '{}' does not exist.", branch_name));
    }
    if upstream.trim().is_empty() || upstream.trim() != upstream {
        blockers.push("Upstream must be a remote branch name like origin/main.".to_string());
    } else if repo.find_branch(upstream, BranchType::Remote).is_err() {
        warnings.push(format!(
            "Remote-tracking branch '{}' is not present locally; config can still be set.",
            upstream
        ));
    }

    Ok(OperationPlan {
        title: format!("Set upstream of '{}' to '{}'", branch_name, upstream),
        current,
        predicted: StateSummary {
            head: format!("branch: {} -> {}", branch_name, upstream),
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: format!(
            "This changes only branch.{}.remote and branch.{}.merge in git config. To undo, set the previous upstream again.",
            branch_name, branch_name
        ),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

pub fn execute_set_upstream(
    repo: &Repository,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    let mut branch = repo
        .find_branch(branch_name, BranchType::Local)
        .map_err(|e| {
            GitError::Other(format!(
                "branch '{}' not found: {}",
                branch_name,
                e.message()
            ))
        })?;
    branch
        .set_upstream(Some(upstream))
        .map_err(|e| GitError::Other(format!("set upstream failed: {}", e.message())))?;
    Ok(())
}
