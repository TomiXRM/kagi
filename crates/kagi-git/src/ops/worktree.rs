use super::*;
use ignore::gitignore::GitignoreBuilder;
use ignore::WalkBuilder;
use kagi_domain::plan_note::{
    CommonNote, DirtyParts, OpPhrase, UntrackedCtx, WorktreeNote, WorktreeRecovery, WorktreeTitle,
};
use kagi_domain::worktree_include::{
    select_worktree_include, WorktreeIncludeCandidate, WorktreeIncludeSelection,
    WORKTREE_INCLUDE_CAP_BYTES,
};

// ────────────────────────────────────────────────────────────
// .worktreeinclude — copy gitignored files into a new worktree (issue #339)
// ────────────────────────────────────────────────────────────

/// Scan the main worktree for files that match `.worktreeinclude`, annotating
/// each with the git facts the pure selector needs. Returns an empty vec when
/// there is no `.worktreeinclude` (preserving the previous behaviour).
fn scan_worktree_include(repo: &Repository, repo_root: &Path) -> Vec<WorktreeIncludeCandidate> {
    let content = match std::fs::read_to_string(repo_root.join(".worktreeinclude")) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut builder = GitignoreBuilder::new(repo_root);
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let _ = builder.add_line(None, line);
    }
    let matcher = match builder.build() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    // Walk everything (including gitignored files — those are exactly what we
    // want) but never descend into `.git`.
    let walker = WalkBuilder::new(repo_root)
        .standard_filters(false)
        .hidden(false)
        .parents(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();

    let mut out = Vec::new();
    for entry in walker.flatten() {
        let rel = match entry.path().strip_prefix(repo_root) {
            Ok(r) if !r.as_os_str().is_empty() => r,
            _ => continue,
        };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // `matched_path_or_any_parents` so a matched directory (e.g.
        // `node_modules/`) selects every file beneath it.
        if !matcher.matched_path_or_any_parents(rel, is_dir).is_ignore() {
            continue;
        }
        if is_dir {
            continue; // only files are copied
        }
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        let is_ignored = repo.is_path_ignored(rel).unwrap_or(false);
        let is_tracked = repo
            .index()
            .ok()
            .and_then(|idx| idx.get_path(rel, 0))
            .is_some();
        let size = if is_symlink {
            0
        } else {
            std::fs::metadata(entry.path())
                .map(|m| m.len())
                .unwrap_or(0)
        };
        out.push(WorktreeIncludeCandidate {
            path: rel.to_string_lossy().replace('\\', "/"),
            is_tracked,
            is_ignored,
            is_symlink,
            size,
        });
    }
    out
}

/// Compute the `.worktreeinclude` selection for the repo's main worktree.
fn worktree_include_selection(repo: &Repository, repo_root: &Path) -> WorktreeIncludeSelection {
    let candidates = scan_worktree_include(repo, repo_root);
    select_worktree_include(&candidates, WORKTREE_INCLUDE_CAP_BYTES)
}

/// Turn a selection into the plan warnings that preview the copy (issue #339).
fn worktree_include_warnings(sel: &WorktreeIncludeSelection) -> Vec<PlanNote> {
    const SAMPLE: usize = 5;
    let mut notes = Vec::new();
    if !sel.copy.is_empty() {
        let sample: Vec<String> = sel.copy.iter().take(SAMPLE).cloned().collect();
        notes.push(PlanNote::Worktree(WorktreeNote::IncludeCopy {
            count: sel.copy.len(),
            total_bytes: sel.total_bytes,
            sample,
            more: sel.copy.len().saturating_sub(SAMPLE),
        }));
    }
    if !sel.skipped_symlinks.is_empty() {
        notes.push(PlanNote::Worktree(WorktreeNote::IncludeSkippedSymlinks {
            count: sel.skipped_symlinks.len(),
        }));
    }
    if sel.over_cap {
        notes.push(PlanNote::Worktree(WorktreeNote::IncludeOverCap {
            total_bytes: sel.total_bytes,
            cap_bytes: sel.cap_bytes,
        }));
    }
    notes
}

