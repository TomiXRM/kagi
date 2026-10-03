//! Remove plan/preflight/executor and defensive backups (#484 slice 1a).
use super::worktree_lifecycle::{admin_plan, lock_reason, worktree_branch_and_dirt};
use super::*;
use git2::{WorktreeLockStatus, WorktreePruneOptions};
use kagi_domain::plan_note::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

// ────────────────────────────────────────────────────────────
// remove
// ────────────────────────────────────────────────────────────

/// Analyse whether removing the linked worktree `name` is safe.
///
/// Blockers: the main worktree (never removable), a dirty, locked, or
/// submodule-containing worktree, or a missing worktree. `delete_branch`
/// controls whether the plan also promises to delete the checked-out branch.
/// Warnings: the removal itself, any ignored content it deletes (#934), and
/// the `pre_remove` steps.
pub fn plan_remove_worktree(
    repo: &Repository,
    name: &str,
    delete_branch: bool,
) -> Result<OperationPlan, GitError> {
    let title = WorktreeTitle::RemoveWorktree {
        name: name.to_string(),
    };

    let wt = match repo.find_worktree(name) {
        Ok(wt) => wt,
        Err(_) => {
            // The main worktree has no admin entry, so a request to remove it
            // lands here — refuse it explicitly rather than reporting "missing".
            let blocker = if name == "main" {
                WorktreeNote::RemoveMainRefused
            } else {
                WorktreeNote::WorktreeMissing {
                    name: name.to_string(),
                }
            };
            return admin_plan(
                repo,
                title,
                Vec::new(),
                vec![PlanNote::Worktree(blocker)],
                None,
                false,
            );
        }
    };

    let path = wt.path().to_path_buf();
    let path_str = path.display().to_string();
    let (branch, dirt) = worktree_branch_and_dirt(&wt);

    let mut blockers = Vec::new();
    if let Some(blocker) = nested_registered_worktree(repo, name, &path)? {
        blockers.push(blocker);
    }
    if has_submodule_content(&wt)? {
        blockers.push(PlanNote::Worktree(WorktreeNote::RemoveContainsSubmodules));
    }
    if let Some(summary) = dirt {
        blockers.push(PlanNote::Worktree(WorktreeNote::RemoveDirty {
            path: path_str.clone(),
            summary,
        }));
    }
    if matches!(wt.is_locked(), Ok(WorktreeLockStatus::Locked(_))) {
        blockers.push(PlanNote::Worktree(WorktreeNote::RemoveLocked {
            path: path_str.clone(),
            reason: lock_reason(&wt),
        }));
    }

    let mut warnings = vec![PlanNote::Worktree(WorktreeNote::RemovesWorktree {
        path: path_str.clone(),
        branch: branch.clone(),
        delete_branch,
    })];
    if let Some(note) = ignored_content_note(&wt, &path_str) {
        warnings.push(note);
    }
    // issue #341: enumerate the typed pre_remove steps from the worktree's own
    // committed config. A command step in an untrusted config marks the note
    // trust-required (and, at execute time, aborts the removal until trusted).
    if let Ok(Some(cfg)) = load_worktree_config(&path) {
        if let Some(note) = pre_remove_note(&cfg) {
            warnings.push(note);
        }
    }
    let recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::RemoveWorktree {
            path: path_str,
            branch: branch.clone(),
        }),
        commands: vec![format!(
            "git worktree add {} {}",
            path.display(),
            branch.as_deref().unwrap_or("<branch>")
        )],
    });

    admin_plan(repo, title, warnings, blockers, recovery, true)
}

