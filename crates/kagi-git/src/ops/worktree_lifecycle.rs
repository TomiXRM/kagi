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
use kagi_domain::remote::shell_quote;
use kagi_domain::remove::{RepoId, WorktreeId};
use kagi_domain::worktree_autolock::{
    classify_auto_unlock, AutoLockToken, AutoUnlockRace, AutoUnlockTarget, LOCK_ASIDE_PREFIX,
};

// ────────────────────────────────────────────────────────────
// Containment-checked worktree directory removal (the safety hole fix)
// ────────────────────────────────────────────────────────────

/// Recursively delete a worktree's working directory **only** after proving it
/// is safe to do so. This is the single place in the codebase allowed to
/// `remove_dir_all` a worktree path.
///
/// Refuses (returns `Err`, deletes nothing) when the target:
/// - is a symlink (never followed into a delete),
/// - resolves to or contains the managing worktree, main worktree or common dir.
///
/// `wt_path` is the registered worktree path.
pub(crate) fn remove_worktree_dir_checked(
    managing_workdir: Option<&Path>,
    main_workdir: Option<&Path>,
    common_dir: &Path,
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
    for (root, label) in [
        (managing_workdir, "managing worktree"),
        (main_workdir, "main worktree"),
        (Some(common_dir), "common repository"),
    ] {
        let Some(root) = root else { continue };
        let protected = std::fs::canonicalize(root).map_err(|e| {
            GitError::Other(format!("cannot resolve {label} '{}': {e}", root.display()))
        })?;
        if protected.starts_with(&target) {
            return Err(GitError::Other(format!(
                "refusing to delete '{}': it contains the {label} at '{}'",
                target.display(),
                protected.display()
            )));
        }
    }

    std::fs::remove_dir_all(&target).map_err(|e| {
        GitError::Other(format!(
            "failed to remove worktree directory '{}': {e}",
            target.display()
        ))
    })
}

#[cfg(test)]
mod checked_delete_tests {
    use super::*;

    #[test]
    fn managing_worktree_is_never_recursively_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("linked");
        let main = tmp.path().join("main");
        let common = main.join(".git");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(target.join("keep"), "linked content").unwrap();
        let err =
            remove_worktree_dir_checked(Some(&target), Some(&main), &common, &target).unwrap_err();
        assert!(err.to_string().contains("managing worktree"), "{err}");
        assert_eq!(
            std::fs::read_to_string(target.join("keep")).unwrap(),
            "linked content"
        );
    }

    #[test]
    fn nonbare_common_dir_under_target_is_never_recursively_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let target = tmp.path().join("linked");
        let common = target.join("common.git");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir_all(common.join("objects")).unwrap();
        std::fs::write(common.join("objects/keep"), "retained").unwrap();
        let err =
            remove_worktree_dir_checked(Some(&main), Some(&main), &common, &target).unwrap_err();
        assert!(err.to_string().contains("common repository"), "{err}");
        assert_eq!(
            std::fs::read_to_string(common.join("objects/keep")).unwrap(),
            "retained"
        );
    }
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
/// Remove checks gitlinks separately, so missing paths do not count as dirt.
pub(super) fn worktree_branch_and_dirt(wt: &git2::Worktree) -> (Option<String>, Option<String>) {
    let Ok(wt_repo) = Repository::open_from_worktree(wt) else {
        return (None, None);
    };
    let branch = wt_repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(str::to_string));
    let dirty = match crate::status::working_tree_status_for_remove(&wt_repo) {
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
        commands: vec![format!(
            "git worktree unlock <path-of-{}>",
            shell_quote(name)
        )],
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
// auto-unlock (#772 Phase 1, ADR-0208 決定 2)
// ────────────────────────────────────────────────────────────

/// The identity of the linked worktree registered as `name`, in the same
/// canonical form `Backend::write_worktree_id` produces when opened *at* that
/// worktree — so a session's recorded target and this read compare equal.
pub fn linked_worktree_identity(repo: &Repository, name: &str) -> Result<WorktreeId, GitError> {
    let common = repo.commondir();
    let canon = |p: &Path| {
        std::fs::canonicalize(p)
            .map_err(|e| GitError::Other(format!("cannot resolve '{}': {e}", p.display())))
    };
    Ok(WorktreeId {
        repo: RepoId(canon(common)?),
        git_dir: canon(&common.join("worktrees").join(name))?,
    })
}

fn admin_dir(repo: &Repository, name: &str) -> PathBuf {
    repo.commondir().join("worktrees").join(name)
}

/// #836: the `locked.kagi-*` files an interrupted release left for `name`, as
/// a note — `None` when there are none. Never cleaned up here (contract D).
pub fn lock_leftover_note(repo: &Repository, name: &str) -> Option<PlanNote> {
    let dir = admin_dir(repo, name);
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|file| file.starts_with(LOCK_ASIDE_PREFIX))
        .collect();
    files.sort();
    (!files.is_empty()).then(|| {
        PlanNote::Worktree(WorktreeNote::LockLeftover {
            name: name.to_string(),
            dir: dir.display().to_string(),
            files,
        })
    })
}

