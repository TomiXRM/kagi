//! Worktree lifecycle operations — remove / lock / prune / repair (issue #340).
//!
//! Each op follows the `plan → confirm → preflight → execute → verify → oplog`
//! path. The one destructive step (removing a worktree's working directory) is
//! routed through [`remove_worktree_dir_checked`], a containment-checked delete
//! that refuses the main worktree, any repo-overlapping path, or a symlinked
//! target. `ops/branch.rs` reuses the same checked path (closing the codebase's
//! only unbounded `remove_dir_all`, flagged in #294).

use super::*;
use git2::WorktreeLockStatus;
use kagi_domain::plan_note::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

// ────────────────────────────────────────────────────────────
// Containment-checked worktree directory removal (the safety hole fix)
// ────────────────────────────────────────────────────────────

/// Recursively delete a worktree's working directory **only** after proving it
/// is safe to do so. This is the single place in the codebase allowed to
/// `remove_dir_all` a worktree path.
///
/// Refuses (returns `Err`, deletes nothing) when the target:
/// - is a symlink (never followed into a delete),
/// - resolves to the main worktree,
/// - overlaps the main repository (is an ancestor of, or lives inside, it).
///
/// `main_workdir` is the main repo's working directory; `wt_path` is the
/// registered worktree path (from `git2::Worktree::path`).
pub(crate) fn remove_worktree_dir_checked(
    main_workdir: &Path,
    wt_path: &Path,
) -> Result<(), GitError> {
    // A symlinked worktree path is refused outright: canonicalizing it would
    // follow the link and a recursive delete could then escape the repo tree.
    let meta = std::fs::symlink_metadata(wt_path).map_err(|e| {
        GitError::Other(format!(
            "cannot stat worktree directory '{}': {e}",
            wt_path.display()
        ))
    })?;
    if meta.file_type().is_symlink() {
        return Err(GitError::Other(format!(
            "refusing to delete worktree path '{}': it is a symlink",
            wt_path.display()
        )));
    }

    let target = std::fs::canonicalize(wt_path).map_err(|e| {
        GitError::Other(format!(
            "cannot resolve worktree directory '{}': {e}",
            wt_path.display()
        ))
    })?;
    let main = std::fs::canonicalize(main_workdir).map_err(|e| {
        GitError::Other(format!(
            "cannot resolve main worktree '{}': {e}",
            main_workdir.display()
        ))
    })?;

    if target == main {
        return Err(GitError::Other(
            "refusing to delete the main worktree".to_string(),
        ));
    }
    // The catastrophic case: the target is an ANCESTOR of the repo, so a
    // recursive delete would take the repository (or the filesystem root) with
    // it. A worktree nested *inside* the repo is unusual but harmless to delete
    // — only the ancestor direction is refused.
    if main.starts_with(&target) {
        return Err(GitError::Other(format!(
            "refusing to delete '{}': it contains the main repository at '{}'",
            target.display(),
            main.display()
        )));
    }

    std::fs::remove_dir_all(&target).map_err(|e| {
        GitError::Other(format!(
            "failed to remove worktree directory '{}': {e}",
            target.display()
        ))
    })
}

// ────────────────────────────────────────────────────────────
// shared helpers
// ────────────────────────────────────────────────────────────

/// Build an admin/ref-only plan skeleton (HEAD of the main repo is unchanged by
/// all four lifecycle ops), letting each op supply its own notes/title/recovery.
pub(super) fn admin_plan(
    repo: &Repository,
    title: WorktreeTitle,
    warnings: Vec<PlanNote>,
    blockers: Vec<PlanNote>,
    recovery: Option<PlanRecovery>,
    destructive: bool,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);
    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Worktree(title),
        current: StateSummary {
            head: head.display(),
            dirty: dirty.clone(),
        },
        predicted: StateSummary {
            head: head.display(),
            dirty,
        },
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive,
        equivalent_command: None,
    })
}

/// Open a linked worktree as its own repo and return `(branch, dirty_summary)`.
/// `dirty_summary` is `None` when the worktree is clean or unreadable-as-clean.
pub(super) fn worktree_branch_and_dirt(wt: &git2::Worktree) -> (Option<String>, Option<String>) {
    let Ok(wt_repo) = Repository::open_from_worktree(wt) else {
        return (None, None);
    };
    let branch = wt_repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(str::to_string));
    let dirty = match working_tree_status(&wt_repo) {
        Ok(st) if st.is_dirty() => Some(status_summary_display(&st)),
        _ => None,
    };
    (branch, dirty)
}