/// A target may contain another registered worktree under an ignored
/// directory. Its files are not in the target's dirty status or backups.
/// Ignore only the target's own registration; protect every other linked
/// worktree, including the manager, and the main worktree when present.
fn nested_registered_worktree(
    repo: &Repository,
    name: &str,
    target_path: &Path,
) -> Result<Option<PlanNote>, GitError> {
    let target = std::fs::canonicalize(target_path)
        .map_err(|e| GitError::Other(format!("cannot resolve removal target: {e}")))?;
    let names = repo
        .worktrees()
        .map_err(|e| GitError::Other(format!("cannot list registered worktrees: {e}")))?;
    for registered in names.iter() {
        let registered = registered
            .map_err(|e| GitError::Other(format!("cannot read registered worktree name: {e}")))?
            .ok_or_else(|| GitError::Other("invalid registered worktree name".into()))?;
        if registered == name {
            continue;
        }
        let other = repo
            .find_worktree(registered)
            .map_err(|e| GitError::Other(format!("cannot inspect registered worktree: {e}")))?;
        let path = match std::fs::canonicalize(other.path()) {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => other.path().to_path_buf(),
            Err(e) => {
                return Err(GitError::Other(format!(
                    "cannot resolve registered worktree: {e}"
                )));
            }
        };
        if path.starts_with(&target) {
            return Ok(Some(PlanNote::Worktree(
                WorktreeNote::RemoveContainsWorktree {
                    path: path.display().to_string(),
                },
            )));
        }
    }
    let main = Repository::open(repo.commondir())
        .map_err(|e| GitError::Other(format!("cannot open common repository: {e}")))?;
    if let Some(workdir) = main.workdir() {
        let path = std::fs::canonicalize(workdir)
            .map_err(|e| GitError::Other(format!("cannot resolve main worktree: {e}")))?;
        if path.starts_with(&target) {
            return Ok(Some(PlanNote::Worktree(
                WorktreeNote::RemoveContainsWorktree {
                    path: path.display().to_string(),
                },
            )));
        }
    }
    Ok(None)
}

/// Re-check the same registered paths before hooks and immediately before
/// deletion. A newly nested worktree must stop even when the target stays
/// clean because its parent directory is ignored.
pub(crate) fn preflight_remove_nested_worktrees(
    repo: &Repository,
    name: &str,
) -> Result<(), GitError> {
    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("cannot inspect removal target: {e}")))?;
    if let Some(blocker) = nested_registered_worktree(repo, name, wt.path())? {
        return Err(GitError::Blocked(Box::new(blocker)));
    }
    Ok(())
}

/// A populated gitlink path is not safe to remove, even when the submodule is
/// uninitialized: Git status and ignored scans omit files inside that path.
/// Check only direct entries, without walking the submodule or following a
/// symlink at the path. A missing or empty directory remains removable.
fn has_submodule_content(wt: &git2::Worktree) -> Result<bool, GitError> {
    let wt_repo = Repository::open_from_worktree(wt)
        .map_err(|e| GitError::Other(format!("cannot inspect worktree submodules: {e}")))?;
    let workdir = wt_repo
        .workdir()
        .ok_or_else(|| GitError::Other("worktree has no working directory".into()))?;
    for submodule in wt_repo
        .submodules()
        .map_err(|e| GitError::Other(format!("cannot inspect worktree submodules: {e}")))?
    {
        let path = workdir.join(submodule.path());
        match std::fs::symlink_metadata(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(GitError::Other(format!(
                    "cannot inspect worktree submodule: {err}"
                )));
            }
            Ok(metadata) if !metadata.is_dir() => return Ok(true),
            Ok(_) => {}
        }
        let mut entries = std::fs::read_dir(&path)
            .map_err(|e| GitError::Other(format!("cannot inspect worktree submodule: {e}")))?;
        if let Some(entry) = entries.next() {
            entry
                .map_err(|e| GitError::Other(format!("cannot inspect worktree submodule: {e}")))?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// A linked worktree can acquire submodule content after confirmation or during
/// a pre-remove step. Refuse it through the same typed plan blocker.
pub(crate) fn preflight_remove_submodules(repo: &Repository, name: &str) -> Result<(), GitError> {
    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("cannot inspect worktree submodules: {e}")))?;
    if has_submodule_content(&wt)? {
        return Err(GitError::Blocked(Box::new(PlanNote::Worktree(
            WorktreeNote::RemoveContainsSubmodules,
        ))));
    }
    Ok(())
}