/// Copy the selected `.worktreeinclude` files into a freshly created worktree.
/// Best-effort: never overwrites an existing destination, never fails the
/// worktree creation (the worktree already exists at this point).
fn copy_worktree_include(sel: &WorktreeIncludeSelection, repo_root: &Path, target: &Path) {
    for rel in &sel.copy {
        let dst = target.join(rel);
        if dst.exists() {
            continue; // no overwrite (issue #339 §5)
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(repo_root.join(rel), &dst);
    }
}

// ────────────────────────────────────────────────────────────
// create-worktree helpers
// ────────────────────────────────────────────────────────────

/// Lexically normalize a path without requiring the final path to exist.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(component.as_os_str()),
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// Canonicalize the longest existing prefix of `path` (resolving symlinks) and
/// re-append the components that don't exist yet. Lets worktree containment be
/// checked even when the target's parent directory hasn't been created.
fn canonicalize_nearest_existing(path: &Path) -> std::io::Result<PathBuf> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path;
    loop {
        match std::fs::canonicalize(cur) {
            Ok(mut real) => {
                for part in tail.iter().rev() {
                    real.push(part);
                }
                return Ok(real);
            }
            Err(e) => match (cur.file_name(), cur.parent()) {
                (Some(name), Some(parent)) => {
                    tail.push(name.to_os_string());
                    cur = parent;
                }
                _ => return Err(e),
            },
        }
    }
}

/// Validate and normalize a worktree path entered by the user.
///
/// Relative paths are interpreted relative to `repo_root`.  The target path
/// itself must not already exist, but its parent must exist so validation works
/// for the normal `../repo-worktrees/new-branch` case.
///
/// Returns the English-only error string (back-compat shim over
/// [`validate_worktree_path_keyed`]).
pub fn validate_worktree_path(
    repo_root: &Path,
    input: impl AsRef<Path>,
) -> Result<PathBuf, String> {
    validate_worktree_path_keyed(repo_root, input).map_err(|e| e.to_string())
}

/// Like [`validate_worktree_path`] but returns a [`WorktreeValidationError`] so
/// the UI can localize the two keyed reasons (empty / already exists).
pub fn validate_worktree_path_keyed(
    repo_root: &Path,
    input: impl AsRef<Path>,
) -> Result<PathBuf, WorktreeValidationError> {
    use WorktreeValidationError::{Keyed, Other};
    let input = input.as_ref();
    if input.as_os_str().is_empty() {
        return Err(Keyed(WorktreePathError::Empty));
    }

    let repo_root = std::fs::canonicalize(repo_root)
        .map_err(|e| Other(format!("Repository root is not accessible: {}", e)))?;
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        repo_root.join(input)
    };
    let candidate = normalize_path(&candidate);

    if candidate.exists() {
        return Err(Keyed(WorktreePathError::Exists(
            candidate.display().to_string(),
        )));
    }

    let parent = candidate
        .parent()
        .ok_or_else(|| Other("Worktree path must have a parent directory.".to_string()))?;
    let filename = candidate
        .file_name()
        .ok_or_else(|| Other("Worktree path must name a directory.".to_string()))?;

    // The immediate parent need not exist yet — the default worktree path is
    // `../<repo>-worktrees/<branch>` and `execute_create_worktree` creates the
    // parent before adding the worktree. Resolve symlinks on the longest
    // existing prefix so the containment check below is still symlink-safe.
    let parent = canonicalize_nearest_existing(parent)
        .map_err(|e| Other(format!("Parent directory is not accessible: {}", e)))?;
    let candidate_real_parent = normalize_path(&parent.join(filename));

    if candidate_real_parent == repo_root || candidate_real_parent.starts_with(&repo_root) {
        return Err(Other(format!(
            "Worktree path '{}' must be outside the repository.",
            candidate_real_parent.display()
        )));
    }

    Ok(candidate_real_parent)
}

