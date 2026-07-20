use super::*;

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
    if !parent.exists() {
        return Err(Other(format!(
            "Parent directory '{}' does not exist.",
            parent.display()
        )));
    }

    let parent = std::fs::canonicalize(parent)
        .map_err(|e| Other(format!("Parent directory is not accessible: {}", e)))?;
    let filename = candidate
        .file_name()
        .ok_or_else(|| Other("Worktree path must name a directory.".to_string()))?;
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
        plan.blockers.push(PlanNote::verbatim(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before checking out the new branch.",
            status.conflicted.len()
        )));
    }
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        plan.blockers.push(PlanNote::verbatim(format!(
            "Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing.",
            parts.join(", ")
        )));
    }
    if !status.untracked.is_empty() {
        plan.warnings.push(PlanNote::verbatim(format!(
            "{} untracked file(s) will remain after switching branches.",
            status.untracked.len()
        )));
    }

    plan.title = PlanTitle::verbatim(format!(
        "Create branch '{}' @ {} and checkout",
        name,
        at.short()
    ));
    plan.predicted.head = format!("branch: {}", name);
    plan.recovery = Some(PlanRecovery::verbatim(format!(
        "This creates branch '{}' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d {}\nTo return after checkout:\n  git checkout {}",
        name,
        name,
        plan.current.head.strip_prefix("branch: ").unwrap_or("<previous-branch>")
    )));
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
            blockers.push(format!(
                "Branch '{}' does not exist in this repository.",
                branch
            ));
        }
        if let Some(path) = branch_checked_out_worktree_path(repo, branch)? {
            blockers.push(format!(
                "Branch '{}' is already checked out in another worktree: {}",
                branch,
                path.display()
            ));
        }
        OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            title: PlanTitle::verbatim(format!("Open worktree for '{}'", branch)),
            current: StateSummary {
                head: head.display(),
                dirty: status_summary_display(&status),
            },
            predicted: StateSummary {
                head: head.display(),
                dirty: status_summary_display(&status),
            },
            warnings: Vec::new(),
            blockers: PlanNote::wrap_all(blockers),
            recovery: Some(PlanRecovery::verbatim(String::new())),
            head_at_plan: head,
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        }
    } else {
        plan_create_branch(repo, branch, start)?
    };
    let target_path = match validate_worktree_path(repo_root, path.as_ref()) {
        Ok(path) => path,
        Err(msg) => {
            plan.blockers.push(PlanNote::verbatim(msg));
            if path.as_ref().is_absolute() {
                normalize_path(path.as_ref())
            } else {
                normalize_path(&repo_root.join(path.as_ref()))
            }
        }
    };
    plan.title = PlanTitle::verbatim(format!("Create worktree '{}' @ {}", branch, start.short()));
    plan.predicted = StateSummary {
        head: plan.current.head.clone(),
        dirty: plan.current.dirty.clone(),
    };
    plan.recovery = Some(PlanRecovery::verbatim(format!(
        "Remove the linked worktree if needed:\n  git worktree remove {}\nThe branch can then be removed with:\n  git branch -d {}",
        target_path.display(),
        branch
    )));
    plan.warnings.push(PlanNote::verbatim(format!(
        "Creates a linked worktree at '{}' with branch '{}' (start point {}).",
        target_path.display(),
        branch,
        start.short()
    )));

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

    let worktree_name = worktree_name_from_path(&target_path, branch);
    repo.worktree(&worktree_name, &target_path, Some(&opts))
        .map_err(|e| GitError::Other(format!("worktree creation failed: {}", e.message())))?;

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
                let reason_display = reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(|r| format!("\"{}\"", r))
                    .unwrap_or_else(|| "(no reason recorded)".to_string());
                warnings.push(format!(
                    "Locked with reason: {} — a lock is deliberate protection someone \
                     placed on this worktree. Make sure it is no longer needed.",
                    reason_display
                ));
            }
            Ok(git2::WorktreeLockStatus::Unlocked) => {
                blockers.push(format!("Worktree '{}' is already unlocked.", name));
            }
            Err(e) => {
                blockers.push(format!(
                    "Could not read the lock state of worktree '{}': {}",
                    name,
                    e.message()
                ));
            }
        },
        Err(_) => {
            blockers.push(format!("Worktree '{}' does not exist.", name));
        }
    }

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::verbatim(format!("Unlock worktree '{}'", name)),
        current: StateSummary {
            head: head.display(),
            dirty: dirty.clone(),
        },
        predicted: StateSummary {
            head: head.display(),
            dirty,
        },
        warnings: PlanNote::wrap_all(warnings),
        blockers: PlanNote::wrap_all(blockers),
        recovery: Some(PlanRecovery::verbatim(format!(
            "Re-lock the worktree if needed:\n  git worktree lock --reason \"<why>\" <path-of-{}>",
            name
        ))),
        head_at_plan: head,
        stash_count_at_plan: 0,
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