/// A read-only status walk of the linked worktree's ignored content. Git
/// reports an ignored directory as one entry without descending into it;
/// symlinks are entries, never followed. Fail closed at execution time.
fn ignored_content_counts(wt: &git2::Worktree) -> Result<(usize, usize), GitError> {
    let wt_repo = Repository::open_from_worktree(wt)
        .map_err(|e| GitError::Other(format!("cannot inspect ignored worktree content: {e}")))?;
    let mut opts = git2::StatusOptions::new();
    opts.include_ignored(true)
        .recurse_ignored_dirs(false)
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true);
    let statuses = wt_repo
        .statuses(Some(&mut opts))
        .map_err(|e| GitError::Other(format!("cannot inspect ignored worktree content: {e}")))?;
    let (mut files, mut folders) = (0, 0);
    for entry in statuses.iter().filter(|entry| entry.status().is_ignored()) {
        if entry.path_bytes().ends_with(b"/") {
            folders += 1;
        } else {
            files += 1;
        }
    }
    Ok((files, folders))
}

/// The confirmed plan carries the counts in its warning, including zero when
/// it has no ignored-content warning. Re-plan blockers alone do not preserve
/// what the user saw (#936).
fn ensure_ignored_content_not_increased(
    wt: &git2::Worktree,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    let approved = plan
        .warnings
        .iter()
        .find_map(|note| match note {
            PlanNote::Worktree(WorktreeNote::RemoveIgnoredFiles { files, folders, .. }) => {
                Some((*files, *folders))
            }
            _ => None,
        })
        .unwrap_or((0, 0));
    let current = ignored_content_counts(wt)?;
    if current.0 > approved.0 || current.1 > approved.1 {
        return Err(GitError::Blocked(Box::new(PlanNote::Worktree(
            WorktreeNote::RemoveIgnoredContentChanged,
        ))));
    }
    Ok(())
}

/// Refuse new unreviewed ignored content before trust or pre_remove steps run.
pub(crate) fn preflight_remove_ignored_content(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
) -> Result<(), GitError> {
    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("cannot inspect worktree ignored content: {e}")))?;
    ensure_ignored_content_not_increased(&wt, plan)
}

/// The warning shown when the worktree could be read during planning.
fn ignored_content_note(wt: &git2::Worktree, path: &str) -> Option<PlanNote> {
    let (files, folders) = ignored_content_counts(wt).ok()?;
    (files + folders > 0).then(|| {
        PlanNote::Worktree(WorktreeNote::RemoveIgnoredFiles {
            path: path.to_string(),
            files,
            folders,
        })
    })
}

/// Remove the linked worktree `name`: preflight → ODB-backup any uncommitted
/// content (defensive; the plan already blocks dirt) → containment-checked
/// directory delete → prune admin entry → optionally delete the branch → verify.
///
/// Returns a [`DiscardOutcome`] carrying the ODB backups (empty for the normal
/// clean path) so the caller records them in the oplog as a recovery handle. A
/// failure *after* the directory delete began still returns `Ok` with
/// [`DiscardOutcome::error`] set (issue #413, mirroring `execute_discard`) — the
/// backup blob SHAs are the user's only handle on any raced-in uncommitted
/// content, so they must never be dropped by a bare `Err`.
#[cfg(test)]
pub(crate) fn execute_remove_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    delete_branch: bool,
) -> Result<DiscardOutcome, GitError> {
    execute_remove_worktree_progress(
        repo,
        plan,
        name,
        delete_branch,
        &mut kagi_domain::remove::RemoveProgress::default(),
        None,
    )
}