/// Classify the lock currently on `name` against what the session expects.
/// Shared by plan and preflight so the two can never disagree.
fn auto_unlock_check(
    repo: &Repository,
    name: &str,
    target: &AutoUnlockTarget,
) -> Result<git2::Worktree, PlanNote> {
    let wt = repo.find_worktree(name).map_err(|_| {
        PlanNote::Worktree(WorktreeNote::WorktreeMissing {
            name: name.to_string(),
        })
    })?;
    // A leftover means an earlier release was interrupted after moving the
    // lock aside: nothing is released until a person has looked at it.
    if let Some(leftover) = lock_leftover_note(repo, name) {
        return Err(leftover);
    }
    let (locked, reason) = match wt.is_locked() {
        Ok(WorktreeLockStatus::Locked(reason)) => (true, reason),
        Ok(WorktreeLockStatus::Unlocked) => (false, None),
        Err(e) => {
            return Err(PlanNote::Worktree(WorktreeNote::LockStateUnreadable {
                name: name.to_string(),
                err: e.message().to_string(),
            }))
        }
    };
    let identity = linked_worktree_identity(repo, name).map_err(|e| {
        PlanNote::Worktree(WorktreeNote::LockStateUnreadable {
            name: name.to_string(),
            err: e.to_string(),
        })
    })?;
    classify_auto_unlock(locked, reason.as_deref(), &identity, target)
        .map_err(|refusal| {
            PlanNote::Worktree(WorktreeNote::AutoUnlockRefused {
                name: name.to_string(),
                refusal,
            })
        })
        .map(|()| wt)
}

/// Analyse whether the lock on `name` may be released by the terminal session
/// holding `target`. Unlike [`plan_unlock_worktree`](super::plan_unlock_worktree)
/// this refuses anything that is not *this session's* Kagi lock on *this*
/// worktree (contract B): manual and foreign locks, another session's token,
/// or a worktree other than the one recorded, are blockers, never warnings.
pub fn plan_auto_unlock_worktree(
    repo: &Repository,
    name: &str,
    target: &AutoUnlockTarget,
) -> Result<OperationPlan, GitError> {
    let title = WorktreeTitle::AutoUnlockWorktree {
        name: name.to_string(),
    };
    let (warnings, blockers) = match auto_unlock_check(repo, name, target) {
        Ok(_) => (
            vec![PlanNote::Worktree(WorktreeNote::LockedWithReason {
                reason: Some(target.token.reason()),
            })],
            Vec::new(),
        ),
        Err(blocker) => (Vec::new(), vec![blocker]),
    };
    let recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::Unlock {
            name: name.to_string(),
        }),
        commands: vec![format!(
            "git worktree lock --reason {} <path-of-{}>",
            shell_quote(&target.token.reason()),
            shell_quote(name)
        )],
    });
    admin_plan(repo, title, warnings, blockers, recovery, false)
}