fn worktree_name_from_path(path: &Path, branch: &str) -> String {
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(branch);
    base.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

/// Build a create-branch plan whose predicted HEAD reflects the optional
/// checkout-after-create UI checkbox.
pub fn plan_create_branch_with_checkout(
    repo: &Repository,
    name: &str,
    at: &CommitId,
    checkout_after: bool,
) -> Result<OperationPlan, GitError> {
    let mut plan = plan_create_branch(repo, name, at)?;
    if !checkout_after {
        return Ok(plan);
    }

    let status = working_tree_status(repo)?;
    if !status.conflicted.is_empty() {
        plan.blockers
            .push(PlanNote::Common(CommonNote::ConflictedFiles {
                count: status.conflicted.len(),
                before: OpPhrase::CheckingOutTheNewBranch,
            }));
    }
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let parts = DirtyParts {
            staged: status.staged.len(),
            modified: status.unstaged.len(),
        };
        plan.blockers.push(PlanNote::Worktree(
            WorktreeNote::DirtyBlocksCheckoutAfterCreate { parts },
        ));
    }
    if !status.untracked.is_empty() {
        plan.warnings
            .push(PlanNote::Common(CommonNote::UntrackedRemain {
                count: status.untracked.len(),
                ctx: UntrackedCtx::AfterSwitchingBranches,
            }));
    }

    let prev = plan
        .current
        .head
        .strip_prefix("branch: ")
        .unwrap_or("<previous-branch>")
        .to_string();
    plan.title = PlanTitle::Worktree(WorktreeTitle::CreateBranchCheckout {
        name: name.to_string(),
        at: at.short().to_string(),
    });
    plan.predicted.head = format!("branch: {}", name);
    plan.recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::CreateBranchCheckout {
            name: name.to_string(),
            prev: prev.clone(),
        }),
        commands: vec![
            format!("git branch -d {}", name),
            format!("git checkout {}", prev),
        ],
    });
    Ok(plan)
}

// ────────────────────────────────────────────────────────────
// plan_create_worktree
// ────────────────────────────────────────────────────────────

/// Analyse whether creating a linked worktree with a new branch is safe.
pub fn plan_create_worktree(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
    start: &CommitId,
) -> Result<OperationPlan, GitError> {
    plan_create_worktree_impl(repo, branch, path, start, false)
}

/// Analyse whether creating a linked worktree for an existing local branch is safe.
pub fn plan_open_worktree_for_branch(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
) -> Result<OperationPlan, GitError> {
    let branch_commit = resolve_branch_commit(repo, branch)?;
    plan_create_worktree_impl(
        repo,
        branch,
        path,
        &CommitId(branch_commit.id().to_string()),
        true,
    )
}

fn plan_create_worktree_impl(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
    start: &CommitId,
    allow_existing_branch: bool,
) -> Result<OperationPlan, GitError> {
    let repo_root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?;
    let mut plan = if allow_existing_branch {
        let head = resolve_head(repo)?;
        let status = working_tree_status(repo)?;
        let mut blockers = Vec::new();
        if repo.find_branch(branch, BranchType::Local).is_err() {
            blockers.push(PlanNote::Common(CommonNote::BranchMissing {
                name: branch.to_string(),
                in_repo: true,
            }));
        }
        if let Some(path) = branch_checked_out_worktree_path(repo, branch)? {
            blockers.push(PlanNote::Worktree(WorktreeNote::BranchInOtherWorktree {
                branch: branch.to_string(),
                path: path.display().to_string(),
            }));
        }
        OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            // `title`/`recovery` are always overwritten below (both branches
            // of this `if`/`else` converge on the same final assignment) —
            // these placeholders are never observed. (ADR-0129 appendix
            // §G-5: the legacy `"Open worktree for '{}'"` title here was dead
            // code for the same reason; removed rather than kept unreachable.)
            title: PlanTitle::Worktree(WorktreeTitle::CreateWorktree {
                branch: branch.to_string(),
                start: start.short().to_string(),
            }),
            current: StateSummary {
                head: head.display(),
                dirty: status_summary_display(&status),
            },
            predicted: StateSummary {
                head: head.display(),
                dirty: status_summary_display(&status),
            },
            warnings: Vec::new(),
            blockers,
            recovery: None,
            head_at_plan: head,
            stash_count_at_plan: 0,
            worktree_digest: None,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        }
    } else {
        plan_create_branch(repo, branch, start)?
    };
    let target_path = match validate_worktree_path_keyed(repo_root, path.as_ref()) {
        Ok(path) => path,
        Err(err) => {
            let note = match err {
                WorktreeValidationError::Keyed(e) => CommonNote::WorktreePathErrorKeyed(e),
                // Not one of the two keyed reasons (empty / already exists);
                // English-only passthrough (ADR-0129 appendix §E).
                WorktreeValidationError::Other(message) => {
                    CommonNote::GitErrorPassthrough { message }
                }
            };
            plan.blockers.push(PlanNote::Common(note));
            if path.as_ref().is_absolute() {
                normalize_path(path.as_ref())
            } else {
                normalize_path(&repo_root.join(path.as_ref()))
            }
        }
    };
    plan.title = PlanTitle::Worktree(WorktreeTitle::CreateWorktree {
        branch: branch.to_string(),
        start: start.short().to_string(),
    });
    plan.predicted = StateSummary {
        head: plan.current.head.clone(),
        dirty: plan.current.dirty.clone(),
    };
    plan.recovery = Some(PlanRecovery {
        kind: RecoveryKind::Worktree(WorktreeRecovery::CreateWorktree {
            path: target_path.display().to_string(),
            branch: branch.to_string(),
        }),
        commands: vec![
            format!("git worktree remove {}", target_path.display()),
            format!("git branch -d {}", branch),
        ],
    });
    plan.warnings
        .push(PlanNote::Worktree(WorktreeNote::CreatesLinkedWorktree {
            path: target_path.display().to_string(),
            branch: branch.to_string(),
            start: start.short().to_string(),
        }));

    // issue #339: preview the .worktreeinclude copy set (no-op if absent).
    let sel = worktree_include_selection(repo, repo_root);
    plan.warnings.extend(worktree_include_warnings(&sel));

    // issue #341: enumerate the typed post_create steps (no-op if no config).
    // A command step in an untrusted config marks the note trust-required, so
    // confirming this plan doubles as the trust prompt.
    if let Ok(Some(cfg)) = load_worktree_config(repo_root) {
        if let Some(note) = post_create_note(&cfg) {
            plan.warnings.push(note);
        }
    }

    Ok(plan)
}