pub(crate) fn execute_remove_worktree_progress(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    delete_branch: bool,
    progress: &mut kagi_domain::remove::RemoveProgress,
    fault: Option<kagi_domain::remove::RemoveFaultPoint>,
) -> Result<DiscardOutcome, GitError> {
    use kagi_domain::remove::{RemoveFaultPoint as Fault, RemoveStage as Stage};
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(format!(
            "remove-worktree refused: plan has {} blocker(s)",
            plan.blockers.len()
        )));
    }
    preflight_check(repo, plan)?;

    // A linked worktree may be the deletion target. Its own workdir cannot
    // anchor containment; use the surviving main workdir, or the shared
    // common dir when the repository is bare (#915, #938).
    let main_repo = Repository::open(repo.commondir())
        .map_err(|e| GitError::Other(format!("cannot open common repository: {e}")))?;
    let protected_root = main_repo
        .workdir()
        .unwrap_or_else(|| main_repo.commondir())
        .to_path_buf();
    // Pre-remove copy/symlink steps still source from a worktree. With a bare
    // common dir, preserve the caller's linked worktree as their source.
    let step_source = main_repo
        .workdir()
        .or_else(|| repo.workdir())
        .ok_or_else(|| GitError::Other("no worktree for pre-remove steps".to_string()))?
        .to_path_buf();

    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("worktree '{}' not found: {}", name, e.message())))?;
    let wt_path = wt.path().to_path_buf();

    // issue #405: close the plan→execute TOCTOU. The plan's dirty/locked blockers
    // are point-in-time and `preflight` only inspects the MAIN repo, so a worktree
    // that turned dirty or got locked since planning would otherwise be deleted
    // anyway. Re-detect on the LINKED worktree now and refuse — the same
    // execute-time re-check `ops/branch.rs` already does ("the world may have
    // changed since planning"). This runs BEFORE any pre_remove command or delete.
    let (_branch_now, dirt_now) = worktree_branch_and_dirt(&wt);
    if let Some(summary) = dirt_now {
        return Err(GitError::Other(format!(
            "worktree '{}' became dirty since planning — refusing to remove (no --force): {}",
            wt_path.display(),
            summary
        )));
    }
    if matches!(wt.is_locked(), Ok(WorktreeLockStatus::Locked(_))) {
        return Err(GitError::Other(format!(
            "worktree '{}' was locked since planning — refusing to remove",
            wt_path.display()
        )));
    }
    preflight_remove_submodules(repo, name)?;
    preflight_remove_nested_worktrees(repo, name)?;

    // Capture the ref before any pre-remove hook. The hook is allowed to take
    // time, so deleting whichever commit the branch points at afterwards would
    // turn a reviewed delete into a delete of a newly-pushed commit.
    let branch: Option<String> = Repository::open(&wt_path).ok().and_then(|r| {
        r.head()
            .ok()
            .and_then(|h| h.shorthand().ok().map(str::to_string))
    });
    progress.branch_tip = branch
        .as_ref()
        .and_then(|name| repo.find_branch(name, git2::BranchType::Local).ok())
        .and_then(|branch| branch.get().target())
        .map(|oid| crate::CommitId(oid.to_string()));

    // issue #341: run the typed pre_remove steps as a precondition of deletion.
    // A failed, untrusted, or headless-blocked command returns Err here — BEFORE
    // any destructive step — so the worktree survives (matches preflight ethos:
    // "docker compose down" failing must not orphan the container by proceeding).
    if let Ok(Some(cfg)) = load_worktree_config(&wt_path) {
        // issue #393: bind execution to the exact config the plan showed. If it
        // changed since planning, refuse rather than run unreviewed content.
        if let Some(expected) = plan_worktree_config_sha(plan) {
            verify_worktree_config_sha(&wt_path, expected)?;
        }
        let trusted = is_worktree_config_trusted(&cfg);
        let env = StepEnv {
            main_root: step_source.clone(),
            worktree: wt_path.clone(),
        };
        super::worktree_steps::run_pre_remove_progress(
            &cfg.steps.pre_remove,
            &env,
            trusted,
            progress,
            fault,
        )?;
    }
    // A pre_remove step can populate a gitlink. Detect it before trying to
    // back up this worktree's files, as well as at the deletion boundary.
    preflight_remove_submodules(repo, name)?;

    // Belt-and-suspenders: the plan blocks dirt, but a race could have dirtied
    // the worktree since. Back up any uncommitted content into the main ODB
    // before the delete so nothing is ever lost (mirrors the discard order).
    odb_backup_worktree(repo, &wt_path, &mut progress.backups)?;
    progress.stage = Stage::BackupCaptured;
    let backups = progress.backups.clone();

    // From here the working directory may be partly deleted. issue #413: every
    // fallible step below returns the backups inside a PARTIAL `DiscardOutcome`
    // instead of a bare `Err`, so the ODB backup blob SHAs always reach the oplog
    // as a recovery handle (they were dropped on every error path before).
    let partial = |err: String| -> Result<DiscardOutcome, GitError> {
        Ok(DiscardOutcome {
            backups: backups.clone(),
            unverified: vec![wt_path.display().to_string()],
            error: Some(err),
        })
    };

    if fault == Some(Fault::FailAfterBackupBeforeDelete) {
        return partial("injected failure after backup".into());
    }

    // `pre_remove` may create ignored output without changing the dirty
    // blocker. Nothing below may delete the tree until that output is compared
    // with the counts the user actually confirmed.
    ensure_ignored_content_not_increased(&wt, plan)?;
    preflight_remove_submodules(repo, name)?;
    preflight_remove_nested_worktrees(repo, name)?;

    // Containment-checked recursive delete (the ONLY sanctioned one).
    progress.stage = Stage::DeletionStarted;
    if let Err(e) = remove_worktree_dir_checked(&protected_root, &wt_path) {
        return partial(e.to_string());
    }
    if fault == Some(Fault::PanicAfterDeletionStarted) {
        panic!("injected executor unwind after delete");
    }
    if fault == Some(Fault::FailAfterDirectoryDelete) {
        return partial("injected failure after directory delete".into());
    }

    // Prune the now-orphaned admin entry.
    progress.observations.push("admin prune started".into());
    let mut opts = WorktreePruneOptions::new();
    opts.valid(true).working_tree(true);
    if let Err(e) = wt.prune(Some(&mut opts)) {
        return partial(format!("worktree prune failed: {}", e.message()));
    }
    progress.stage = Stage::AdminPruned;

    if delete_branch {
        if let Some(ref b) = branch {
            if fault == Some(Fault::MoveBranchBeforeDelete) {
                advance_branch_before_delete_for_test(repo, b, progress.branch_tip.as_ref())?;
            }
            let expected = progress.branch_tip.as_ref().map(|tip| tip.0.as_str());
            let mut branch_ref = match repo.find_branch(b, git2::BranchType::Local) {
                Ok(branch_ref) => branch_ref,
                Err(error) => {
                    progress
                        .observations
                        .push(format!("branch {b} deletion skipped: {}", error.message()));
                    return partial(format!(
                        "worktree removed, but branch '{b}' could not be checked before deletion: {}",
                        error.message()
                    ));
                }
            };
            let actual = branch_ref.get().target().map(|oid| oid.to_string());
            if expected.is_none() || actual.as_deref() != expected {
                let before = expected.unwrap_or("unavailable");
                let after = actual.as_deref().unwrap_or("unavailable");
                progress
                    .observations
                    .push(format!("branch moved {before}→{after}, kept"));
                return partial(format!(
                    "worktree removed, but branch '{b}' moved {before}→{after}, kept"
                ));
            }
            // Delete only the same ref whose target just matched the captured tip.
            progress
                .observations
                .push(format!("branch {b} deletion started"));
            if let Err(e) = branch_ref.delete() {
                return partial(format!(
                    "worktree removed, but branch '{}' delete failed: {}",
                    b,
                    e.message()
                ));
            }
            progress.stage = Stage::BranchDeleted;
        }
    }

    // Verify the admin entry is gone.
    progress.verification = kagi_domain::remove::RemoveVerification::Unknown;
    if repo.find_worktree(name).is_ok() {
        progress.verification =
            kagi_domain::remove::RemoveVerification::Failed("worktree still registered".into());
        return partial(format!(
            "worktree '{}' still registered after remove — unexpected state",
            name
        ));
    }
    progress.stage = Stage::Done;
    progress.verification = kagi_domain::remove::RemoveVerification::Verified;
    progress
        .observations
        .push("verified: admin entry absent".into());
    Ok(DiscardOutcome::complete(backups))
}

