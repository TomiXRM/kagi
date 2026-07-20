//! Pull operation pipeline (T-HT-003 / ADR-0009 Guarded).
//!
//! Split out of the monolithic `ops/pull_push.rs` (Wave 3, ADR-0116 /
//! T-SPLIT-PULLPUSH-001). Behaviour-preserving move only.
//!
//! Covers the pull triple for the current branch (`plan_pull` / `execute_pull`),
//! the ref-only fast-forward pull for a non-current branch
//! (`plan_pull_branch_ff` / `execute_pull_branch_ff`), and the snapshot-only
//! remote pull plan (`plan_pull_remote`). The closely-related tracking-branch
//! checkout and switch-to-latest flows live in the sibling `switch.rs`.

use super::remote_common::{
    local_branch_oid, resolve_upstream_info, resolve_upstream_oid, short_oid_string,
};
use super::*;
use kagi_domain::plan_note::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};
use kagi_domain::plan_note::{PullNote, PullRecovery, PullTitle};

/// Build the confirm plan for pulling a **remote** branch over SSH (ADR-0089
/// Phase 3 / ADR-0097). There is no local `Repository`, so this synthesises the
/// [`OperationPlan`] the modal needs from the snapshot's ahead/behind counts
/// rather than a git2 dry run; the pull itself runs via `kagi::remote::remote_pull`
/// in the UI layer. `head_summary` and `upstream` are display-only.
pub fn plan_pull_remote(
    branch: &str,
    upstream: &str,
    behind: usize,
    ahead: usize,
    remote_dirty: bool,
    head_summary: String,
) -> OperationPlan {
    let title = PlanTitle::Pull(PullTitle::PullRemote {
        branch: branch.to_string(),
        upstream: upstream.to_string(),
        behind,
    });

    let mut warnings: Vec<PlanNote> = Vec::new();
    if ahead > 0 && behind > 0 {
        warnings.push(PlanNote::Pull(PullNote::RemoteDiverged {
            branch: branch.to_string(),
            ahead,
            behind,
        }));
    }
    if remote_dirty {
        warnings.push(PlanNote::Pull(PullNote::RemoteDirty));
    }

    OperationPlan {
        disposition: PlanDisposition::Ready,
        title,
        current: StateSummary {
            head: head_summary.clone(),
            dirty: if remote_dirty {
                "remote tree dirty".to_string()
            } else {
                "remote (read-only view)".to_string()
            },
        },
        predicted: StateSummary {
            head: head_summary,
            dirty: if behind == 0 {
                "no change".to_string()
            } else if ahead > 0 {
                "merged on remote".to_string()
            } else {
                "fast-forwarded on remote".to_string()
            },
        },
        warnings,
        blockers: Vec::new(),
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Pull(PullRecovery::PullRemote),
            commands: Vec::new(),
        }),
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    }
}

// ────────────────────────────────────────────────────────────
// plan_pull  (ADR-0009 Guarded)
// ────────────────────────────────────────────────────────────