pub(super) fn lock_reason(wt: &git2::Worktree) -> Option<String> {
    match wt.is_locked() {
        Ok(WorktreeLockStatus::Locked(reason)) => reason
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────
// lock
// ────────────────────────────────────────────────────────────

/// Analyse whether locking the linked worktree `name` with `reason` is safe.
/// Lock is ref/admin-only and never destructive. Already-locked / missing are
/// blockers (no-op family).
pub fn plan_lock_worktree(
    repo: &Repository,
    name: &str,
    reason: Option<&str>,
) -> Result<OperationPlan, GitError> {
    let title = WorktreeTitle::LockWorktree {
        name: name.to_string(),
    };
    let reason = reason
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);

    let wt = match repo.find_worktree(name) {
        Ok(wt) => wt,
        Err(_) => {
            return admin_plan(
                repo,
                title,
                Vec::new(),
                vec![PlanNote::Worktree(WorktreeNote::WorktreeMissing {
                    name: name.to_string(),
                })],
                None,
                false,
            );
        }
    };

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    if matches!(wt.is_locked(), Ok(WorktreeLockStatus::Locked(_))) {
        blockers.push(PlanNote::Worktree(WorktreeNote::AlreadyLocked {
            name: name.to_string(),
            reason: lock_reason(&wt),
        }));
    } else {
        warnings.push(PlanNote::Worktree(WorktreeNote::LocksWorktree {
            path: wt.path().display().to_string(),
            reason: reason.clone(),
        }));
    }

    let recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::LockWorktree {
            name: name.to_string(),
        }),
        commands: vec![format!("git worktree unlock <path-of-{}>", name)],
    });
    admin_plan(repo, title, warnings, blockers, recovery, false)
}

/// Lock the linked worktree `name`: preflight → lock → verify.
pub(crate) fn execute_lock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    reason: Option<&str>,
) -> Result<(), GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(
            "lock-worktree refused: plan has blockers".to_string(),
        ));
    }
    preflight_check(repo, plan)?;
    let reason = reason.map(str::trim).filter(|r| !r.is_empty());

    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("worktree '{}' not found: {}", name, e.message())))?;
    if matches!(wt.is_locked(), Ok(WorktreeLockStatus::Locked(_))) {
        return Err(GitError::Other(format!(
            "worktree '{}' is already locked",
            name
        )));
    }
    wt.lock(reason)
        .map_err(|e| GitError::Other(format!("worktree lock failed: {}", e.message())))?;
    match wt.is_locked() {
        Ok(WorktreeLockStatus::Locked(_)) => Ok(()),
        _ => Err(GitError::Other(format!(
            "worktree '{}' not locked after lock — unexpected state",
            name
        ))),
    }
}

// ────────────────────────────────────────────────────────────
// prune
// ────────────────────────────────────────────────────────────

/// Collect the registered worktrees git considers prunable (working directory
/// gone / admin entry stale). kagi selects the targets itself — it never shells
/// out to a blind `git worktree prune`.
///
/// issue #372 item 1: bulk prune is scoped to **kagi-created** worktrees only.
/// A worktree added by hand (`git worktree add`, no `.kagi-created` marker) is
/// excluded — a bulk op must never touch a directory the user set up outside
/// kagi. (Removing one specific hand-added worktree is still allowed via the
/// explicit single-worktree remove path.)
fn prunable_worktrees(repo: &Repository) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(names) = repo.worktrees() else {
        return out;
    };
    for name in names.iter().filter_map(|r| r.ok().flatten()) {
        if !is_kagi_created(repo, name) {
            continue; // hand-added — out of scope for bulk prune
        }
        if let Ok(wt) = repo.find_worktree(name) {
            // Default options: only worktrees whose working directory is gone
            // (and that are not locked) count as prunable. Setting `valid(true)`
            // here would wrongly mark *live* worktrees prunable.
            if wt.is_prunable(None).unwrap_or(false) {
                out.push((name.to_string(), wt.path().display().to_string()));
            }
        }
    }
    out
}

