use super::*;
use kagi_domain::plan_note::{BranchNote, BranchRecovery, BranchTitle, CommonNote};

// ────────────────────────────────────────────────────────────
// plan_create_branch
// ────────────────────────────────────────────────────────────

/// Compute the keyed branch-name validation errors for the **create-branch**
/// path (W29-I18N-WAVE2), in the same order the legacy code pushed them.
///
/// This is the single source of truth for the create-branch name reasons: the
/// plan builder maps each error through [`BranchNameError::Display`] into the
/// English-only `blockers` Vec (preserving the pinned wording), and the UI maps
/// the same errors to localized messages. The commit-existence blocker is *not*
/// keyed here (it stays English-only in the plan).
pub fn create_branch_name_errors(repo: &Repository, name: &str) -> Vec<BranchNameError> {
    let mut errs: Vec<BranchNameError> = Vec::new();

    if name.is_empty() {
        errs.push(BranchNameError::EmptyCreate);
    }

    // Invalid name (use git2 ref validation on the full ref path).
    if !name.is_empty() && !git2::Reference::is_valid_name(&format!("refs/heads/{}", name)) {
        errs.push(BranchNameError::CreateInvalidRef(name.to_string()));
    }

    // Leading `-` is rejected explicitly: although git2 considers it a valid ref
    // name, it is ambiguous on the command line (may be interpreted as a flag).
    if !name.is_empty() && name.starts_with('-') {
        errs.push(BranchNameError::CreateLeadingDash(name.to_string()));
    }

    // Already-exists check.
    if !name.is_empty() && repo.find_branch(name, BranchType::Local).is_ok() {
        errs.push(BranchNameError::CreateExists(name.to_string()));
    }

    errs
}

/// Analyse whether creating a new local branch at `at` is safe and return an
/// [`OperationPlan`].
///
/// This is a **Safe-class** operation (ADR-0004): it does not modify HEAD or the
/// working tree.  No warnings are produced; only blockers.
///
/// # Blocker conditions
///
/// - `name` is empty.
/// - `name` fails `git2::Reference::is_valid_name("refs/heads/<name>")` — e.g.
///   names containing `..`, a leading `-`, spaces, or other invalid characters.
/// - A local branch with `name` already exists.
/// - The commit `at` does not exist in the repository.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_create_branch(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD ──────────────────────────────────────
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

    // ── 3. Check blockers ────────────────────────────────────
    // The branch-name reasons are computed as keyed errors (W29-I18N-WAVE2) so
    // the UI can localize them (ADR-0129 appendix §E) as
    // `CommonNote::BranchNameErrorKeyed`, not a `BranchNote`.
    let mut blockers: Vec<PlanNote> = create_branch_name_errors(repo, name)
        .into_iter()
        .map(|e| PlanNote::Common(CommonNote::BranchNameErrorKeyed(e)))
        .collect();

    // Commit existence check.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())));
    let commit_exists = match oid {
        Ok(oid) => repo.find_commit(oid).is_ok(),
        Err(_) => false,
    };
    if !commit_exists {
        blockers.push(PlanNote::Branch(BranchNote::CommitMissing {
            sha: at.short().to_string(),
        }));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the new branch appears as an additional ref.
    let short_sha = at.short().to_string();
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let recovery = PlanRecovery {
        kind: RecoveryKind::Branch(BranchRecovery::CreateBranch {
            name: name.to_string(),
        }),
        commands: vec![format!("git branch -d {}", name)],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::CreateBranch {
            name: name.to_string(),
            at: short_sha,
            checkout: false,
        }),
        current,
        predicted,
        warnings: Vec::new(),
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
// execute_create_branch
// ────────────────────────────────────────────────────────────

/// Create a new local branch named `name` pointing at commit `at`.
///
/// Uses `repo.branch(name, &commit, false)` — the `force` argument is **always
/// `false`** (a literal constant) to prevent overwriting an existing branch.
///
/// **This function does not perform a checkout.**  HEAD remains unchanged.
///
/// # Errors
///
/// Returns [`GitError::Other`] if:
/// - `at` is not a valid or existing commit OID.
/// - A branch named `name` already exists (`force=false` is enforced by libgit2).
/// - Any other libgit2 failure.
pub fn execute_create_branch(repo: &Repository, name: &str, at: &CommitId) -> Result<(), GitError> {
    // Resolve the target commit.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())))?;
    let commit = repo.find_commit(oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            at.short(),
            e.message()
        ))
    })?;

    // Create the branch.  force=false is a literal constant — never change this.
    repo.branch(name, &commit, false)
        .map_err(|e| GitError::Other(format!("branch creation failed: {}", e.message())))?;

    Ok(())
}