// ────────────────────────────────────────────────────────────
// execute_create_worktree
// ────────────────────────────────────────────────────────────

/// Create a new branch at `start` and attach it to a new linked worktree.
pub fn execute_create_worktree(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
    start: &CommitId,
) -> Result<(), GitError> {
    execute_create_worktree_impl(repo, branch, path, start, false)
}

/// Attach an existing local branch to a new linked worktree.
pub fn execute_open_worktree_for_branch(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
) -> Result<(), GitError> {
    let branch_commit = resolve_branch_commit(repo, branch)?;
    execute_create_worktree_impl(
        repo,
        branch,
        path,
        &CommitId(branch_commit.id().to_string()),
        true,
    )
}

fn execute_create_worktree_impl(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
    start: &CommitId,
    allow_existing_branch: bool,
) -> Result<(), GitError> {
    let repo_root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?;
    let target_path = validate_worktree_path(repo_root, path.as_ref()).map_err(GitError::Other)?;

    if allow_existing_branch {
        if let Some(path) = branch_checked_out_worktree_path(repo, branch)? {
            return Err(GitError::Other(format!(
                "Branch '{}' is already checked out in another worktree: {}",
                branch,
                path.display()
            )));
        }
    } else {
        execute_create_branch(repo, branch, start)?;
    }

    let refname = format!("refs/heads/{}", branch);
    let branch_ref = repo
        .find_reference(&refname)
        .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&branch_ref));

    // The default path's parent (`../<repo>-worktrees/`) may not exist yet;
    // libgit2 will not create it, so create it here (containment already
    // verified by validate_worktree_path above).
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!("could not create worktree parent directory: {}", e))
        })?;
    }

    let worktree_name = worktree_name_from_path(&target_path, branch);
    repo.worktree(&worktree_name, &target_path, Some(&opts))
        .map_err(|e| GitError::Other(format!("worktree creation failed: {}", e.message())))?;

    // issue #339: copy .worktreeinclude files into the fresh worktree. Computed
    // fresh (not from the plan) since execute has no plan handle here; best-
    // effort so a copy hiccup never undoes a created worktree.
    let sel = worktree_include_selection(repo, repo_root);
    copy_worktree_include(&sel, repo_root, &target_path);

    // issue #341: run the typed post_create steps. Best-effort — the worktree
    // already exists, so a step failure never undoes it. copy/symlink always
    // run; command runs only when the config is trusted (and never headless).
    if let Ok(Some(cfg)) = load_worktree_config(repo_root) {
        let trusted = is_worktree_config_trusted(&cfg);
        let env = StepEnv {
            main_root: repo_root.to_path_buf(),
            worktree: target_path.clone(),
        };
        let _report = run_post_create(&cfg.steps.post_create, &env, trusted);
    }

    Ok(())
}

