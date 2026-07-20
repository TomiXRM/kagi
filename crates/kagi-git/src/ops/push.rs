//! Push operation pipeline (T-HT-004) and its branch / set-upstream variants.
//!
//! Split out of the monolithic `ops/pull_push.rs` (Wave 3, ADR-0116 /
//! T-SPLIT-PULLPUSH-001). Behaviour-preserving move only.
//!
//! **force / --force / --force-with-lease are never used** anywhere in this
//! module — non-fast-forward pushes are rejected by the remote, never forced.

use super::remote_common::{
    local_branch_oid, resolve_upstream_info, resolve_upstream_oid, short_oid_string,
};
use super::*;
// ADR-0129 appendix §B-5: this op's own typed notes, plus the cross-op
// `CommonNote`/`PlanOp` this file's HEAD-state blockers and GitError
// passthroughs reuse. `PushPunct` is reached through its owning submodule
// (not the `plan_note` flat re-export) so this fan-out PR never has to touch
// the shared `plan_note/mod.rs`.
use kagi_domain::plan_note::push::PushPunct;
use kagi_domain::plan_note::{CommonNote, PlanOp, PushNote, PushRecovery, PushTitle, RecoveryKind};

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
    // ADR-0129 appendix §B-5: typed notes, not English prose. `message_en()`
    // reproduces the exact legacy strings for oplog/klog/EN (golden-tested
    // in kagi-domain's `plan_note::push`).
    let mut blockers: Vec<PlanNote> = Vec::new();
    let warnings: Vec<PlanNote> = vec![PlanNote::Push(PushNote::NoForceUsed {
        punct: PushPunct::EmDash,
    })];

    // Detached HEAD.
    if let Head::Detached { .. } = &head {
        blockers.push(PlanNote::Common(CommonNote::HeadDetached {
            op: PlanOp::Push,
        }));
    }

    // Unborn HEAD.
    if let Head::Unborn { .. } = &head {
        blockers.push(PlanNote::Common(CommonNote::HeadUnborn {
            op: PlanOp::Push,
        }));
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
            return Ok(OperationPlan {
                disposition: PlanDisposition::for_blockers(&blockers),
                title: PlanTitle::Push(PushTitle::PushBlocked),
                current,
                predicted,
                warnings,
                blockers,
                recovery: Some(PlanRecovery {
                    kind: RecoveryKind::Push(PushRecovery::PushBlocked),
                    commands: vec!["git checkout <branch>".to_string()],
                }),
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
                blockers.push(PlanNote::Push(PushNote::NoUpstreamNoRemotes {
                    branch: branch_name.clone(),
                }));
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
    // ADR-0129 F-2: the UI's push no-op detection keyed on this blocker's
    // text ("nothing to push"); the semantic state now travels with the plan.
    // NoOp only when it is the sole blocker — any other blocker means the
    // plan is genuinely blocked, matching the old `.all(contains(…))` check.
    let nothing_to_push = has_upstream && ahead_count == 0 && blockers.is_empty();
    if has_upstream && ahead_count == 0 {
        blockers.push(PlanNote::Push(PushNote::AlreadyUpToDate {
            branch: branch_name.clone(),
            punct: PushPunct::EmDash,
        }));
    }

    // ── 7. Determine title ────────────────────────────────────
    let is_set_upstream_flow = !has_upstream && blockers.is_empty();
    let title = if is_set_upstream_flow {
        PlanTitle::Push(PushTitle::Push {
            branch: branch_name.clone(),
            remote: remote_name.clone(),
            set_upstream: true,
        })
    } else if has_upstream {
        PlanTitle::Push(PushTitle::Push {
            branch: branch_name.clone(),
            remote: remote_name.clone(),
            set_upstream: false,
        })
    } else {
        PlanTitle::Push(PushTitle::PushBlocked)
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
    let recovery = PlanRecovery {
        kind: RecoveryKind::Push(PushRecovery::Push),
        commands: vec![
            "git pull".to_string(),
            "git push".to_string(),
            "git reflog".to_string(),
        ],
    };

    Ok(OperationPlan {
        disposition: if nothing_to_push {
            PlanDisposition::NoOp(NoOpKind::PushUpToDate)
        } else {
            PlanDisposition::for_blockers(&blockers)
        },
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(recovery),
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
    // ADR-0129 appendix §B-5: typed notes for plan_push_branch. The two bare
    // `{}` error-message sites stay a `CommonNote::GitErrorPassthrough` (§G-4
    // — GitError keying is out of scope for this migration); the "no upstream"
    // site gets its own `PushNote` variant (this text is shared in English
    // with pull-ff's blocker, but that category is a separate fan-out PR).
    let mut blockers: Vec<PlanNote> = Vec::new();
    let warnings: Vec<PlanNote> = vec![PlanNote::Push(PushNote::NoForceUsed {
        punct: PushPunct::Semicolon,
    })];

    let local_oid = match local_branch_oid(repo, branch_name) {
        Ok(oid) => oid,
        Err(e) => {
            blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                message: e.to_string(),
            }));
            git2::Oid::ZERO_SHA1
        }
    };

    let (remote_name, upstream_oid, has_upstream) = if set_upstream {
        match choose_push_remote(repo) {
            Ok(remote) => (remote, None, false),
            Err(e) => {
                blockers.push(PlanNote::Common(CommonNote::GitErrorPassthrough {
                    message: e.to_string(),
                }));
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
                blockers.push(PlanNote::Push(PushNote::NoUpstreamWithErr {
                    branch: branch_name.to_string(),
                    err: e.to_string(),
                }));
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

    // ADR-0129 F-2: same no-op semantics as plan_push (sole up-to-date blocker).
    let nothing_to_push = blockers.is_empty() && has_upstream && ahead_count == 0;
    if nothing_to_push {
        blockers.push(PlanNote::Push(PushNote::AlreadyUpToDate {
            branch: branch_name.to_string(),
            punct: PushPunct::Semicolon,
        }));
    }

    let preview_commits = if blockers.is_empty() {
        build_push_preview_for_oid(repo, local_oid, upstream_oid).unwrap_or_default()
    } else {
        Vec::new()
    };

    let title = PlanTitle::Push(PushTitle::PushBranch {
        branch: branch_name.to_string(),
        remote: remote_name.clone(),
        set_upstream,
    });

    Ok(OperationPlan {
        disposition: if nothing_to_push {
            PlanDisposition::NoOp(NoOpKind::PushUpToDate)
        } else {
            PlanDisposition::for_blockers(&blockers)
        },
        title,
        current,
        predicted: StateSummary {
            head: format!("branch: {} (pushed {} commit(s))", branch_name, ahead_count),
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Push(PushRecovery::PushBranch),
            commands: Vec::new(),
        }),
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
    // ADR-0129 appendix §B-5 / §A15: `Branch '{}' does not exist.` is the
    // cross-op `CommonNote::BranchMissing { in_repo: false }` tail (shared in
    // English with rename); the rest are this op's own `PushNote` variants.
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if repo.find_branch(branch_name, BranchType::Local).is_err() {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: branch_name.to_string(),
            in_repo: false,
        }));
    }
    if upstream.trim().is_empty() || upstream.trim() != upstream {
        blockers.push(PlanNote::Push(PushNote::UpstreamFormatInvalid));
    } else if repo.find_branch(upstream, BranchType::Remote).is_err() {
        warnings.push(PlanNote::Push(PushNote::UpstreamNotPresentLocally {
            upstream: upstream.to_string(),
        }));
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Push(PushTitle::SetUpstream {
            branch: branch_name.to_string(),
            upstream: upstream.to_string(),
        }),
        current,
        predicted: StateSummary {
            head: format!("branch: {} -> {}", branch_name, upstream),
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Push(PushRecovery::SetUpstream {
                branch: branch_name.to_string(),
            }),
            commands: Vec::new(),
        }),
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