/// Re-run the ownership check immediately before releasing: HEAD unchanged
/// (`preflight_check`), no interrupted-release leftover, and the lock is
/// still this session's token on this worktree. The release itself then
/// compares again, atomically ([`release_lock_file`]).
pub fn preflight_auto_unlock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    target: &AutoUnlockTarget,
) -> Result<git2::Worktree, GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(
            "auto-unlock-worktree refused: plan has blockers".to_string(),
        ));
    }
    preflight_check(repo, plan)?;
    auto_unlock_check(repo, name, target).map_err(|note| {
        GitError::Other(format!(
            "auto-unlock-worktree refused at preflight: {}",
            note.message_en()
        ))
    })
}

/// Release this session's Kagi lock on `name`: preflight → compare-and-unlock
/// → verify (ADR-0212). `race` is the test seam; production passes `None`.
pub(crate) fn execute_auto_unlock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    target: &AutoUnlockTarget,
    race: Option<&AutoUnlockRace>,
) -> Result<(), GitError> {
    let wt = preflight_auto_unlock_worktree(repo, plan, name, target)?;
    release_lock_file(&admin_dir(repo, name), &target.token.reason(), race)?;
    // Verify: no lock carrying this session's token remains. A lock someone
    // else placed after ours went away is theirs to keep.
    match wt.is_locked() {
        Ok(WorktreeLockStatus::Unlocked) => Ok(()),
        Ok(WorktreeLockStatus::Locked(reason))
            if AutoLockToken::parse(reason.as_deref()).as_ref() != Some(&target.token) =>
        {
            Ok(())
        }
        _ => Err(GitError::Other(format!(
            "worktree '{}' still carries this terminal's lock after auto-unlock — unexpected state",
            name
        ))),
    }
}

/// Compare-and-unlock on `<admin>/locked` (#836). Git has no such operation,
/// and libgit2's `unlock` deletes whatever lock is there. Instead:
///
/// 1. `rename` the lock aside (atomic): from then on only this call holds
///    that file, whoever wrote it. No lock at all → already released, `Ok`.
/// 2. Read the moved file. This session's token → delete it: released.
/// 3. Anyone else's → put it back with `hard_link`, which fails instead of
///    replacing a lock that appeared meanwhile; then the moved file stays as a
///    `locked.kagi-*` leftover for a person to resolve (contract D).
fn release_lock_file(
    admin: &Path,
    token: &str,
    race: Option<&AutoUnlockRace>,
) -> Result<(), GitError> {
    let locked = admin.join("locked");
    let relock = |reason: &str| {
        let _ = std::fs::write(&locked, reason);
    };
    match race {
        Some(AutoUnlockRace::UnlockBeforeMove) => {
            let _ = std::fs::remove_file(&locked);
        }
        Some(AutoUnlockRace::RelockBeforeMove(reason))
        | Some(AutoUnlockRace::RelockAroundMove { before: reason, .. }) => relock(reason),
        _ => {}
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let aside = admin.join(format!("{LOCK_ASIDE_PREFIX}{}-{nonce}", std::process::id()));
    match std::fs::rename(&locked, &aside) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(GitError::Other(format!(
                "cannot move the worktree lock aside: {e}"
            )))
        }
    }
    match race {
        Some(AutoUnlockRace::RelockAfterMove(reason))
        | Some(AutoUnlockRace::RelockAroundMove { after: reason, .. }) => relock(reason),
        _ => {}
    }
    // git writes `reason\n`, libgit2 the bare reason.
    let found = std::fs::read_to_string(&aside).unwrap_or_default();
    if found.trim_end_matches(['\n', '\r']) == token {
        return std::fs::remove_file(&aside).map_err(|e| {
            GitError::Other(format!(
                "the lock was moved to {} but could not be removed: {e}",
                aside.display()
            ))
        });
    }
    match std::fs::hard_link(&aside, &locked) {
        Ok(()) => {
            let _ = std::fs::remove_file(&aside);
            Err(GitError::Other(
                "auto-unlock-worktree refused: the lock was no longer this terminal's when it was released; it was put back untouched"
                    .to_string(),
            ))
        }
        Err(e) => Err(GitError::Other(format!(
            "auto-unlock-worktree refused: the lock was no longer this terminal's, and a newer lock appeared before it could be put back ({e}); the moved lock is kept at {}",
            aside.display()
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