/// Return the path of a registered worktree that currently has `branch`
/// checked out, if any.
pub fn branch_checked_out_worktree_path(
    repo: &Repository,
    branch: &str,
) -> Result<Option<PathBuf>, GitError> {
    let current_path = repo.workdir().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut paths = Vec::new();
    if repo.is_worktree() {
        if let Some(main_path) = repo.commondir().parent().map(|p| p.to_path_buf()) {
            paths.push(main_path);
        }
    } else {
        paths.push(current_path.clone());
    }
    let names = repo
        .worktrees()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    for name in names.iter() {
        let Ok(Some(name)) = name else {
            continue;
        };
        if let Ok(wt) = repo.find_worktree(name) {
            paths.push(wt.path().to_path_buf());
        }
    }

    for path in paths {
        let Ok(wt_repo) = Repository::open(&path) else {
            continue;
        };
        let checked = wt_repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().ok().map(str::to_string));
        if checked.as_deref() == Some(branch) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

// ────────────────────────────────────────────────────────────
// plan_unlock_worktree / execute_unlock_worktree
// ────────────────────────────────────────────────────────────

/// Analyse whether unlocking the linked worktree `name` is safe.
///
/// Unlock is ref/admin-only: it never touches any working tree, so the plan is
/// never destructive. A lock is deliberate protection, so the plan surfaces the
/// recorded reason as a warning for the user to weigh before confirming.
pub fn plan_unlock_worktree(repo: &Repository, name: &str) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    match repo.find_worktree(name) {
        Ok(wt) => match wt.is_locked() {
            Ok(git2::WorktreeLockStatus::Locked(reason)) => {
                let reason = reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_string);
                warnings.push(PlanNote::Worktree(WorktreeNote::LockedWithReason {
                    reason,
                }));
            }
            Ok(git2::WorktreeLockStatus::Unlocked) => {
                blockers.push(PlanNote::Worktree(WorktreeNote::AlreadyUnlocked {
                    name: name.to_string(),
                }));
            }
            Err(e) => {
                blockers.push(PlanNote::Worktree(WorktreeNote::LockStateUnreadable {
                    name: name.to_string(),
                    err: e.message().to_string(),
                }));
            }
        },
        Err(_) => {
            blockers.push(PlanNote::Worktree(WorktreeNote::WorktreeMissing {
                name: name.to_string(),
            }));
        }
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Worktree(WorktreeTitle::UnlockWorktree {
            name: name.to_string(),
        }),
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
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Worktree(WorktreeRecovery::Unlock {
                name: name.to_string(),
            }),
            commands: vec![format!(
                "git worktree lock --reason \"<why>\" <path-of-{}>",
                name
            )],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Unlock the linked worktree `name`: preflight (HEAD unchanged) → unlock →
/// verify the lock is gone.
pub fn execute_unlock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;

    let wt = repo
        .find_worktree(name)
        .map_err(|e| GitError::Other(format!("worktree '{}' not found: {}", name, e.message())))?;
    match wt.is_locked() {
        Ok(git2::WorktreeLockStatus::Locked(_)) => {}
        Ok(git2::WorktreeLockStatus::Unlocked) => {
            return Err(GitError::Other(format!(
                "worktree '{}' is already unlocked",
                name
            )));
        }
        Err(e) => {
            return Err(GitError::Other(format!(
                "could not read lock state of worktree '{}': {}",
                name,
                e.message()
            )));
        }
    }
    wt.unlock()
        .map_err(|e| GitError::Other(format!("worktree unlock failed: {}", e.message())))?;

    // Verify: the lock must be gone.
    match wt.is_locked() {
        Ok(git2::WorktreeLockStatus::Unlocked) => Ok(()),
        _ => Err(GitError::Other(format!(
            "worktree '{}' still reports locked after unlock — unexpected state",
            name
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `copy_worktree_include` must never overwrite an existing destination
    /// file, and must create a fresh one (issue #339 §5 no-overwrite).
    #[test]
    fn copy_worktree_include_never_overwrites() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("keep"), "SOURCE").unwrap();
        std::fs::write(src.path().join("new"), "SOURCE").unwrap();
        std::fs::write(dst.path().join("keep"), "EXISTING").unwrap();

        let sel = WorktreeIncludeSelection {
            copy: vec!["keep".into(), "new".into()],
            ..Default::default()
        };
        copy_worktree_include(&sel, src.path(), dst.path());

        assert_eq!(
            std::fs::read_to_string(dst.path().join("keep")).unwrap(),
            "EXISTING",
            "existing dest must not be overwritten"
        );
        assert_eq!(
            std::fs::read_to_string(dst.path().join("new")).unwrap(),
            "SOURCE",
            "absent dest must be copied"
        );
    }
}