/// Advances `branch` only for the finite `MoveBranchBeforeDelete` test fault.
#[doc(hidden)]
fn advance_branch_before_delete_for_test(
    repo: &Repository,
    branch: &str,
    expected: Option<&crate::CommitId>,
) -> Result<(), GitError> {
    let expected =
        expected.ok_or_else(|| GitError::Other("test fault needs a captured branch tip".into()))?;
    let oid = git2::Oid::from_str(&expected.0)
        .map_err(|e| GitError::Other(format!("test fault invalid captured branch tip: {e}")))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(format!("test fault cannot find captured tip: {e}")))?;
    let signature = super::build_signature(repo)?;
    let advanced = repo
        .commit(
            None,
            &signature,
            &signature,
            "test: advance branch before delete",
            &commit
                .tree()
                .map_err(|e| GitError::Other(e.message().to_string()))?,
            &[&commit],
        )
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    repo.reference(
        &format!("refs/heads/{branch}"),
        advanced,
        true,
        "test: move branch before delete",
    )
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(())
}

/// Write every uncommitted file in the worktree at `wt_path` into the MAIN
/// repo's ODB, returning `path → blob SHA`. Fail closed: clean worktrees return
/// an empty vec. Never follows symlinks (mirrors discard's #324 guard).
fn odb_backup_worktree(
    repo: &Repository,
    wt_path: &Path,
    backups: &mut Vec<DiscardBackup>,
) -> Result<(), GitError> {
    let wt_repo = Repository::open(wt_path)
        .map_err(|error| GitError::Other(format!("backup: open worktree: {error}")))?;
    let status = working_tree_status(&wt_repo)?;
    let mut rels: Vec<String> = Vec::new();
    let push_rel = |p: &Path, rels: &mut Vec<String>| {
        let rel = p.to_string_lossy().replace('\\', "/");
        if !rel.is_empty() && !rels.contains(&rel) {
            rels.push(rel);
        }
    };
    for fs in status.staged.iter().chain(status.unstaged.iter()) {
        push_rel(&fs.path, &mut rels);
    }
    for p in &status.untracked {
        push_rel(p, &mut rels);
    }
    let backup_id = super::backup::operation_id();
    for rel in rels {
        let abs = wt_path.join(&rel);
        let Some(content) = read_worktree_backup(&abs)? else {
            continue; // Already deleted paths have no working-tree bytes to capture.
        };
        backups.push(super::backup::write_blob(
            repo,
            &backup_id,
            backups.len(),
            rel,
            &content,
        )?);
    }
    Ok(())
}