fn local_branch_names(repo: &Repository) -> Result<Vec<String>, GitError> {
    let mut names = Vec::new();
    let branches = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
    for branch_result in branches {
        let (branch, _) = branch_result
            .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
        if let Ok(Some(name)) = branch.name() {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

fn branch_config_entries(repo: &Repository, branch_name: &str) -> Vec<(String, String)> {
    let prefix = format!("branch.{}.", branch_name);
    let mut entries_out = Vec::new();
    if let Ok(mut config) = repo.config() {
        if let Ok(snap) = config.snapshot() {
            if let Ok(mut entries) = snap.entries(None) {
                while let Some(Ok(entry)) = entries.next() {
                    let key = match entry.name() {
                        Ok(k) if k.starts_with(&prefix) => k.to_string(),
                        _ => continue,
                    };
                    let value = match entry.value() {
                        Ok(v) => v.to_string(),
                        Err(_) => continue,
                    };
                    let suffix: String = key.chars().skip(prefix.chars().count()).collect();
                    if entries_out.iter().any(|(existing, _)| existing == &suffix) {
                        continue;
                    }
                    entries_out.push((suffix, value));
                }
            }
        }
    }
    entries_out
}

/// Tolerantly wipe the `branch.<name>.*` config section before a ref delete.
///
/// gh CLI is known to write duplicated `branch.<name>.*` keys (e.g.
/// github-pr-owner-number, one copy per `gh pr` invocation). libgit2's
/// `Branch::delete()` wipes the section key-by-key and aborts on the
/// duplicates ("could not find key … to delete") BEFORE deleting the ref —
/// so the first attempt fails and a retry succeeds. Removing the entries
/// tolerantly first means the ref deletion cannot be blocked by config
/// garbage. Best-effort: a key that is already gone (or lives in a read-only
/// level) is not an error.
pub(crate) fn pre_clean_branch_config(repo: &Repository, name: &str) {
    let prefix = format!("branch.{}.", name);
    if let Ok(mut config) = repo.config() {
        let mut keys: Vec<String> = Vec::new();
        if let Ok(snap) = config.snapshot() {
            if let Ok(mut entries) = snap.entries(None) {
                while let Some(Ok(entry)) = entries.next() {
                    if let Ok(k) = entry.name() {
                        if k.starts_with(&prefix) && !keys.iter().any(|e| e == k) {
                            keys.push(k.to_string());
                        }
                    }
                }
            }
        }
        for key in keys {
            if config.remove_multivar(&key, ".*").is_err() {
                let _ = config.remove(&key);
            }
        }
    }
}

fn remove_branch_config_section(repo: &Repository, branch_name: &str) {
    let prefix = format!("branch.{}.", branch_name);
    if let Ok(mut config) = repo.config() {
        let mut keys = Vec::new();
        if let Ok(snap) = config.snapshot() {
            if let Ok(mut entries) = snap.entries(None) {
                while let Some(Ok(entry)) = entries.next() {
                    if let Ok(k) = entry.name() {
                        if k.starts_with(&prefix) && !keys.iter().any(|e| e == k) {
                            keys.push(k.to_string());
                        }
                    }
                }
            }
        }
        for key in keys {
            if config.remove_multivar(&key, ".*").is_err() {
                let _ = config.remove(&key);
            }
        }
    }
}

pub fn plan_rename_branch(
    repo: &Repository,
    old_name: &str,
    new_name: &str,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    if repo.find_branch(old_name, BranchType::Local).is_err() {
        blockers.push(PlanNote::Common(CommonNote::BranchMissing {
            name: old_name.to_string(),
            in_repo: false,
        }));
    }
    let existing = local_branch_names(repo)?;
    if let BranchRenameValidation::Invalid(reason) =
        validate_branch_rename(old_name, new_name, &existing)
    {
        // ADR-0129 appendix §E: also a keyed `BranchNameError`, not a
        // `BranchNote`.
        blockers.push(PlanNote::Common(CommonNote::BranchNameErrorKeyed(reason)));
    }
    if status.is_dirty() {
        warnings.push(PlanNote::Branch(BranchNote::RenameRefOnlyDirty));
    }
    warnings.push(PlanNote::Branch(BranchNote::RenameRemoteNotRenamed));

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::RenameBranch {
            old: old_name.to_string(),
            new: new_name.to_string(),
        }),
        current,
        predicted: StateSummary {
            head: match &head {
                Head::Attached { branch, .. } if branch == old_name => {
                    format!("branch: {}", new_name)
                }
                _ => head.display(),
            },
            dirty: "working tree unchanged".to_string(),
        },
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Branch(BranchRecovery::RenameBranch {
                old: old_name.to_string(),
                new: new_name.to_string(),
            }),
            commands: vec![format!("git branch -m {} {}", new_name, old_name)],
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

pub fn execute_rename_branch(
    repo: &Repository,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<(), GitError> {
    preflight_check(repo, plan)?;
    let existing = local_branch_names(repo)?;
    if let BranchRenameValidation::Invalid(reason) =
        validate_branch_rename(old_name, new_name, &existing)
    {
        return Err(GitError::Other(reason.to_string()));
    }

    let saved_config = branch_config_entries(repo, old_name);
    let mut branch = repo.find_branch(old_name, BranchType::Local).map_err(|e| {
        GitError::Other(format!("branch '{}' not found: {}", old_name, e.message()))
    })?;
    branch
        .rename(new_name, false)
        .map_err(|e| GitError::Other(format!("branch rename failed: {}", e.message())))?;

    remove_branch_config_section(repo, old_name);
    remove_branch_config_section(repo, new_name);
    if let Ok(mut config) = repo.config() {
        for (suffix, value) in saved_config {
            let key = format!("branch.{}.{}", new_name, suffix);
            config.set_str(&key, &value).map_err(|e| {
                GitError::Other(format!("config carry-over failed: {}", e.message()))
            })?;
        }
    }

    if repo.find_branch(new_name, BranchType::Local).is_err() {
        return Err(GitError::Other(format!(
            "branch '{}' was not found after rename",
            new_name
        )));
    }
    if repo.find_branch(old_name, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "branch '{}' still exists after rename",
            old_name
        )));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// UndoOutcome  (T-HT-009)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// plan_undo_commit  (T-HT-009)
// ────────────────────────────────────────────────────────────

/// A worktree (other than the main one) that has `branch` checked out.
pub struct WorktreeCheckout {
    /// Worktree admin name (`git worktree list` identifier, used for prune).
    pub name: String,
    /// Working-directory path shown to the user.
    pub path: std::path::PathBuf,
    /// Uncommitted changes present (staged/unstaged/untracked/conflicted).
    pub dirty: bool,
    /// Worktree is locked (`git worktree lock`) — never auto-removed.
    pub locked: bool,
}

/// Find the linked worktree that has local branch `name` checked out, if any.
///
/// Git refuses to delete a branch while any worktree has it checked out; the
/// raw libgit2 error is user-hostile (user report: agent-created worktrees
/// linger and pin their branch). Detect it at PLAN time instead.
pub fn worktree_checkout_of(repo: &Repository, name: &str) -> Option<WorktreeCheckout> {
    let full_ref = format!("refs/heads/{name}");
    let wt_names = repo.worktrees().ok()?;
    for i in 0..wt_names.len() {
        let Ok(Some(wt_name)) = wt_names.get(i) else {
            continue;
        };
        let Ok(wt) = repo.find_worktree(wt_name) else {
            continue;
        };
        let Ok(wt_repo) = Repository::open_from_worktree(&wt) else {
            continue;
        };
        let head_matches = wt_repo
            .head()
            .ok()
            .and_then(|h| h.name().ok().map(|n| n == full_ref))
            .unwrap_or(false);
        if !head_matches {
            continue;
        }
        let dirty = working_tree_status(&wt_repo)
            .map(|st| st.is_dirty())
            .unwrap_or(true); // unreadable status: err on the safe side
        let locked = matches!(wt.is_locked(), Ok(git2::WorktreeLockStatus::Locked(_)));
        return Some(WorktreeCheckout {
            name: wt_name.to_string(),
            path: wt.path().to_path_buf(),
            dirty,
            locked,
        });
    }
    None
}

/// Analyse whether deleting local branch `name` is safe and return an
/// [`OperationPlan`].
///
/// # Design (ADR-0014)
///
/// Delete-branch is a **ref-only** operation: `Branch::delete()` removes the
/// local ref and does NOT touch the working tree or index.  **Force delete is
/// intentionally absent.**
///
/// The merged-or-not check uses `repo.graph_descendant_of(head_oid, tip_oid)`:
/// this returns `true` when `head_oid` is a descendant of `tip_oid`, meaning
/// `tip_oid` is reachable from HEAD (i.e. already merged into HEAD).
///
/// # Blocker conditions
///
/// - The named branch does not exist.
/// - The named branch is the currently checked-out branch (HEAD is attached to it).
/// - HEAD is detached and the branch tip is HEAD (prevents deleting the only
///   ref pointing at the current commit).
/// - The branch tip commit is **not** reachable from HEAD — the branch is
///   unmerged; force delete is not provided.
/// - A linked worktree has the branch checked out and is dirty or locked.
///
/// # Warning conditions
///
/// - The branch has an upstream configured: the remote branch is NOT deleted
///   by this operation.
/// - A CLEAN linked worktree has the branch checked out: the plan removes the
///   worktree first, then deletes the branch (re-validated at execute time).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_delete_branch(repo: &Repository, name: &str) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD ──────────────────────────────────────
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

    // ── 3. Check blockers ────────────────────────────────────
    let mut blockers: Vec<PlanNote> = Vec::new();
    let mut warnings: Vec<PlanNote> = Vec::new();

    // Branch existence check.
    let branch_result = repo.find_branch(name, BranchType::Local);
    let branch = match branch_result {
        Ok(b) => b,
        Err(_) => {
            blockers.push(PlanNote::Common(CommonNote::BranchMissing {
                name: name.to_string(),
                in_repo: true,
            }));
            // Build minimal plan with blocker and return early.
            let predicted = StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            };
            return Ok(OperationPlan {
                disposition: PlanDisposition::for_blockers(&blockers),
                title: PlanTitle::Branch(BranchTitle::DeleteBranch {
                    name: name.to_string(),
                    tip: None,
                }),
                current,
                predicted,
                warnings,
                blockers,
                recovery: Some(PlanRecovery {
                    kind: RecoveryKind::Branch(BranchRecovery::DeleteBranch {
                        name: name.to_string(),
                        tip: None,
                    }),
                    commands: Vec::new(),
                }),
                head_at_plan: head,
                stash_count_at_plan: 0,
                preview_files: Vec::new(),
                preview_commits: Vec::new(),
                destructive: false,
            });
        }
    };

    // Resolve the branch tip OID (needed for merged check and recovery string).
    let tip_oid = branch
        .get()
        .target()
        .ok_or_else(|| GitError::Other(format!("branch '{}' has no target OID", name)))?;

    let tip_short = {
        let s = tip_oid.to_string();
        s.get(..8).unwrap_or(&s).to_string()
    };

    // Current-branch check (HEAD attached to this branch).
    if let Head::Attached {
        branch: ref head_branch,
        ..
    } = head
    {
        if head_branch == name {
            blockers.push(PlanNote::Branch(BranchNote::DeleteCurrentBranch {
                name: name.to_string(),
            }));
        }
    }

    // Worktree-checkout check (user report: AI-agent worktrees linger and pin
    // their branch; git's raw refusal was opaque). Dirty or locked worktrees
    // BLOCK — a clean one becomes part of the plan: remove it, then delete.
    if let Some(wt) = worktree_checkout_of(repo, name) {
        if wt.locked {
            blockers.push(PlanNote::Branch(BranchNote::DeleteBranchInLockedWorktree {
                name: name.to_string(),
                path: wt.path.display().to_string(),
            }));
        } else if wt.dirty {
            blockers.push(PlanNote::Branch(BranchNote::DeleteBranchInDirtyWorktree {
                name: name.to_string(),
                path: wt.path.display().to_string(),
            }));
        } else {
            warnings.push(PlanNote::Branch(BranchNote::DeleteRemovesPinningWorktree {
                name: name.to_string(),
                path: wt.path.display().to_string(),
            }));
        }
    }

    // Detached HEAD + tip == HEAD check.
    if let Head::Detached { ref target } = head {
        let head_oid_res = git2::Oid::from_str(target);
        if let Ok(head_oid) = head_oid_res {
            if head_oid == tip_oid {
                blockers.push(PlanNote::Branch(BranchNote::DeleteDetachedAtTip {
                    name: name.to_string(),
                }));
            }
        }
    }

    // Merged check: branch tip must be reachable from HEAD.
    // graph_descendant_of(a, b) returns true when a is a descendant of b,
    // i.e. b is reachable FROM a.  We want: HEAD can reach tip.
    // So: graph_descendant_of(head_oid, tip_oid) OR head_oid == tip_oid.
    //
    // This check is only meaningful when HEAD has a commit (Attached or Detached).
    let is_merged = match &head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            match git2::Oid::from_str(target) {
                Ok(head_oid) => {
                    head_oid == tip_oid
                        || repo.graph_descendant_of(head_oid, tip_oid).unwrap_or(false)
                }
                Err(_) => false,
            }
        }
        Head::Unborn { .. } => {
            // No commits at all: the branch cannot have been merged.
            false
        }
    };

    if !is_merged {
        blockers.push(PlanNote::Branch(BranchNote::DeleteUnmerged {
            name: name.to_string(),
            tip: tip_short.clone(),
        }));
    }

    // Upstream warning: remote branch is NOT deleted.
    let has_upstream = branch.upstream().is_ok();
    if has_upstream {
        warnings.push(PlanNote::Branch(BranchNote::DeleteKeepsRemote {
            name: name.to_string(),
        }));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the deleted branch disappears from the ref list.
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: current.dirty.clone(),
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    // ADR-0129 F-4: the restore command is structured data (`commands`) — the
    // UI reads `recovery.commands.first()` instead of parsing the display
    // text's second line.
    let recovery = PlanRecovery {
        kind: RecoveryKind::Branch(BranchRecovery::DeleteBranch {
            name: name.to_string(),
            tip: Some(tip_short.clone()),
        }),
        commands: vec![format!("git branch {} {}", name, tip_short)],
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Branch(BranchTitle::DeleteBranch {
            name: name.to_string(),
            tip: Some(tip_short),
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
// execute_delete_branch  (W2-DELETE, ADR-0014)
// ────────────────────────────────────────────────────────────

/// Delete the local branch named `name`.
///
/// # Design (ADR-0014)
///
/// Uses `Branch::delete()` — a **ref-only** deletion that does NOT modify the
/// working tree, index, HEAD, or any remote.  **Force delete is never used.**
///
/// Steps:
/// 1. [`preflight_check`] — verify HEAD has not moved since planning.
/// 2. Locate the branch via `repo.find_branch(name, BranchType::Local)`.
/// 3. Call `branch.delete()` to remove the local ref.
/// 4. Verify the branch is gone (`find_branch` now returns `Err`).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure, including:
/// - HEAD has moved since planning (preflight mismatch).
/// - Branch no longer exists at execute time (already deleted externally).
/// - `branch.delete()` fails for any reason.
/// - Post-delete verify finds the branch still present.
pub fn execute_delete_branch(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
) -> Result<(), GitError> {
    // ── 1. Preflight: HEAD must not have moved since planning ─
    preflight_check(repo, plan)?;

    // ── 2. Locate the branch ──────────────────────────────────
    let mut branch = repo
        .find_branch(name, BranchType::Local)
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", name, e.message())))?;

    // ── 2.2 Remove the pinning worktree, if the plan promised to ─────────
    // Re-detect at execute time (the preflight spirit: the world may have
    // changed since planning). A worktree that turned dirty or locked in the
    // meantime REFUSES instead of destroying work.
    if let Some(wt) = worktree_checkout_of(repo, name) {
        if wt.locked {
            return Err(GitError::Other(format!(
                "worktree '{}' is locked — not removing it",
                wt.path.display()
            )));
        }
        if wt.dirty {
            return Err(GitError::Other(format!(
                "worktree '{}' has uncommitted changes — not removing it",
                wt.path.display()
            )));
        }
        // Clean: delete the working directory, then prune the admin entry.
        std::fs::remove_dir_all(&wt.path).map_err(|e| {
            GitError::Other(format!(
                "failed to remove worktree dir '{}': {e}",
                wt.path.display()
            ))
        })?;
        let wt_handle = repo
            .find_worktree(&wt.name)
            .map_err(|e| GitError::Other(format!("worktree lookup failed: {}", e.message())))?;
        let mut opts = git2::WorktreePruneOptions::new();
        opts.valid(true);
        // NOTE: no [kagi] line here — kagi-git emits none (contract lines are
        // the UI layer's job via klog!); the delete-branch UI logs the removal.
        wt_handle
            .prune(Some(&mut opts))
            .map_err(|e| GitError::Other(format!("worktree prune failed: {}", e.message())))?;
    }

    // ── 2.5 Pre-clean the branch's config section ─────────────
    pre_clean_branch_config(repo, name);

    // ── 3. Delete the branch ref (ref-only, no WT change) ─────
    // Branch::delete() removes refs/heads/<name>. force=false would be the
    // --delete flag; here we rely on plan-time merged check instead.
    branch
        .delete()
        .map_err(|e| GitError::Other(format!("branch delete failed: {}", e.message())))?;

    // ── 4. Verify the branch is gone ─────────────────────────
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(GitError::Other(format!(
            "branch '{}' still exists after delete — unexpected state",
            name
        )));
    }

    Ok(())
}