/// Analyse the prune. Shows a dry-run preview (count + paths); a no-op when
/// nothing is prunable is a blocker.
pub fn plan_prune_worktrees(repo: &Repository) -> Result<OperationPlan, GitError> {
    const SAMPLE: usize = 5;
    let targets = prunable_worktrees(repo);

    let (warnings, blockers) = if targets.is_empty() {
        (
            Vec::new(),
            vec![PlanNote::Worktree(WorktreeNote::PruneNothing)],
        )
    } else {
        let sample: Vec<String> = targets
            .iter()
            .take(SAMPLE)
            .map(|(_, p)| p.clone())
            .collect();
        (
            vec![PlanNote::Worktree(WorktreeNote::PrunePreview {
                count: targets.len(),
                sample,
                more: targets.len().saturating_sub(SAMPLE),
            })],
            Vec::new(),
        )
    };

    let recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::Prune),
        commands: vec!["git worktree add <path> <branch>".to_string()],
    });
    // Prune only drops stale admin entries whose workdir is already gone — no
    // working tree is deleted, so it is not destructive.
    admin_plan(
        repo,
        WorktreeTitle::PruneWorktrees,
        warnings,
        blockers,
        recovery,
        false,
    )
}

/// Prune the stale worktree admin entries kagi selected: preflight → prune each
/// → verify none remain prunable.
pub(crate) fn execute_prune_worktrees(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<usize, GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(
            "prune-worktrees refused: plan has blockers".to_string(),
        ));
    }
    preflight_check(repo, plan)?;

    let targets = prunable_worktrees(repo);
    let mut pruned = 0;
    for (name, _) in &targets {
        let wt = repo.find_worktree(name).map_err(|e| {
            GitError::Other(format!(
                "worktree '{}' lookup failed: {}",
                name,
                e.message()
            ))
        })?;
        // Default options — these were already detected prunable (workdir gone).
        wt.prune(None).map_err(|e| {
            GitError::Other(format!(
                "worktree prune failed for '{}': {}",
                name,
                e.message()
            ))
        })?;
        pruned += 1;
    }

    if !prunable_worktrees(repo).is_empty() {
        return Err(GitError::Other(
            "prunable worktrees remain after prune — unexpected state".to_string(),
        ));
    }
    Ok(pruned)
}

// ────────────────────────────────────────────────────────────
// repair
// ────────────────────────────────────────────────────────────

/// Analyse the repair. Repair is idempotent and only fixes `.git` links (never
/// touches files), so it carries no blockers — the plan's value is its
/// description of the three failure modes it fixes.
pub fn plan_repair_worktrees(repo: &Repository) -> Result<OperationPlan, GitError> {
    let recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::Repair),
        commands: vec!["git worktree repair".to_string()],
    });
    admin_plan(
        repo,
        WorktreeTitle::RepairWorktrees,
        vec![PlanNote::Worktree(WorktreeNote::RepairsWorktrees)],
        Vec::new(),
        recovery,
        false,
    )
}

/// Registered worktrees whose working directory still exists but whose admin
/// link does not resolve — i.e. repair should have fixed them but did not. A
/// worktree whose directory is *gone* is prunable, not repairable, so it is
/// excluded (repair legitimately leaves those). Used as the repair verify step.
fn unrepaired_worktrees(repo: &Repository) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(names) = repo.worktrees() else {
        return out;
    };
    for name in names.iter().filter_map(|r| r.ok().flatten()) {
        if let Ok(wt) = repo.find_worktree(name) {
            if wt.path().exists() && wt.validate().is_err() {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Repair worktree links via `git worktree repair` (libgit2 has no equivalent).
///
/// `git worktree repair` returns a non-zero exit when it cannot fix a link
/// (read-only `.git` file, unresolved parent, permissions) — and `run_git`
/// surfaces that in `out.status`, not as `Err` (issue #391, same class as #296).
/// It can also exit 0 having repaired only *some* links, so the exit check is
/// paired with a verify pass, exactly as `execute_prune_worktrees` verifies.
pub(crate) fn execute_repair_worktrees(
    repo: &Repository,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    let repo_dir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?
        .to_path_buf();
    let out = run_git(&repo_dir, &["worktree", "repair"])?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "worktree repair failed (exit {}): {}",
            out.status,
            out.stderr.trim()
        )));
    }
    // Verify: a repair that exited 0 may still have left links unrepaired.
    let unrepaired = unrepaired_worktrees(repo);
    if !unrepaired.is_empty() {
        return Err(GitError::Other(format!(
            "worktree repair left {} link(s) unrepaired: {}",
            unrepaired.len(),
            unrepaired.join(", ")
        )));
    }
    Ok(())
}