fn read_worktree_backup(path: &Path) -> Result<Option<Vec<u8>>, GitError> {
    use std::io::ErrorKind;
    let failure = |error| GitError::Other(format!("backup: read {}: {error}", path.display()));
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(error)),
    };
    let content = if metadata.file_type().is_symlink() {
        std::fs::read_link(path).map(|target| target.to_string_lossy().into_owned().into_bytes())
    } else {
        std::fs::read(path)
    };
    match content {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error)
            if error.kind() == ErrorKind::NotFound
                && matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(failure(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("HOME", dir)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@e")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@e")
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    }

    /// issue #413: the ODB backup blob SHA is a REAL, recoverable object in the
    /// MAIN repo's ODB — i.e. the recovery handle the oplog records actually
    /// resolves to the raced-in content, not a dangling name.
    #[test]
    fn odb_backup_worktree_returns_recoverable_blob() {
        let base = tempfile::tempdir().unwrap();
        let main = base.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main", "."]);
        std::fs::write(main.join("README.md"), "x\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "init"]);
        let wt = base.path().join("wt");
        git(
            &main,
            &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        );
        // Dirty the worktree with a known-content untracked file (the race #413
        // guards: content present at execute that the plan did not see).
        std::fs::write(wt.join("scratch.txt"), "raced work\n").unwrap();

        let repo = Repository::open(&main).unwrap();
        let mut backups = Vec::new();
        odb_backup_worktree(&repo, &wt, &mut backups).expect("backup");
        assert_eq!(backups.len(), 1, "the untracked file must be backed up");
        assert_eq!(backups[0].path, "scratch.txt");
        let oid = git2::Oid::from_str(&backups[0].blob).unwrap();
        let blob = repo
            .find_blob(oid)
            .expect("backup blob SHA must resolve in the main ODB (issue #413)");
        assert_eq!(blob.content(), b"raced work\n");
        // A tracked deletion is absent, whereas a dirty unreadable file must
        // abort the backup phase. Neither case may silently lose existing bytes.
        std::fs::remove_file(wt.join("README.md")).unwrap();
        let mut deleted = Vec::new();
        odb_backup_worktree(&repo, &wt, &mut deleted).unwrap();
        assert_eq!(deleted.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = wt.join("scratch.txt");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
            let failed = odb_backup_worktree(&repo, &wt, &mut Vec::new());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(failed.is_err(), "unreadable dirty content must stop backup");
            assert_eq!(std::fs::read(&path).unwrap(), b"raced work\n");
            let link = wt.join("broken-link");
            std::os::unix::fs::symlink("absent-target", &link).unwrap();
            assert_eq!(
                read_worktree_backup(&link).unwrap().unwrap(),
                b"absent-target"
            );
        }
        // Failed repository open/status is not a successful empty backup.
        assert!(odb_backup_worktree(&repo, &base.path().join("absent"), &mut Vec::new()).is_err());
    }
    #[test]
    fn pre_remove_failure_keeps_the_worktree() {
        if std::env::var_os("KAGI_REMOVE_EXECUTOR_UNIT_CHILD").is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "ops::worktree_remove::tests::pre_remove_failure_keeps_the_worktree",
                    "--nocapture",
                ])
                .env("KAGI_REMOVE_EXECUTOR_UNIT_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        use crate::ops::{load_worktree_config, trust_worktree_config};
        fn write_config(root: &Path, body: &str) {
            std::fs::create_dir_all(root.join(".kagi")).unwrap();
            std::fs::write(root.join(".kagi/worktree.toml"), body).unwrap();
        }
        let store = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", store.path());
        for key in ["KAGI_OPEN_REPO", "KAGI_MENU_DUMP", "KAGI_SELECT_FIRST"] {
            std::env::remove_var(key);
        }

        let td = tempfile::tempdir().unwrap();
        let main = td.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "master"]);
        std::fs::write(main.join("a.txt"), "a\n").unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "base"]);

        let wt = td.path().join("wt");
        git(
            &main,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feat"],
        );
        // A pre_remove command that FAILS (`false` exits 1). Committed so the
        // worktree stays clean (an untracked file would block removal on its own).
        write_config(&wt, "[[pre_remove]]\ntype = \"command\"\nrun = \"false\"\n");
        git(&wt, &["add", "."]);
        git(&wt, &["commit", "-qm", "config"]);

        let backend = crate::Backend::open(&main).expect("open main");
        let plan = backend.plan_remove_worktree("wt", false).expect("plan");

        // Untrusted → removal aborts, worktree survives.
        let err = crate::ops::execute_remove_worktree(
            &git2::Repository::open(&main).unwrap(),
            &plan,
            "wt",
            false,
        )
        .expect_err("untrusted pre_remove command must abort the removal");
        assert!(format!("{err:?}").to_lowercase().contains("trust"));
        assert!(wt.exists(), "worktree must survive an aborted removal");

        // Trust it → the command now runs, fails (exit 1), still aborts.
        let cfg = load_worktree_config(&wt).unwrap().unwrap();
        trust_worktree_config(&cfg).unwrap();
        let err = crate::ops::execute_remove_worktree(
            &git2::Repository::open(&main).unwrap(),
            &plan,
            "wt",
            false,
        )
        .expect_err("a failing pre_remove command must abort the removal");
        assert!(format!("{err:?}").contains("exited with status"));
        assert!(
            wt.exists(),
            "worktree must survive when the pre_remove command fails"
        );

        std::env::remove_var("KAGI_LOG_DIR");
    }

    /// A trusted pre-remove command can register a new worktree after
    /// preflight. Recheck immediately before recursive deletion: the ignored
    /// directory was present at plan time, so its count has not grown.
    #[test]
    fn pre_remove_nested_worktree_survives_final_deletion_guard() {
        if std::env::var_os("KAGI_REMOVE_NESTED_UNIT_CHILD").is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "ops::worktree_remove::tests::pre_remove_nested_worktree_survives_final_deletion_guard",
                    "--nocapture",
                ])
                .env("KAGI_REMOVE_NESTED_UNIT_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        use crate::ops::{load_worktree_config, trust_worktree_config};
        let store = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", store.path());
        for key in ["KAGI_OPEN_REPO", "KAGI_MENU_DUMP", "KAGI_SELECT_FIRST"] {
            std::env::remove_var(key);
        }
        let td = tempfile::tempdir().unwrap();
        let main = td.path().join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        std::fs::write(main.join("README.md"), "unbacked bytes\n").unwrap();
        std::fs::write(main.join(".gitignore"), "nested/\n").unwrap();
        let outer = td.path().join("outer");
        let inner = outer.join("nested/inner");
        std::fs::create_dir(main.join(".kagi")).unwrap();
        let config = format!(
            "[[pre_remove]]\ntype = \"command\"\nrun = \"git -C {} worktree add -q -b inner {}\"\n\
             [[pre_remove]]\ntype = \"command\"\nrun = \"cp README.md nested/inner/untracked.txt\"\n",
            main.display(),
            inner.display()
        );
        std::fs::write(main.join(".kagi/worktree.toml"), config).unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-qm", "base"]);
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "outer",
                outer.to_str().unwrap(),
            ],
        );
        std::fs::create_dir(outer.join("nested")).unwrap();
        std::fs::write(outer.join("nested/seed"), "ignored seed\n").unwrap();
        let cfg = load_worktree_config(&outer).unwrap().unwrap();
        trust_worktree_config(&cfg).unwrap();
        let repo = Repository::open(&main).unwrap();
        let plan = plan_remove_worktree(&repo, "outer", false).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(
            ignored_content_counts(&repo.find_worktree("outer").unwrap()).unwrap(),
            (0, 1)
        );
        let error = execute_remove_worktree(&repo, &plan, "outer", false)
            .expect_err("a newly registered inner worktree must stop deletion");
        assert!(
            matches!(
                error,
                GitError::Blocked(note)
                    if matches!(
                        *note,
                        PlanNote::Worktree(WorktreeNote::RemoveContainsWorktree { .. })
                    )
            ),
            "expected nested registered worktree blocker"
        );
        assert_eq!(
            std::fs::read_to_string(inner.join("untracked.txt")).unwrap(),
            "unbacked bytes\n"
        );
        assert!(outer.exists() && repo.find_worktree("inner").is_ok());
        std::env::remove_var("KAGI_LOG_DIR");
    }
}