/// Analyse whether a pull is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0009 Guarded policy)
///
/// - HEAD is detached or unborn.
/// - No upstream is configured for the current branch.
/// - Repository is in a conflict state.
/// - Dirty paths that would be overwritten by the pull are checked at execute
///   time, after fetch reveals the exact upstream tip.
/// - (Plan-time) in-memory merge with the current upstream tip predicts a
///   conflict — shown as a **warning** at plan time (fetch may change things)
///   but still allows execution (the execute phase re-checks after fetch).
///
/// # Warnings
///
/// - The behind count shown is local knowledge; fetch may reveal more commits.
/// - Staged, unstaged, or untracked files exist. Pull may proceed, but execute
///   refuses if the fetched update would touch any dirty path.
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
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Detached HEAD: no branch to advance.
    if let Head::Detached { .. } = &head {
        blockers.push(PlanNote::Common(CommonNote::HeadDetached {
            op: PlanOp::Pull,
        }));
    }

    // Unborn HEAD: no commits exist yet.
    if let Head::Unborn { .. } = &head {
        blockers.push(PlanNote::Common(CommonNote::HeadUnborn {
            op: PlanOp::Pull,
        }));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(PlanNote::Common(CommonNote::ConflictedFiles {
            count: status.conflicted.len(),
            before: OpPhrase::Pulling,
        }));
    }

    // Dirty working tree (staged / unstaged) — warning only. The exact pull
    // target is known only after fetch, so execute re-checks dirty paths against
    // the fetched tree and refuses only if they overlap.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        warnings.push(PlanNote::Pull(PullNote::DirtyPullGuard {
            parts: DirtyParts {
                staged: status.staged.len(),
                modified: status.unstaged.len(),
            },
        }));
    }

    // Untracked files — warning only, but execute refuses if pull would create
    // or modify the same path.
    if !status.untracked.is_empty() {
        warnings.push(PlanNote::Common(CommonNote::UntrackedRemain {
            count: status.untracked.len(),
            ctx: UntrackedCtx::PullFetchMayTouch,
        }));
    }

    // ── 4. Resolve upstream (only when HEAD is attached) ─────
    let (branch_name, remote_name, behind_count) = if let Head::Attached { branch, .. } = &head {
        match resolve_upstream_info(repo, branch) {
            Ok(info) => info,
            Err(e) => {
                blockers.push(PlanNote::Pull(PullNote::NoUpstreamWithHint {
                    branch: branch.clone(),
                    err: e.to_string(),
                }));
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
        if let Ok(has_conflict) = predict_merge_conflict(repo, &branch_name, &remote_name) {
            if has_conflict {
                warnings.push(PlanNote::Pull(PullNote::MergePrediction));
            }
        }
    }

    // ── 6. Predicted StateSummary ─────────────────────────────
    let predicted = StateSummary {
        head: format!("branch: {}", branch_name),
        dirty: current.dirty.clone(),
    };

    // ── 7. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Pull(PullRecovery::Pull),
        commands: vec![
            "git reset --hard HEAD~1".to_string(),
            "git reflog".to_string(),
        ],
    };

    Ok(OperationPlan {
        // ADR-0129 F-1: the UI's pull no-op detection keyed on the title text
        // ("up to date (local knowledge…"); the semantic state now travels
        // with the plan instead.
        disposition: if !blockers.is_empty() {
            PlanDisposition::Blocked
        } else if behind_count == 0 {
            PlanDisposition::NoOp(NoOpKind::PullUpToDate)
        } else {
            PlanDisposition::Ready
        },
        title: PlanTitle::Pull(PullTitle::Pull {
            branch: branch_name,
            remote: remote_name,
            behind: behind_count,
        }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
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
    let fetch_out = run_git(repo_path, &["fetch", "--prune", &remote_name])
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

    let head_commit_for_safety = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let head_tree_for_safety = head_commit_for_safety
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;

    // ── 4. Fast-forward check ─────────────────────────────────
    // HEAD is an ancestor of upstream if upstream is a descendant of HEAD.
    let can_ff = repo
        .graph_descendant_of(upstream_oid, head_oid)
        .unwrap_or(false);

    if can_ff {
        let upstream_commit = repo.find_commit(upstream_oid).map_err(|e| {
            GitError::Other(format!("upstream commit lookup failed: {}", e.message()))
        })?;
        let upstream_tree = upstream_commit.tree().map_err(|e| {
            GitError::Other(format!("upstream tree lookup failed: {}", e.message()))
        })?;

        ensure_pull_does_not_touch_dirty_paths(repo, &head_tree_for_safety, &upstream_tree)?;

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
    let head_commit = head_commit_for_safety;
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

    ensure_pull_does_not_touch_dirty_paths(repo, &head_tree_for_safety, &new_tree)?;

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
// Internal helpers (pull)
// ────────────────────────────────────────────────────────────

fn ensure_pull_does_not_touch_dirty_paths(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<(), GitError> {
    let status = working_tree_status(repo)?;
    if status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty() {
        return Ok(());
    }

    let mut dirty_paths: std::collections::HashSet<PathBuf> =
        status.untracked.iter().cloned().collect();
    for file in status.staged.iter().chain(status.unstaged.iter()) {
        dirty_paths.insert(file.path.clone());
        if let ChangeKind::Renamed { from } = &file.change {
            dirty_paths.insert(from.clone());
        }
    }

    let changed_paths = pull_changed_paths_between_trees(repo, old_tree, new_tree)?;
    let mut overlapping: Vec<String> = changed_paths
        .into_iter()
        .filter(|path| dirty_paths.contains(path))
        .map(|path| path.display().to_string())
        .collect();
    overlapping.sort();
    overlapping.dedup();

    if overlapping.is_empty() {
        Ok(())
    } else {
        Err(GitError::Other(format!(
            "pull would overwrite dirty path(s): {}. Stash or commit those paths, then pull again.",
            overlapping.join(", ")
        )))
    }
}

fn pull_changed_paths_between_trees(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<PathBuf>, GitError> {
    let diff = repo
        .diff_tree_to_tree(Some(old_tree), Some(new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for pull safety failed: {}",
                e.message()
            ))
        })?;

    let mut paths = Vec::new();
    for delta in diff.deltas() {
        if let Some(path) = delta.old_file().path() {
            paths.push(path.to_path_buf());
        }
        if let Some(path) = delta.new_file().path() {
            paths.push(path.to_path_buf());
        }
    }
    Ok(paths)
}

/// Attempt an in-memory merge with the current upstream tip to predict conflicts.
///
/// Returns `Ok(true)` if a conflict is predicted, `Ok(false)` if the merge
/// would be clean (or fast-forward), or `Err(...)` if the prediction itself
/// failed (non-fatal — caller ignores and treats as no warning).
fn predict_merge_conflict(
    repo: &Repository,
    branch_name: &str,
    remote_name: &str,
) -> Result<bool, GitError> {
    let head_oid = repo.head().ok().and_then(|r| r.target());
    let upstream_oid = resolve_upstream_oid(repo, branch_name, remote_name).ok();

    let (head_oid, upstream_oid) = match (head_oid, upstream_oid) {
        (Some(h), Some(u)) => (h, u),
        _ => return Ok(false),
    };

    // If already fast-forward or up-to-date, no conflict possible.
    if head_oid == upstream_oid {
        return Ok(false);
    }
    if repo
        .graph_descendant_of(head_oid, upstream_oid)
        .unwrap_or(false)
        || repo
            .graph_descendant_of(upstream_oid, head_oid)
            .unwrap_or(false)
    {
        return Ok(false);
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

    Ok(index.has_conflicts())
}

// ────────────────────────────────────────────────────────────
// plan_pull_branch_ff / execute_pull_branch_ff (ref-only ff pull)
// ────────────────────────────────────────────────────────────

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
    let mut warnings: Vec<PlanNote> = Vec::new();
    let mut blockers: Vec<PlanNote> = Vec::new();

    if !status.conflicted.is_empty() {
        warnings.push(PlanNote::Pull(PullNote::ConflictedRefOnly {
            count: status.conflicted.len(),
        }));
    } else if status.is_dirty() {
        warnings.push(PlanNote::Pull(PullNote::DirtyRefOnly));
    }

    let local_oid = match local_branch_oid(repo, branch_name) {
        Ok(oid) => oid,
        Err(e) => {
            blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                message: e.to_string(),
            }));
            git2::Oid::ZERO_SHA1
        }
    };

    let (remote_name, upstream_oid, behind_count) = match resolve_upstream_info(repo, branch_name) {
        Ok((_, remote, behind)) => {
            let oid = resolve_upstream_oid(repo, branch_name, &remote).ok();
            (remote, oid, behind)
        }
        Err(e) => {
            blockers.push(PlanNote::Pull(PullNote::NoUpstream {
                branch: branch_name.to_string(),
                err: e.to_string(),
            }));
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
                blockers.push(PlanNote::Pull(PullNote::AlreadyUpToDate {
                    branch: branch_name.to_string(),
                }));
            } else if !repo
                .graph_descendant_of(upstream_oid, local_oid)
                .unwrap_or(false)
            {
                blockers.push(PlanNote::Pull(PullNote::CannotFastForward {
                    branch: branch_name.to_string(),
                }));
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
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Pull(PullTitle::PullBranchFf {
            branch: branch_name.to_string(),
            remote: remote_name,
            behind: behind_count,
        }),
        current,
        predicted: StateSummary {
            head: predicted_head,
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Pull(PullRecovery::PullBranchFf {
                branch: branch_name.to_string(),
            }),
            commands: vec![format!("git branch -f {} <old-sha>", branch_name)],
        }),
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

    let fetch_out = run_git(repo_path, &["fetch", "--prune", &remote_name])
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

#[cfg(test)]
mod remote_pull_tests {
    use super::*;

    #[test]
    fn test_plan_pull_remote_ff_has_no_blockers() {
        let plan = plan_pull_remote(
            "main",
            "origin/main",
            3,
            0,
            false,
            "branch: main".to_string(),
        );
        assert!(plan.blockers.is_empty());
        assert!(!plan.destructive);
        assert!(plan.title.message_en().contains("3 commit"));
        assert!(plan.predicted.dirty.contains("fast-forward"));
    }

    #[test]
    fn test_plan_pull_remote_diverged_warns_merge() {
        let plan = plan_pull_remote(
            "main",
            "origin/main",
            2,
            1,
            false,
            "branch: main".to_string(),
        );
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.message_en().contains("diverged")));
        assert!(plan.predicted.dirty.contains("merge"));
    }
}
