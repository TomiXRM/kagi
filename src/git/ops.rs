//! Checkout, create-branch, create-worktree, stash-push, stash-apply, stash-pop, cherry-pick, pull, push, undo-commit, and delete-branch operation pipelines — T013〜T016, T-HT-003/004/007/009, W2-DELETE
//!
//! Implements the **plan → preflight → execute** pipeline for:
//! - `checkout` (ADR-0004, Guarded class): `plan_checkout` / `preflight_check` / `execute_checkout`
//! - `checkout commit` (ADR-0022): `plan_checkout_commit` / `preflight_check` / `execute_checkout_commit`
//! - `create-branch` (ADR-0004, Safe class): `plan_create_branch` / `execute_create_branch`
//! - `create-worktree` (ADR-0025, Safe-create class): `plan_create_worktree` / `execute_create_worktree`
//! - `stash-push` (ADR-0004, Guarded class): `plan_stash_push` / `execute_stash_push`
//! - `stash-apply` (ADR-0004, Guarded class): `plan_stash_apply` / `execute_stash_apply`
//! - `stash-pop` (ADR-0009, Destructive-緩和): `plan_stash_pop` / `execute_stash_pop`
//! - `cherry-pick` (ADR-0004/0005, Guarded class): `plan_cherry_pick` / `execute_cherry_pick`
//! - `pull` (ADR-0004/0005/0009, Guarded class): `plan_pull` / `execute_pull`
//! - `delete-branch` (ADR-0014, Safe-class + merged-only guard): `plan_delete_branch` / `execute_delete_branch`
//!
//! The checkout operation is **always safe-mode only**: `CheckoutBuilder::safe()` is the only
//! strategy used.  Force-checkout and any reset/clean APIs are intentionally absent.
//!
//! The create-branch operation uses `repo.branch(name, &commit, false)` — force=false is a
//! **literal constant** and must never be changed.
//!
//! The stash-apply operation uses `repo.stash_apply(index, None)` **only**.
//! The stash-pop operation uses `repo.stash_apply(index, None)` then, **only on success**,
//! calls the private `stash_drop_internal` helper.  `repo.stash_drop` is **never** called
//! directly from public API.
//!
//! The cherry-pick operation uses `repo.cherrypick_commit(&commit, &head_commit, 0, None)`
//! **exclusively** for both plan and execute — the working-tree variant `repo.cherrypick()` is
//! **never used**.  This keeps the repo state clean (no CHERRYPICK state, no abort needed).
//!
//! The delete-branch operation uses `Branch::delete()` — a ref-only deletion that does NOT
//! touch the working tree.  **Force delete is intentionally absent.**  Only branches whose
//! tip commit is reachable from HEAD (merged) may be deleted; unmerged branches are a blocker.
//!
//! # Public API
//!
//! - [`plan_checkout`]          — generate an [`OperationPlan`] for checkout
//! - [`plan_checkout_commit`]   — generate an [`OperationPlan`] for detached commit checkout
//! - [`preflight_check`]        — verify HEAD has not moved since planning
//! - [`execute_checkout`]       — perform the checkout (safe-mode only)
//! - [`execute_checkout_commit`] — detach HEAD at a commit (safe-mode only)
//! - [`plan_create_branch`]     — generate an [`OperationPlan`] for branch creation
//! - [`execute_create_branch`]  — create the branch (force=false, no checkout)
//! - [`plan_create_worktree`]   — generate an [`OperationPlan`] for worktree + branch creation
//! - [`execute_create_worktree`] — create the branch and linked worktree
//! - [`plan_stash_push`]        — generate an [`OperationPlan`] for stash push
//! - [`execute_stash_push`]     — stash local modifications
//! - [`plan_stash_apply`]       — generate an [`OperationPlan`] for stash apply
//! - [`execute_stash_apply`]    — apply a stash entry (apply only, no pop/drop)
//! - [`plan_stash_pop`]         — generate an [`OperationPlan`] for stash pop (ADR-0009)
//! - [`execute_stash_pop`]      — apply then drop on success (pop = apply + drop-if-clean)
//! - [`preflight_check_stash`]  — verify HEAD + stash count unchanged since planning
//! - [`plan_cherry_pick`]       — generate an [`OperationPlan`] for cherry-pick (in-memory, no WT touch)
//! - [`execute_cherry_pick`]    — apply a cherry-pick commit (in-memory → commit → checkout_head safe)
//! - [`plan_pull`]              — generate an [`OperationPlan`] for pull (fetch + merge/fast-forward)
//! - [`execute_pull`]           — run fetch(CLI) then merge/FF (in-memory, no MERGING state)
//! - [`plan_delete_branch`]     — generate an [`OperationPlan`] for branch deletion (merged only)
//! - [`execute_delete_branch`]  — delete the branch ref (no working-tree changes, no force)
//!
//! # Environment variables (test / headless use only)
//!
//! | Variable            | Effect |
//! |---------------------|--------|
//! | `KAGI_PLAN_CHECKOUT=<branch>`  | generate a plan for `<branch>` and emit a plan log |
//! | `KAGI_CHECKOUT_COMMIT=<row>`    | generate a detached checkout plan for commit row and emit a plan log |
//! | `KAGI_CREATE_BRANCH=<name>`    | generate a create-branch plan for HEAD and emit a plan log |
//! | `KAGI_PLAN_WORKTREE=<name>:<path>` | generate a create-worktree plan for HEAD and emit a plan log |
//! | `KAGI_STASH_PUSH=1`            | generate a stash-push plan and emit a plan log |
//! | `KAGI_STASH_APPLY=<index>`     | generate a stash-apply plan for `<index>` and emit a plan log |
//! | `KAGI_CHERRY_PICK=<sha>`       | generate a cherry-pick plan for `<sha>` and emit a plan log |
//! | `KAGI_PULL=1`                  | generate a pull plan and emit a plan log |
//! | `KAGI_DELETE_BRANCH=<name>`    | generate a delete-branch plan for `<name>` and emit a plan log |
//! | `KAGI_AUTO_CONFIRM=1`          | **(TEST-ONLY)** if there are no blockers, proceed directly to execute after planning. **Never set this in normal use.** |

use std::path::{Component, Path, PathBuf};

use git2::{BranchType, Repository, StashFlags, WorktreeAddOptions};
use kagi_domain::head::Head;

use super::cli::run_git;
use super::log::CommitId;
use super::{
    resolve_head,
    status::{working_tree_status, ChangeKind, FileStatus},
    GitError,
};

pub use kagi_domain::plan::{
    AmendMode, AmendOutcome, BranchNameError, BranchRenameValidation, DiscardBackup,
    DiscardOutcome, FetchOutcome, MergeKind, PullOutcome, PushOutcome, StateSummary, UndoOutcome,
    WorktreePathError, WorktreeValidationError,
};

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// A complete plan describing what an operation will do, including
/// any blockers that prevent execution and warnings that should be surfaced.
///
/// If `blockers` is non-empty the UI **must not** offer the Execute button.
#[derive(Debug, Clone)]
pub struct OperationPlan {
    /// Human-readable title, e.g. `"Checkout branch 'feature/two'"`.
    pub title: String,
    /// Repository state *before* the operation.
    pub current: StateSummary,
    /// Predicted repository state *after* the operation.
    pub predicted: StateSummary,
    /// Non-fatal observations (shown in yellow).  The operation can still
    /// proceed if there are warnings but no blockers.
    pub warnings: Vec<String>,
    /// Conditions that prevent execution (shown in red).  At least one blocker
    /// means the Execute button must be hidden.
    pub blockers: Vec<String>,
    /// Plain-text recovery guidance shown to the user before they confirm.
    pub recovery: String,
    /// The HEAD state captured *at plan time*, used by [`preflight_check`] to
    /// detect whether the repo has changed between planning and execution.
    pub(crate) head_at_plan: Head,
    /// Number of stash entries captured at plan time.  Used by
    /// [`preflight_check_stash`] to detect concurrent stash modifications.
    /// For non-stash operations this is always `0`.
    pub(crate) stash_count_at_plan: usize,
    /// Files that will be changed by the operation, as computed by an in-memory
    /// dry run.  Non-empty only for cherry-pick plans.  Used by the plan modal
    /// to render a preview file tree (T016).
    pub preview_files: Vec<FileStatus>,
    /// Commits that will be pushed, as `"<short>  <summary>"` strings.
    /// Non-empty only for push plans (T-HT-004).  Shown in the plan modal
    /// (newest first, capped at 100 entries at plan time).
    pub preview_commits: Vec<String>,
    /// History-rewriting flag (ADR-0023).  `true` for plans that rewrite
    /// history (e.g. amend), which the UI must gate behind a **two-stage
    /// confirmation**.  Defaults to `false` for every other plan.
    pub destructive: bool,
}

impl OperationPlan {
    /// Return the stash entry count captured at plan time.
    ///
    /// Pass this value to [`preflight_check_stash`] to verify that the stash
    /// list has not changed since the plan was generated.
    pub fn stash_count_at_plan(&self) -> usize {
        self.stash_count_at_plan
    }
}

/// Validate a local branch rename target without touching repository state.
///
/// This intentionally accepts only `chars()`-level string checks here, then
/// delegates full refname syntax to libgit2's reference validator. Callers pass
/// `existing_branches` from a snapshot or test fixture so this remains a pure
/// function.
pub fn validate_branch_rename(
    old_name: &str,
    new_name: &str,
    existing_branches: &[String],
) -> BranchRenameValidation {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return BranchRenameValidation::Invalid(BranchNameError::Required);
    }
    if trimmed != new_name {
        return BranchRenameValidation::Invalid(BranchNameError::Whitespace);
    }
    if trimmed == old_name {
        return BranchRenameValidation::Invalid(BranchNameError::SameName);
    }
    if existing_branches.iter().any(|name| name == trimmed) {
        return BranchRenameValidation::Invalid(BranchNameError::RenameExists(trimmed.to_string()));
    }

    let full_ref = format!("refs/heads/{}", trimmed);
    if !git2::Reference::is_valid_name(&full_ref) {
        return BranchRenameValidation::Invalid(BranchNameError::RenameInvalid(
            trimmed.to_string(),
        ));
    }

    BranchRenameValidation::Valid
}

fn status_summary_display(status: &super::status::WorkingTreeStatus) -> String {
    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    }
}

// ────────────────────────────────────────────────────────────
// plan_checkout
// ────────────────────────────────────────────────────────────

/// Analyse whether checking out `branch` is safe and return an [`OperationPlan`].
///
/// # Blocker conditions (ADR-0004 Guarded policy)
///
/// - Target branch does not exist in the repository.
/// - Target branch is already the current HEAD branch (no-op would be confusing).
/// - Repository is in a conflict state (`status.conflicted` is non-empty).
/// - Staged **or** unstaged changes exist — checking out could lose work.
///   The user is instructed to stash their changes first.
///
/// # Warning conditions
///
/// - Untracked files exist.  The checkout itself will not touch them but users
///   are reminded they remain after switching branches.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_checkout(repo: &Repository, branch: &str) -> Result<OperationPlan, GitError> {
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
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Branch existence check.
    let branch_exists = repo.find_branch(branch, BranchType::Local).is_ok();

    if !branch_exists {
        blockers.push(format!(
            "Branch '{}' does not exist in this repository.",
            branch
        ));
    }

    // Already-HEAD check (only meaningful when HEAD is attached).
    if let Head::Attached {
        branch: ref current_branch,
        ..
    } = head
    {
        if current_branch == branch {
            blockers.push(format!(
                "Branch '{}' is already the current HEAD branch.",
                branch
            ));
        }
    }

    // Conflict state check.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before switching branches.",
            status.conflicted.len()
        ));
    }

    // Staged / unstaged changes — Guarded policy: block execution to prevent
    // accidental loss of work.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree has {} — changes could be lost. \
             Stash your changes before switching branches: \
             `git stash push -u` then `git stash pop` after checkout.",
            parts.join(", ")
        ));
    }

    // Untracked files — warning only (safe checkout leaves them alone).
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain after switching branches.",
            status.untracked.len()
        ));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD will point to the target branch; dirty is unchanged (we only update
    // the head description; working-tree state is preserved or unchanged).
    let predicted = StateSummary {
        head: format!("branch: {}", branch),
        dirty: current.dirty.clone(),
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let current_branch_name = match &head {
        Head::Attached { branch: b, .. } => b.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
        Head::Unborn { branch: b } => b.clone(),
    };
    let recovery = format!(
        "If anything goes wrong you can return to '{}' with:\n  git checkout {}\n\
         The reflog records every HEAD movement:\n  git reflog",
        current_branch_name, current_branch_name
    );

    Ok(OperationPlan {
        title: format!("Checkout branch '{}'", branch),
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
// preflight_check
// ────────────────────────────────────────────────────────────

/// Verify that HEAD has not changed since the plan was generated.
///
/// If the repository state has diverged (e.g. another process committed or
/// checked out a different branch), this returns an error so the caller can
/// abort and ask the user to re-plan.
///
/// # Errors
///
/// Returns [`GitError::Other`] when HEAD has changed or on unexpected failures.
pub fn preflight_check(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    let current_head = resolve_head(repo)?;
    if current_head != plan.head_at_plan {
        return Err(GitError::Other(format!(
            "Repository state changed since planning. \
             HEAD was {:?} at plan time but is now {:?}. \
             Please re-plan before proceeding.",
            plan.head_at_plan, current_head
        )));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// execute_checkout
// ────────────────────────────────────────────────────────────

/// Execute a branch checkout using **safe mode only**.
///
/// This function performs the two-step libgit2 checkout:
/// 1. `repo.checkout_tree(target_tree, Some(CheckoutBuilder::new().safe()))` —
///    update the working tree and index to match the target branch tip.
/// 2. `repo.set_head("refs/heads/<branch>")` — point HEAD at the target branch.
///
/// The order (checkout_tree **before** set_head) is intentional: updating the
/// working tree before moving HEAD ensures that if the checkout fails mid-way,
/// HEAD still points to the original branch.
///
/// **Force checkout and all reset/clean APIs are explicitly not used here.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure, including safe-mode
/// conflicts where an untracked file would be overwritten.
pub fn execute_checkout(repo: &Repository, branch: &str) -> Result<(), GitError> {
    // Locate the branch reference.
    let branch_ref = repo
        .find_branch(branch, BranchType::Local)
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", branch, e.message())))?;

    // Peel to the commit, then to the tree object for checkout_tree.
    let obj = branch_ref
        .get()
        .peel_to_commit()
        .map_err(|e| GitError::Other(e.message().to_string()))?
        .into_object();

    // Safe-mode checkout: will NOT overwrite modified tracked files.
    // Force is intentionally absent.
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();

    repo.checkout_tree(&obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;

    // Update HEAD to point at the new branch.
    let refname = format!("refs/heads/{}", branch);
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head failed: {}", e.message())))?;

    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_checkout_commit / execute_checkout_commit
// ────────────────────────────────────────────────────────────

/// Analyse whether checking out `id` as detached HEAD is safe and return an
/// [`OperationPlan`].
///
/// Dirty working-tree state is surfaced as a warning, not a blocker: execution
/// still uses `checkout_tree(...safe())`, so libgit2 refuses rather than
/// overwriting local changes. This keeps the normal plan → confirm → preflight
/// → execute → verify pipeline intact while preserving the repository on safe
/// checkout failure.
pub fn plan_checkout_commit(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut warnings = vec![
        "detached HEAD になります。新しい作業を残す場合は branch を作成してください。".to_string(),
        "Create branch here を先に使うことを推奨します。".to_string(),
    ];
    let mut blockers = Vec::new();

    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;
    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if matches!(&head, Head::Attached { target, .. } | Head::Detached { target } if target == &target_oid.to_string())
    {
        blockers.push("Commit is already HEAD.".to_string());
    }

    if status.is_dirty() {
        // BUG-2: in-memory dry-run. A safe-mode `checkout_tree` only fails when a
        // locally-modified tracked path overlaps a path the checkout would
        // rewrite (HEAD-tree → target-tree diff). If the dirty paths overlap that
        // set, the green "proceed" plan would error in the footer — promote the
        // warning to a blocker so the plan matches what execute actually does.
        match predict_checkout_commit_conflict(repo, &head, target_oid, &status) {
            Some(blocker) => blockers.push(blocker),
            None => {
                warnings.push(format!(
                    "Working tree is dirty ({}). Safe checkout may fail; stash or commit first.",
                    dirty_display
                ));
            }
        }
    }

    let target_short = id.short().to_string();
    let predicted = StateSummary {
        head: format!("detached: {}", target_short),
        dirty: dirty_display,
    };

    let current_ref = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
        Head::Unborn { branch } => branch.clone(),
    };
    let summary_line = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect::<String>();
    let recovery = format!(
        "If this was accidental, return with:\n  git checkout {}\n\
         To keep new work from the detached state, create a branch:\n  git switch -c <name>\n\
         The reflog records every HEAD movement:\n  git reflog",
        current_ref
    );

    Ok(OperationPlan {
        title: format!(
            "Checkout commit {} '{}' (detached HEAD)",
            target_short, summary_line
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

/// BUG-2 dry-run: predict whether a safe-mode checkout of `target_oid` would be
/// refused because a locally-modified tracked path overlaps a path the checkout
/// must rewrite.
///
/// Mirrors [`predict_stash_pop_conflict`] in spirit: pure in-memory analysis,
/// **never** touches the working tree or HEAD. Returns `Some(blocker)` when the
/// dirty (staged or unstaged) tracked paths intersect the HEAD-tree → target-tree
/// diff. Untracked files cannot conflict with a safe checkout, so they are
/// ignored here (libgit2 leaves them in place). On any analysis failure we return
/// `None` (fall back to the existing warning) — never invent a blocker we cannot
/// substantiate.
fn predict_checkout_commit_conflict(
    repo: &Repository,
    head: &Head,
    target_oid: git2::Oid,
    status: &super::status::WorkingTreeStatus,
) -> Option<String> {
    // Resolve the current HEAD tree (the baseline the checkout diffs against).
    let head_oid = match head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            git2::Oid::from_str(target).ok()?
        }
        Head::Unborn { .. } => return None,
    };
    let head_tree = repo.find_commit(head_oid).ok()?.tree().ok()?;
    let target_tree = repo.find_commit(target_oid).ok()?.tree().ok()?;

    // Paths the checkout would write (anything that differs between the two trees).
    let mut touched: std::collections::HashSet<String> = std::collections::HashSet::new();
    let diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&target_tree), None)
        .ok()?;
    for delta in diff.deltas() {
        if let Some(p) = delta.old_file().path() {
            touched.insert(p.to_string_lossy().into_owned());
        }
        if let Some(p) = delta.new_file().path() {
            touched.insert(p.to_string_lossy().into_owned());
        }
    }
    if touched.is_empty() {
        return None;
    }

    // Locally-modified tracked paths (staged + unstaged). Untracked excluded.
    let mut overlap: Vec<String> = Vec::new();
    for f in status.staged.iter().chain(status.unstaged.iter()) {
        let p = f.path.to_string_lossy().into_owned();
        if touched.contains(&p) && !overlap.contains(&p) {
            overlap.push(p);
        }
    }
    if overlap.is_empty() {
        return None;
    }
    overlap.sort();

    Some(format!(
        "Working tree has local changes to {} file(s) that the target commit also \
         modifies: {}. Safe checkout would be refused (1 conflict prevents checkout). \
         Stash or commit these changes first.",
        overlap.len(),
        overlap.join(", ")
    ))
}

/// Execute a detached commit checkout using **safe mode only**.
///
/// Order matters: this checks out the target tree while HEAD still points at
/// the old baseline, then detaches HEAD at the target commit. Moving HEAD first
/// would make safe checkout compare the target tree to itself and risk a no-op.
pub fn execute_checkout_commit(repo: &Repository, id: &CommitId) -> Result<(), GitError> {
    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;
    let obj = commit.into_object();

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(&obj, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree failed: {}", e.message())))?;

    repo.set_head_detached(target_oid)
        .map_err(|e| GitError::Other(format!("set_head_detached failed: {}", e.message())))?;

    Ok(())
}

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
    // the UI can localize them; their `Display` is pushed verbatim into the
    // English-only `blockers` Vec that the tests pin.
    let mut blockers: Vec<String> = create_branch_name_errors(repo, name)
        .iter()
        .map(|e| e.to_string())
        .collect();

    // Commit existence check.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())));
    let commit_exists = match oid {
        Ok(oid) => repo.find_commit(oid).is_ok(),
        Err(_) => false,
    };
    if !commit_exists {
        blockers.push(format!(
            "Commit '{}' does not exist in this repository.",
            at.short()
        ));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the new branch appears as an additional ref.
    let short_sha = at.short().to_string();
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "The new branch '{}' can be removed without side effects:\n  git branch -d {}\n\
         (Branch creation does not move HEAD or alter the working tree.)",
        name, name
    );

    Ok(OperationPlan {
        title: format!("Create branch '{}' @ {}", name, short_sha),
        current,
        predicted,
        warnings: Vec::new(),
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

// ────────────────────────────────────────────────────────────
// create-worktree helpers
// ────────────────────────────────────────────────────────────

/// Lexically normalize a path without requiring the final path to exist.
fn normalize_path(path: &Path) -> PathBuf {
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
        plan.blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before checking out the new branch.",
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
        plan.blockers.push(format!(
            "Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing.",
            parts.join(", ")
        ));
    }
    if !status.untracked.is_empty() {
        plan.warnings.push(format!(
            "{} untracked file(s) will remain after switching branches.",
            status.untracked.len()
        ));
    }

    plan.title = format!("Create branch '{}' @ {} and checkout", name, at.short());
    plan.predicted.head = format!("branch: {}", name);
    plan.recovery = format!(
        "This creates branch '{}' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d {}\nTo return after checkout:\n  git checkout {}",
        name,
        name,
        plan.current.head.strip_prefix("branch: ").unwrap_or("<previous-branch>")
    );
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
            title: format!("Open worktree for '{}'", branch),
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
            recovery: String::new(),
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
            plan.blockers.push(msg);
            if path.as_ref().is_absolute() {
                normalize_path(path.as_ref())
            } else {
                normalize_path(&repo_root.join(path.as_ref()))
            }
        }
    };
    plan.title = format!("Create worktree '{}' @ {}", branch, start.short());
    plan.predicted = StateSummary {
        head: plan.current.head.clone(),
        dirty: plan.current.dirty.clone(),
    };
    plan.recovery = format!(
        "Remove the linked worktree if needed:\n  git worktree remove {}\nThe branch can then be removed with:\n  git branch -d {}",
        target_path.display(),
        branch
    );
    plan.warnings.push(format!(
        "Creates a linked worktree at '{}' with branch '{}' (start point {}).",
        target_path.display(),
        branch,
        start.short()
    ));

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

// ────────────────────────────────────────────────────────────
// plan_stash_push
// ────────────────────────────────────────────────────────────

/// Analyse whether a stash push is safe and return an [`OperationPlan`].
///
/// Stash push is a **Guarded-class** operation (ADR-0004): it modifies the
/// working tree and index by saving all local modifications to a new stash
/// entry, leaving the working tree clean.
///
/// # Blocker conditions
///
/// - There are no local modifications (staged, unstaged, untracked all empty) —
///   nothing to stash.
/// - The repository is in a conflict state — stash cannot be created during
///   a merge conflict.
///
/// # Warning conditions
///
/// - Untracked files are included in the stash (equivalent to `git stash -u`).
///   This is intentional for convenience but is surfaced as a warning.
///
/// # Predicted state
///
/// - Working tree will be clean after the push.
/// - Stash count will increase by 1.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Count existing stashes ────────────────────────────
    let stash_count = count_stashes(repo)?;

    // ── 3. Build current StateSummary ────────────────────────
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

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Nothing to stash.
    // When include_untracked=false, untracked files don't count as "something to stash".
    let has_something_to_stash = if include_untracked {
        status.is_dirty()
    } else {
        !status.staged.is_empty() || !status.unstaged.is_empty()
    };
    if !has_something_to_stash {
        blockers.push(
            "Nothing to stash: working tree is already clean \
             (no staged, modified, or untracked files)."
                .to_string(),
        );
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before stashing.",
            status.conflicted.len()
        ));
    }

    // Untracked files included in stash (warning, not blocker) — only when include_untracked=true.
    if include_untracked && !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will be included in the stash \
             (equivalent to `git stash push -u`).",
            status.untracked.len()
        ));
    }

    // When include_untracked=false, warn that untracked files will NOT be stashed.
    if !include_untracked && !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will NOT be included in the stash \
             (include_untracked=false). They will remain in the working tree.",
            status.untracked.len()
        ));
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After push: working tree is clean, stash count +1.
    let msg_label = message.unwrap_or("(no message)");
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: "clean".to_string(),
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "To inspect stash entries:  git stash list\n\
         To restore without removing the stash entry:  git stash apply stash@{{0}}\n\
         Stash message that will be used: \"{}\"",
        msg_label
    );

    Ok(OperationPlan {
        title: format!(
            "Stash push — save local modifications ({})",
            stash_count + 1
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_push
// ────────────────────────────────────────────────────────────

/// Execute a stash push: save local modifications to a new stash entry.
///
/// When `include_untracked` is `true`, uses
/// `repo.stash_save2(&sig, message, Some(StashFlags::INCLUDE_UNTRACKED))`
/// (equivalent to `git stash push -u`).  When `false`, uses `StashFlags::DEFAULT`
/// so untracked files remain in the working tree.
///
/// The signature is read from the repository config (`user.name` / `user.email`);
/// if either is absent, falls back to `"kagi <kagi@local>"`.
///
/// **stash_drop is only called internally by `execute_stash_pop` — it is never
/// called from this function.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<(), GitError> {
    // Build the signature from repo config, with fallback.
    let sig = build_signature(repo)?;

    let flags = if include_untracked {
        Some(StashFlags::INCLUDE_UNTRACKED)
    } else {
        Some(StashFlags::DEFAULT)
    };

    repo.stash_save2(&sig, message, flags)
        .map_err(|e| GitError::Other(format!("stash push failed: {}", e.message())))?;

    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_stash_apply
// ────────────────────────────────────────────────────────────

/// Analyse whether applying stash entry at `index` is safe and return an
/// [`OperationPlan`].
///
/// Stash apply is a **Guarded-class** operation (ADR-0004): applying to a
/// dirty working tree risks mixing changes, so we require a clean tree.
///
/// # Blocker conditions
///
/// - `index` is out of range (no stash entry at that position).
/// - The repository is in a conflict state.
/// - The working tree is dirty (staged or unstaged changes exist) — applying
///   to a dirty tree risks unexpected merge conflicts mixing two sets of
///   changes.
///
/// # Predicted state
///
/// - Working tree will contain the stashed changes (dirty again).
/// - The stash entry **remains** in the stash list (apply, not pop).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_apply(repo: &mut Repository, index: usize) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Collect stash entries ─────────────────────────────
    let stashes = collect_stash_entries(repo)?;
    let stash_count = stashes.len();

    // ── 3. Build current StateSummary ────────────────────────
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

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<String> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(format!(
            "Stash index {} is out of range (only {} stash entr{} exist).",
            index,
            stash_count,
            if stash_count == 1 { "y" } else { "ies" }
        ));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before applying a stash.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged or unstaged) — MVP policy: clean only.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree is dirty ({}) — stash apply is only allowed on a clean \
             working tree to prevent accidental merge conflicts.",
            parts.join(", ")
        ));
    }

    // ── 5. Predicted StateSummary ─────────────────────────────
    // After apply: working tree will reflect the stash content.
    // The stash entry **remains** (apply, not pop).
    let stash_message = stashes
        .get(index)
        .map(|(_, msg)| msg.clone())
        .unwrap_or_else(|| format!("stash@{{{}}}", index));

    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: format!("restored from stash@{{{}}}", index),
    };

    // ── 6. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "The stash entry stash@{{{}}} is NOT removed by apply — it remains in the list.\n\
         If the apply caused conflicts, resolve them manually; the stash is safely preserved.\n\
         To see remaining stash entries:  git stash list\n\
         Stash message: \"{}\"",
        index, stash_message
    );

    Ok(OperationPlan {
        title: format!("Stash apply — restore stash@{{{}}}", index),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_apply
// ────────────────────────────────────────────────────────────

/// Apply the stash entry at `index` to the working tree.
///
/// Uses `repo.stash_apply(index, None)`.
///
/// **This function does NOT remove the stash entry** — the stash is preserved
/// after apply.  For apply + drop, use [`execute_stash_pop`] instead.
/// The stash entry at `index` is preserved after this call.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure (including apply
/// conflicts — in that case the stash entry remains intact).
pub fn execute_stash_apply(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply failed: {}", e.message())))?;
    Ok(())
}

// ────────────────────────────────────────────────────────────
// plan_stash_pop  (T-HT-007, ADR-0009)
// ────────────────────────────────────────────────────────────

/// Analyse whether a stash pop at `index` is safe and return an [`OperationPlan`].
///
/// Stash pop is a **Destructive-class (緩和付き)** operation (ADR-0009): on success
/// it applies the stash entry AND removes it from the stash list.  This is
/// irreversible — unlike apply, which preserves the stash entry.
///
/// # Design (ADR-0009)
///
/// The pop is blocked when a conflict is **predicted** via an in-memory merge of
/// `stash_commit` with the current HEAD.  The stash commit structure is:
///
/// ```text
/// stash@{N}  (the stash commit itself)
///   parent[0] = stash base commit (HEAD at stash-push time)
///   parent[1] = index snapshot commit
///   parent[2] = untracked files commit  (if INCLUDE_UNTRACKED was used)
/// ```
///
/// Conflict prediction: `repo.merge_commits(&head_commit, &stash_commit, None)`.
/// If the in-memory index has conflicts → blocker with a message recommending
/// `stash apply` instead, so the user can resolve conflicts without losing the
/// stash entry.
///
/// # Blocker conditions
///
/// - `index` out of range.
/// - Repository is in a conflict state.
/// - Working tree is dirty (staged or unstaged changes).
/// - Conflict **predicted** by in-memory merge of stash commit with HEAD
///   ("use apply instead, stash will not be consumed").
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_stash_pop(repo: &mut Repository, index: usize) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Collect stash entries with OIDs for conflict prediction ───────────
    let stashes_with_oid = collect_stash_entries_with_oid(repo)?;
    let stash_count = stashes_with_oid.len();
    let stashes: Vec<(usize, String)> = stashes_with_oid
        .iter()
        .map(|(i, msg, _)| (*i, msg.clone()))
        .collect();
    let stash_oid_for_index: Option<git2::Oid> = stashes_with_oid
        .iter()
        .find(|(i, _, _)| *i == index)
        .map(|(_, _, oid)| *oid);

    // ── 3. Build current StateSummary ────────────────────────
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

    // ── 4. Check blockers ────────────────────────────────────
    let mut blockers: Vec<String> = Vec::new();

    // Index out of range.
    if index >= stash_count {
        blockers.push(format!(
            "Stash index {} is out of range (only {} stash entr{} exist).",
            index,
            stash_count,
            if stash_count == 1 { "y" } else { "ies" }
        ));
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve conflicts before applying a stash.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged or unstaged) — same policy as stash apply.
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree is dirty ({}) — stash pop is only allowed on a clean \
             working tree to prevent accidental merge conflicts.",
            parts.join(", ")
        ));
    }

    // ── 5. Stash info + conflict prediction (only when index is valid) ────
    let stash_message = stashes
        .get(index)
        .map(|(_, msg)| msg.clone())
        .unwrap_or_else(|| format!("stash@{{{}}}", index));

    // Predict conflicts via in-memory merge of stash commit with HEAD.
    // Only run when we have no blockers so far (index valid, not dirty, no conflict state).
    if blockers.is_empty() {
        if let Some(stash_oid) = stash_oid_for_index {
            if let Some(conflict_blocker) = predict_stash_pop_conflict(repo, &head, stash_oid) {
                blockers.push(conflict_blocker);
            }
        }
    }

    // ── 6. Predicted StateSummary ─────────────────────────────
    // After pop: working tree reflects stash content; stash entry is REMOVED.
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: format!(
            "restored from stash@{{{}}} (stash entry will be removed)",
            index
        ),
    };

    // ── 7. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "WARNING: pop = apply + drop.  If apply succeeds, stash@{{{}}} is permanently removed.\n\
         The stash entry \"{}\" will be consumed.\n\
         To restore without removing the stash: use 'Stash Apply' instead.\n\
         To see remaining stash entries:  git stash list",
        index, stash_message
    );

    Ok(OperationPlan {
        title: format!("Stash pop — apply and remove stash@{{{}}}", index),
        current,
        predicted,
        warnings: Vec::new(),
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: stash_count,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_pop  (T-HT-007, ADR-0009)
// ────────────────────────────────────────────────────────────

/// Execute a stash pop: apply the stash entry at `index`, then drop it **only on success**.
///
/// # Design (ADR-0009 — Destructive 緩和付き)
///
/// 1. `repo.stash_apply(index, None)` — same as `execute_stash_apply`.
/// 2. If and **only if** the apply succeeds, call `stash_drop_internal(repo, index)`
///    to remove the stash entry.
/// 3. If apply fails for **any** reason, the drop is **not called** — the stash
///    entry remains intact.
///
/// This "apply first, drop on success only" approach prevents the catastrophic
/// case of losing the stash entry when apply produces conflicts or other errors.
/// ADR-0009 mandates conflict prediction as a blocker in `plan_stash_pop` so
/// the execute path should rarely see conflict failures in practice.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_pop(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    // Step 1: Apply the stash.
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply (pop phase) failed: {}", e.message())))?;

    // Step 2: Drop ONLY after successful apply.
    stash_drop_internal(repo, index)?;

    Ok(())
}

/// Drop stash entry at `index`.
///
/// # ADR-0004 / ADR-0009 — Why this is private
///
/// `stash_drop` is a **Destructive** operation (ADR-0004): it permanently removes
/// a stash entry with no recovery path.  ADR-0009 permits stash_drop **only as
/// the second step of a pop**, and **only when the preceding `stash_apply` has
/// already succeeded**.  Exposing it as a standalone public API would allow callers
/// to drop a stash entry without first verifying that the content was successfully
/// restored to the working tree — exactly the "stash lost, conflict unresolved"
/// footgun that ADR-0009 was designed to prevent.
///
/// This function is therefore intentionally `fn` (private to this module), not `pub fn`.
/// The only caller is [`execute_stash_pop`].
fn stash_drop_internal(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    repo.stash_drop(index)
        .map_err(|e| GitError::Other(format!("stash drop (pop phase) failed: {}", e.message())))
}

// ────────────────────────────────────────────────────────────
// Internal helper: stash pop conflict prediction
// ────────────────────────────────────────────────────────────

/// Predict whether applying a stash commit onto HEAD would produce merge conflicts.
///
/// Uses `repo.merge_commits(&head_commit, &stash_commit, None)` — an in-memory merge
/// that does NOT modify the working tree or repo state.
///
/// The `stash_oid` is the OID of the stash commit itself (parent[0] = base HEAD,
/// parent[1] = index snapshot, parent[2] = untracked files if applicable).
/// Merging the stash commit against the current HEAD predicts whether
/// `git stash apply` would conflict.
///
/// Returns `Some(blocker_message)` if a conflict is predicted, `None` if clean.
fn predict_stash_pop_conflict(
    repo: &Repository,
    head: &Head,
    stash_oid: git2::Oid,
) -> Option<String> {
    // Resolve HEAD OID.
    let head_oid = match head {
        Head::Attached { target, .. } | Head::Detached { target } => {
            git2::Oid::from_str(target).ok()?
        }
        Head::Unborn { .. } => return None,
    };

    let head_commit = repo.find_commit(head_oid).ok()?;
    let stash_commit = repo.find_commit(stash_oid).ok()?;

    // In-memory merge of HEAD with the stash commit: does NOT set MERGING state,
    // does NOT touch the working tree.
    let index_result = repo.merge_commits(&head_commit, &stash_commit, None).ok()?;

    if index_result.has_conflicts() {
        // Collect conflicting file paths.
        let mut conflict_files: Vec<String> = Vec::new();
        if let Ok(conflicts) = index_result.conflicts() {
            for c in conflicts.flatten() {
                let path_bytes: Option<Vec<u8>> = c
                    .our
                    .as_ref()
                    .map(|e| e.path.clone())
                    .or_else(|| c.their.as_ref().map(|e| e.path.clone()))
                    .or_else(|| c.ancestor.as_ref().map(|e| e.path.clone()));
                if let Some(p) = path_bytes {
                    conflict_files.push(String::from_utf8_lossy(&p).into_owned());
                }
            }
        }
        Some(format!(
            "Stash pop would produce {} conflict(s): {}. \
             Pop is blocked to prevent losing the stash entry. \
             Use 'Stash Apply' instead: it applies the stash without removing it, \
             allowing you to resolve conflicts safely.",
            conflict_files.len(),
            if conflict_files.is_empty() {
                "(unknown files)".to_string()
            } else {
                conflict_files.join(", ")
            }
        ))
    } else {
        None
    }
}

// ────────────────────────────────────────────────────────────
// preflight_check_stash
// ────────────────────────────────────────────────────────────

/// Extended preflight check for stash operations.
///
/// Verifies both:
/// 1. HEAD has not changed since the plan was generated (delegates to
///    [`preflight_check`]).
/// 2. The number of stash entries matches `expected_stash_count` — if another
///    process pushed or dropped a stash between planning and execution, abort.
///
/// # Errors
///
/// Returns [`GitError::Other`] when HEAD or stash count has changed, or on
/// unexpected failures.
pub fn preflight_check_stash(
    repo: &mut Repository,
    plan: &OperationPlan,
    expected_stash_count: usize,
) -> Result<(), GitError> {
    // 1. Head check (re-use existing).
    preflight_check(repo, plan)?;

    // 2. Stash count check.
    let current_count = count_stashes(repo)?;
    if current_count != expected_stash_count {
        return Err(GitError::Other(format!(
            "Stash list changed since planning: expected {} entr{}, \
             found {}. Please re-plan before proceeding.",
            expected_stash_count,
            if expected_stash_count == 1 {
                "y"
            } else {
                "ies"
            },
            current_count,
        )));
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────
// Internal helpers (stash)
// ────────────────────────────────────────────────────────────

/// Count the number of stash entries without allocating message strings.
fn count_stashes(repo: &mut Repository) -> Result<usize, GitError> {
    let mut count = 0usize;
    repo.stash_foreach(|_index, _message, _oid| {
        count += 1;
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(count)
}

/// Collect `(index, message)` pairs for all stash entries.
fn collect_stash_entries(repo: &mut Repository) -> Result<Vec<(usize, String)>, GitError> {
    let mut entries: Vec<(usize, String)> = Vec::new();
    repo.stash_foreach(|index, message, _oid| {
        entries.push((index, message.to_owned()));
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(entries)
}

/// Collect `(index, message, oid)` triples for all stash entries.
fn collect_stash_entries_with_oid(
    repo: &mut Repository,
) -> Result<Vec<(usize, String, git2::Oid)>, GitError> {
    let mut entries: Vec<(usize, String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        entries.push((index, message.to_owned(), *oid));
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;
    Ok(entries)
}

/// Build a `git2::Signature` from the repository config.
///
/// Falls back to `"kagi <kagi@local>"` if either `user.name` or `user.email`
/// is not configured.
pub(crate) fn build_signature(repo: &Repository) -> Result<git2::Signature<'static>, GitError> {
    let config = repo
        .config()
        .map_err(|e| GitError::Other(format!("failed to open config: {}", e.message())))?;

    let name = config
        .get_string("user.name")
        .unwrap_or_else(|_| "kagi".to_string());
    let email = config
        .get_string("user.email")
        .unwrap_or_else(|_| "kagi@local".to_string());

    git2::Signature::now(&name, &email)
        .map_err(|e| GitError::Other(format!("failed to create signature: {}", e.message())))
}

fn short_oid(oid: git2::Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

fn conflict_paths_from_index(index: &mut git2::Index) -> Result<Vec<String>, GitError> {
    let mut conflict_files = Vec::new();
    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
    for conflict_result in conflicts {
        let conflict = conflict_result
            .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
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
    Ok(conflict_files)
}

fn preview_files_between_trees(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<FileStatus>, GitError> {
    let mut diff = repo
        .diff_tree_to_tree(Some(old_tree), Some(new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }
    Ok(preview_files)
}

fn resolve_branch_commit<'repo>(
    repo: &'repo Repository,
    name: &str,
) -> Result<git2::Commit<'repo>, GitError> {
    repo.find_branch(name, BranchType::Local)
        .or_else(|_| repo.find_branch(name, BranchType::Remote))
        .and_then(|branch| branch.get().peel_to_commit())
        .or_else(|_| {
            repo.revparse_single(name)
                .and_then(|obj| obj.peel_to_commit())
        })
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", name, e.message())))
}

fn merge_dirty_warnings(status: &super::status::WorkingTreeStatus, op: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        warnings.push(format!(
            "Working tree has {}. Stash or commit before {} if you want a clean rollback point.",
            parts.join(", "),
            op
        ));
        warnings.push("Suggested command: git stash push -u".to_string());
    }
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched.",
            status.untracked.len()
        ));
    }
    warnings
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
// plan_cherry_pick  (T016)
// ────────────────────────────────────────────────────────────

/// Analyse whether cherry-picking `id` onto HEAD is safe and return an
/// [`OperationPlan`] with a preview of the files that would change.
///
/// # Core design (ADR-0005)
///
/// Uses `repo.cherrypick_commit(&commit, &head_commit, 0, None)` to build an
/// **in-memory index** — the working tree and repository state are **never
/// modified** by this function.  The `mainline` argument `0` is correct for
/// non-merge commits; merge commits are rejected as a blocker before reaching
/// this call.
///
/// # Blocker conditions
///
/// - Working tree has staged or unstaged changes (dirty).
/// - Repository is in a conflict state.
/// - `id` is a merge commit (parent_count > 1).
/// - `id` is identical to the current HEAD commit.
/// - HEAD is unborn (no commits) or detached.
/// - The cherry-pick produces no changes (already applied).
/// - The in-memory merge predicts conflicts.
///
/// # Warnings
///
/// - Untracked files are present (they are not touched).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn plan_cherry_pick(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
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

    // Unborn HEAD: no commits → cannot cherry-pick.
    if let Head::Unborn { .. } = &head {
        blockers.push(
            "HEAD is unborn (no commits exist). Cannot cherry-pick onto an empty branch."
                .to_string(),
        );
    }

    // Detached HEAD: MVP requires an attached branch.
    if let Head::Detached { .. } = &head {
        blockers.push(
            "HEAD is detached. Cherry-pick is only supported when HEAD is on a branch.".to_string(),
        );
    }

    // Conflict state in repo.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before cherry-picking.",
            status.conflicted.len()
        ));
    }

    // Dirty working tree (staged / unstaged).
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        let mut parts = Vec::new();
        if !status.staged.is_empty() {
            parts.push(format!("{} staged", status.staged.len()));
        }
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        blockers.push(format!(
            "Working tree has {} — stash or commit changes before cherry-picking.",
            parts.join(", ")
        ));
    }

    // Untracked files: warning only.
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched after cherry-pick.",
            status.untracked.len()
        ));
    }

    // ── 4. Resolve target commit ──────────────────────────────
    // Try both full and prefix match.
    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| {
            // Try short-sha prefix lookup via revparse.
            repo.revparse_single(&id.0).map(|obj| obj.id())
        })
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    // Merge commit check.
    if commit.parent_count() > 1 {
        blockers.push(format!(
            "Commit {} is a merge commit ({} parents). Cherry-picking merge commits \
             requires explicit mainline selection, which is not supported in MVP.",
            id.short(),
            commit.parent_count()
        ));
    }

    // HEAD-same check: resolve HEAD commit.
    let head_oid_opt = match &head {
        Head::Attached { target, .. } => git2::Oid::from_str(target).ok(),
        Head::Detached { target } => git2::Oid::from_str(target).ok(),
        Head::Unborn { .. } => None,
    };

    if let Some(head_oid) = head_oid_opt {
        if head_oid == target_oid {
            blockers.push(format!(
                "Commit {} is the current HEAD commit. Nothing to cherry-pick.",
                id.short()
            ));
        }
    }

    // ── 5. If early blockers, return without in-memory merge ─
    // (Prevents calling cherrypick_commit on unborn/detached/merge/HEAD-same)
    if !blockers.is_empty() {
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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

    // ── 6. Resolve HEAD commit (guaranteed to exist at this point) ─
    let head_commit = repo
        .find_commit(head_oid_opt.unwrap())
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 7. In-memory cherry-pick (core dry-run) ───────────────
    // repo.cherrypick_commit(&commit, &head_commit, mainline=0, None)
    // mainline=0 is correct for non-merge commits (already guarded above).
    // This does NOT modify the working tree or repo state.
    let mut index = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("cherry-pick in-memory failed: {}", e.message())))?;

    // ── 8. Conflict detection ─────────────────────────────────
    let mut conflict_files: Vec<String> = Vec::new();
    if index.has_conflicts() {
        // Collect conflicting paths.
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
        for conflict_result in conflicts {
            let conflict = conflict_result
                .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
            // Each of ours/theirs/ancestor may be Some; grab whichever has a path.
            // In git2, IndexEntry.path is a Vec<u8>.
            let path_bytes: Option<Vec<u8>> = conflict
                .our
                .as_ref()
                .map(|e| e.path.clone())
                .or_else(|| conflict.their.as_ref().map(|e| e.path.clone()))
                .or_else(|| conflict.ancestor.as_ref().map(|e| e.path.clone()));
            if let Some(p) = path_bytes {
                let path_str = String::from_utf8_lossy(&p).into_owned();
                conflict_files.push(path_str);
            }
        }
        blockers.push(format!(
            "Cherry-pick would produce {} conflict(s): {}. Resolve divergence before cherry-picking.",
            conflict_files.len(),
            conflict_files.join(", ")
        ));
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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

    // ── 9. Write in-memory tree and compute preview_files ─────
    // index.write_tree_to(repo) writes the in-memory tree without touching WT.
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;

    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree for preview failed: {}", e.message())))?;

    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("head tree lookup failed: {}", e.message())))?;

    // Diff head tree → cherry-picked tree to get preview files.
    let mut diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;

    // Enable rename detection (same as commit_changed_files).
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files: Vec<FileStatus> = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }

    // ── 10. Empty-result check (already applied) ─────────────
    if preview_files.is_empty() {
        blockers.push(format!(
            "Cherry-picking {} would produce no changes — it appears to have been applied already.",
            id.short()
        ));
        let branch_name = match &head {
            Head::Attached { branch, .. } => branch.clone(),
            _ => "(unknown)".to_string(),
        };
        let predicted = StateSummary {
            head: head_display.clone(),
            dirty: current.dirty.clone(),
        };
        let recovery =
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
                .to_string();
        return Ok(OperationPlan {
            title: format!("Cherry-pick {} onto {}", id.short(), branch_name),
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

    // ── 11. Build plan ─────────────────────────────────────────
    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => "(unknown)".to_string(),
    };

    // summary() returns Result<Option<&str>, Error> in git2 0.21.
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();

    let predicted = StateSummary {
        head: format!(
            "branch: {} (+1 commit: '{}' applied)",
            branch_name, summary_line
        ),
        dirty: "clean".to_string(),
    };

    let recovery = "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
         The previous HEAD sha is recorded in the reflog:\n  git reflog"
        .to_string();

    Ok(OperationPlan {
        title: format!(
            "Cherry-pick {} '{}' onto {}",
            id.short(),
            summary_line,
            branch_name
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_cherry_pick  (T016)
// ────────────────────────────────────────────────────────────

/// Execute a cherry-pick of commit `id` onto HEAD using an **in-memory index**.
///
/// # Design (ADR-0005, T016)
///
/// 1. Calls `repo.cherrypick_commit(&commit, &head_commit, 0, None)` to build
///    an in-memory index — identical to [`plan_cherry_pick`].  This does NOT
///    set the repository into CHERRYPICK state (unlike `repo.cherrypick()`
///    which is **never called** in this codebase).
/// 2. Verifies there are no conflicts (preflight-style double-check).  If
///    conflicts are detected, returns an error without writing anything.
/// 3. Calls `index.write_tree_to(repo)` to write the result tree to the ODB.
/// 4. Creates a new commit via `repo.commit(Some("HEAD"), original_author,
///    committer_from_config, original_message, &tree, &[&head_commit])`.
///    Author and message are preserved from the source commit; committer is
///    read from repo config (falls back to `"kagi <kagi@local>"`).
/// 5. Syncs the working tree to the new HEAD with
///    `repo.checkout_head(Some(CheckoutBuilder::new().safe()))`.
///
/// Returns the new commit's [`CommitId`].
///
/// **`repo.cherrypick()` (the working-tree variant) is never called.**
/// **No reset/force/clean APIs are used.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any failure.
pub fn execute_cherry_pick(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    // ── 1. Resolve target commit ──────────────────────────────
    let target_oid = git2::Oid::from_str(&id.0)
        .or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    // ── 2. Resolve HEAD commit ────────────────────────────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    // ── 3. In-memory cherry-pick (no WT, no repo state change) ─
    // mainline=0 is correct for non-merge commits.
    let mut index = repo
        .cherrypick_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("cherry-pick in-memory failed: {}", e.message())))?;

    // ── 4. Conflict preflight double-check ───────────────────
    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Cherry-pick of {} would produce conflicts. Re-plan before executing.",
            id.short()
        )));
    }

    // ── 5. Write in-memory tree to ODB ───────────────────────
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    // ── 6. Build committer signature ──────────────────────────
    let committer = build_signature(repo)?;

    // ── 7. Preserve author and message from source commit ────
    let original_author = commit.author();
    // message() returns Result<&str, Error> in git2 0.21.
    let original_message = commit
        .message()
        .unwrap_or("(cherry-picked commit)")
        .to_string();

    // ── 8. Create the new commit WITHOUT moving any ref ──────
    // ORDER MATTERS (same pitfall as the pull FF/merge paths): the WT/index
    // must be checked out while HEAD still points at the OLD tree so that
    // safe checkout sees old→new as the change set and updates modified
    // files.  Moving HEAD first turns the checkout into a no-op and leaves
    // stale WT content for files the picked commit modified.
    let new_oid = repo
        .commit(
            None,
            &original_author,
            &committer,
            &original_message,
            &new_tree,
            &[&head_commit],
        )
        .map_err(|e| GitError::Other(format!("commit creation failed: {}", e.message())))?;

    // ── 9. Sync WT + index to the new tree (old baseline) ────
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!(
                "checkout_tree after cherry-pick failed: {}",
                e.message()
            ))
        })?;

    // ── 10. Advance the branch ref to the new commit ─────────
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    repo.reference(
        &refname,
        new_oid,
        true,
        &format!("cherry-pick: {}", &new_oid.to_string()[..8]),
    )
    .map_err(|e| {
        GitError::Other(format!(
            "branch ref update (cherry-pick) failed: {}",
            e.message()
        ))
    })?;

    Ok(CommitId(new_oid.to_string()))
}

// ────────────────────────────────────────────────────────────
// plan_merge_branch / execute_merge_branch  (T-BCM-030 / W31-MERGE-INTO-CONFLICT)
// ────────────────────────────────────────────────────────────

/// Analyse whether merging `target` into the current branch is safe, returning
/// the [`OperationPlan`] paired with the [`MergeKind`] it will perform.
///
/// The dry run is entirely in-memory; no working-tree or ref writes occur during
/// planning. Real blockers (detached / unborn HEAD, already-contains, nothing to
/// merge, a pre-existing conflicted state) still populate `plan.blockers`. A
/// **predicted conflict is NOT a blocker** (W31): blockers stay empty, a warning
/// lists the conflicted file(s), `predicted.dirty` reflects the conflict count,
/// and the returned kind is [`MergeKind::Conflicts`].
pub fn plan_merge_branch(
    repo: &Repository,
    target: &str,
) -> Result<(OperationPlan, MergeKind), GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };
    let mut warnings = merge_dirty_warnings(&status, "merging");
    let mut blockers = Vec::new();

    let (current_branch, head_oid) = match &head {
        Head::Attached { branch, target } => {
            let oid = git2::Oid::from_str(target)
                .map_err(|e| GitError::Other(format!("HEAD oid parse failed: {}", e.message())))?;
            (branch.clone(), oid)
        }
        Head::Detached { .. } => {
            blockers.push("HEAD is detached. Merge is only supported on a branch.".to_string());
            (String::new(), git2::Oid::ZERO_SHA1)
        }
        Head::Unborn { .. } => {
            blockers.push("HEAD is unborn. Cannot merge into an empty branch.".to_string());
            (String::new(), git2::Oid::ZERO_SHA1)
        }
    };

    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before merging.",
            status.conflicted.len()
        ));
    }

    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();
    if !current_branch.is_empty() && target == current_branch {
        blockers.push(format!(
            "Branch '{}' is already the current branch.",
            target
        ));
    }
    if head_oid == target_oid {
        blockers.push(format!("{} is already HEAD. Nothing to merge.", target));
    } else if head_oid != git2::Oid::ZERO_SHA1
        && repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        blockers.push(format!(
            "Current branch '{}' already contains '{}'. Nothing to merge.",
            current_branch, target
        ));
    }

    let title = if current_branch.is_empty() {
        format!("Merge {} into current branch", target)
    } else {
        format!("Merge {} into {}", target, current_branch)
    };
    let recovery = "If this merge is not wanted after execution, use git reflog to find the previous HEAD.\n\
         Fast-forward merges can be undone by moving the branch back; merge commits can be reverted with git revert -m 1 <merge-commit>.".to_string();

    let blocked_plan =
        |blockers: Vec<String>, warnings: Vec<String>, current: StateSummary| OperationPlan {
            title: title.clone(),
            current: current.clone(),
            predicted: StateSummary {
                head: current.head.clone(),
                dirty: current.dirty.clone(),
            },
            warnings,
            blockers,
            recovery: recovery.clone(),
            head_at_plan: head.clone(),
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        };

    if !blockers.is_empty() {
        return Ok((
            blocked_plan(blockers, warnings, current),
            MergeKind::MergeCommit,
        ));
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;
    let target_tree = target_commit
        .tree()
        .map_err(|e| GitError::Other(format!("target tree lookup failed: {}", e.message())))?;
    let can_ff = repo
        .graph_descendant_of(target_oid, head_oid)
        .unwrap_or(false);

    // `kind` records what execution will do; `predicted_dirty` mirrors the
    // post-merge working-tree state shown in the plan card.
    let mut kind = MergeKind::MergeCommit;
    let mut predicted_dirty = "clean".to_string();

    let (preview_files, predicted_head) = if can_ff {
        kind = MergeKind::FastForward;
        (
            preview_files_between_trees(repo, &head_tree, &target_tree)?,
            format!(
                "branch: {} (fast-forward to {} {})",
                current_branch,
                target,
                short_oid(target_oid)
            ),
        )
    } else {
        let mut index = repo
            .merge_commits(&head_commit, &target_commit, None)
            .map_err(|e| {
                GitError::Other(format!("merge_commits in-memory failed: {}", e.message()))
            })?;
        if index.has_conflicts() {
            // W31: a predicted conflict is a WARNING + confirm, not a blocker.
            // We still produce a useful preview from the conflicted merge index
            // (it carries the merged-with-markers tree for the resolvable paths)
            // so the user sees the scope before confirming.
            let conflict_files = conflict_paths_from_index(&mut index)?;
            let files_label = if conflict_files.is_empty() {
                "(unknown files)".to_string()
            } else {
                conflict_files.join(", ")
            };
            warnings.push(format!(
                "Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode.",
                conflict_files.len(),
                files_label
            ));
            predicted_dirty = format!(
                "{} conflicted file(s) (resolve in Conflict Mode)",
                conflict_files.len()
            );
            kind = MergeKind::Conflicts(conflict_files);
            (
                // Preview from the pre-merge head tree to the target tree so the
                // card shows what is coming in (the merge itself is not written
                // here; execution does the real merge).
                preview_files_between_trees(repo, &head_tree, &target_tree)?,
                format!(
                    "branch: {} (merge {} {} — with conflicts)",
                    current_branch,
                    target,
                    short_oid(target_oid)
                ),
            )
        } else {
            let new_tree_oid = index.write_tree_to(repo).map_err(|e| {
                GitError::Other(format!("index.write_tree_to failed: {}", e.message()))
            })?;
            let new_tree = repo.find_tree(new_tree_oid).map_err(|e| {
                GitError::Other(format!("find_tree for preview failed: {}", e.message()))
            })?;
            (
                preview_files_between_trees(repo, &head_tree, &new_tree)?,
                format!(
                    "branch: {} (+1 merge commit from {} {})",
                    current_branch,
                    target,
                    short_oid(target_oid)
                ),
            )
        }
    };

    if preview_files.is_empty() && !matches!(kind, MergeKind::Conflicts(_)) {
        blockers.push(format!("Merging '{}' would produce no changes.", target));
        return Ok((
            blocked_plan(blockers, warnings, current),
            MergeKind::MergeCommit,
        ));
    }

    Ok((
        OperationPlan {
            title,
            current,
            predicted: StateSummary {
                head: predicted_head,
                dirty: predicted_dirty,
            },
            warnings,
            blockers,
            recovery,
            head_at_plan: head,
            stash_count_at_plan: 0,
            preview_files,
            preview_commits: Vec::new(),
            destructive: false,
        },
        kind,
    ))
}

/// Execute a branch merge planned by [`plan_merge_branch`].
///
/// Fast-forward execution checks out the target tree before moving the branch
/// ref. Non-fast-forward execution creates the merge commit without moving any
/// ref, checks out the merge tree, then advances the current branch.
pub fn execute_merge_branch(repo: &Repository, target: &str) -> Result<CommitId, GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is detached. Merge is only supported on a branch.".to_string(),
        ));
    }
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    let current_branch = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;
    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();

    if head_oid == target_oid
        || repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        return Err(GitError::Other(format!(
            "Current branch '{}' already contains '{}'. Re-plan before executing.",
            current_branch, target
        )));
    }

    if repo
        .graph_descendant_of(target_oid, head_oid)
        .unwrap_or(false)
    {
        let obj = target_commit.as_object();
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.safe();
        repo.checkout_tree(obj, Some(&mut cb)).map_err(|e| {
            GitError::Other(format!("checkout_tree (merge FF) failed: {}", e.message()))
        })?;
        let mut branch_ref = repo
            .find_reference(&refname)
            .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
        branch_ref
            .set_target(
                target_oid,
                &format!("merge: fast-forward {} into {}", target, current_branch),
            )
            .map_err(|e| {
                GitError::Other(format!(
                    "branch ref update (merge FF) failed: {}",
                    e.message()
                ))
            })?;
        repo.set_head(&refname)
            .map_err(|e| GitError::Other(format!("set_head (merge FF) failed: {}", e.message())))?;
        return Ok(CommitId(target_oid.to_string()));
    }

    let mut index = repo
        .merge_commits(&head_commit, &target_commit, None)
        .map_err(|e| GitError::Other(format!("merge_commits in-memory failed: {}", e.message())))?;
    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Merge of '{}' would produce conflicts. Re-plan before executing.",
            target
        )));
    }
    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;
    let committer = build_signature(repo)?;
    let author = committer.clone();
    let merge_message = format!("Merge branch '{}' into {}", target, current_branch);
    let new_oid = repo
        .commit(
            None,
            &author,
            &committer,
            &merge_message,
            &new_tree,
            &[&head_commit, &target_commit],
        )
        .map_err(|e| GitError::Other(format!("merge commit creation failed: {}", e.message())))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!("checkout_tree after merge failed: {}", e.message()))
        })?;
    let mut branch_ref = repo
        .find_reference(&refname)
        .map_err(|e| GitError::Other(format!("branch ref lookup failed: {}", e.message())))?;
    branch_ref
        .set_target(
            new_oid,
            &format!("merge: {} into {}", target, current_branch),
        )
        .map_err(|e| {
            GitError::Other(format!("branch ref update (merge) failed: {}", e.message()))
        })?;
    repo.set_head(&refname)
        .map_err(|e| GitError::Other(format!("set_head (merge) failed: {}", e.message())))?;

    Ok(CommitId(new_oid.to_string()))
}

/// Perform a **real** merge of `target` into the current branch that is expected
/// to conflict, leaving the repository in the standard git "merging with
/// conflicts" state (W31-MERGE-INTO-CONFLICT).
///
/// Unlike [`execute_merge_branch`], this does NOT create a commit. It uses
/// git2's real `Repository::merge` so the working tree gets conflict markers,
/// the index gets stage 1/2/3 entries, and `.git/MERGE_HEAD` is written — the
/// exact state [`crate::git::detect_conflict_session`] recognises and that
/// `plan_conflict_abort` / `execute_conflict_abort` can roll back.
///
/// `ORIG_HEAD` is written to the pre-merge HEAD so the conflict abort can
/// restore it (git2's `merge` does not write `ORIG_HEAD` itself). No force /
/// `reset --hard` / `clean` is used; the checkout is the default git merge
/// checkout. Returns the conflicted file paths from the index conflict iterator.
pub fn execute_merge_into_conflict(
    repo: &Repository,
    target: &str,
) -> Result<Vec<String>, GitError> {
    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    if !head_ref.is_branch() {
        return Err(GitError::Other(
            "HEAD is detached. Merge is only supported on a branch.".to_string(),
        ));
    }
    let current_branch = head_ref
        .shorthand()
        .map_err(|e| GitError::Other(format!("HEAD shorthand failed: {}", e.message())))?
        .to_string();
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;

    let target_commit = resolve_branch_commit(repo, target)?;
    let target_oid = target_commit.id();

    if head_oid == target_oid
        || repo
            .graph_descendant_of(head_oid, target_oid)
            .unwrap_or(false)
    {
        return Err(GitError::Other(format!(
            "Current branch '{}' already contains '{}'. Re-plan before executing.",
            current_branch, target
        )));
    }

    // Record the pre-merge HEAD so abort can restore it (git2::merge does not).
    repo.reference(
        "ORIG_HEAD",
        head_oid,
        true,
        &format!("merge {} into {}: ORIG_HEAD", target, current_branch),
    )
    .map_err(|e| GitError::Other(format!("write ORIG_HEAD failed: {}", e.message())))?;

    // Resolve the target as an annotated commit for the real merge.
    let annotated = repo
        .find_annotated_commit(target_oid)
        .map_err(|e| GitError::Other(format!("find_annotated_commit failed: {}", e.message())))?;

    // Default merge checkout: writes conflict markers + stage 1/2/3 index
    // entries + .git/MERGE_HEAD. No force / reset / clean.
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.safe();
    repo.merge(&[&annotated], None, Some(&mut checkout_opts))
        .map_err(|e| GitError::Other(format!("merge into conflict failed: {}", e.message())))?;

    // Collect the conflicted paths from the now-conflicted index.
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    let conflict_files = conflict_paths_from_index(&mut index)?;
    Ok(conflict_files)
}

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

/// Analyse whether reverting `id` on the current branch is safe and return an
/// [`OperationPlan`] built from an in-memory revert index.
///
/// The working tree and refs are not modified by this function. Merge commits
/// are refused even if callers failed to disable them in the menu.
pub fn plan_revert(repo: &Repository, id: &CommitId) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
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

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if let Head::Unborn { .. } = &head {
        blockers.push(
            "HEAD is unborn (no commits exist). Cannot revert on an empty branch.".to_string(),
        );
    }
    if let Head::Detached { .. } = &head {
        blockers.push(
            "HEAD is detached. Revert is only supported when HEAD is on a branch.".to_string(),
        );
    }
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). Resolve conflicts before reverting.",
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
        warnings.push(format!(
            "Working tree has {}. Safe checkout may refuse if those files overlap the revert.",
            parts.join(", ")
        ));
    }
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will remain untouched after revert.",
            status.untracked.len()
        ));
    }

    let target_oid = if id.0.len() == 40 {
        git2::Oid::from_str(&id.0).or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
    } else {
        repo.revparse_single(&id.0).map(|obj| obj.id())
    }
    .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if commit.parent_count() > 1 {
        blockers.push(format!(
            "Commit {} is a merge commit ({} parents). Reverting merge commits requires explicit mainline selection, which is not supported in MVP.",
            id.short(),
            commit.parent_count()
        ));
    }

    let head_oid_opt = match &head {
        Head::Attached { target, .. } => git2::Oid::from_str(target).ok(),
        Head::Detached { target } => git2::Oid::from_str(target).ok(),
        Head::Unborn { .. } => None,
    };

    if let Some(head_oid) = head_oid_opt {
        let is_on_current_branch = head_oid == target_oid
            || repo
                .graph_descendant_of(head_oid, target_oid)
                .unwrap_or(false);
        if !is_on_current_branch {
            blockers.push(format!(
                "Commit {} is not contained in the current branch. Revert only operates on current-branch commits.",
                id.short()
            ));
        }
    }

    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        _ => "(unknown)".to_string(),
    };
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();
    let recovery = "To undo this revert after execution, revert the new revert commit:\n  git revert <new-revert-commit-sha>\n\
         The previous HEAD sha is recorded in the reflog:\n  git reflog".to_string();

    let blocked_plan =
        |blockers: Vec<String>, warnings: Vec<String>, current: StateSummary| OperationPlan {
            title: format!(
                "Revert {} '{}' on {}",
                id.short(),
                summary_line,
                branch_name
            ),
            current: current.clone(),
            predicted: StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            },
            warnings,
            blockers,
            recovery: recovery.clone(),
            head_at_plan: head.clone(),
            stash_count_at_plan: 0,
            preview_files: Vec::new(),
            preview_commits: Vec::new(),
            destructive: false,
        };

    if !blockers.is_empty() {
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let head_oid =
        head_oid_opt.ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    let mut index = repo
        .revert_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("revert in-memory failed: {}", e.message())))?;

    if index.has_conflicts() {
        let mut conflict_files = Vec::new();
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
        for conflict_result in conflicts {
            let conflict = conflict_result
                .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
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
        blockers.push(format!(
            "Revert would produce {} conflict(s): {}. Resolve divergence before reverting.",
            conflict_files.len(),
            conflict_files.join(", ")
        ));
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree for preview failed: {}", e.message())))?;
    let head_tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("head tree lookup failed: {}", e.message())))?;

    let mut diff = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }

    if preview_files.is_empty() {
        blockers.push(format!(
            "Reverting {} would produce no changes.",
            id.short()
        ));
        return Ok(blocked_plan(blockers, warnings, current));
    }

    let predicted = StateSummary {
        head: format!(
            "branch: {} (+1 revert commit が新規作成されます: Revert \"{}\")",
            branch_name, summary_line
        ),
        dirty: if current.dirty == "clean" {
            "clean".to_string()
        } else {
            current.dirty.clone()
        },
    };

    Ok(OperationPlan {
        title: format!(
            "Revert {} '{}' on {}",
            id.short(),
            summary_line,
            branch_name
        ),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
    })
}

/// Execute a revert of `id` on HEAD using an in-memory index.
///
/// The ref-order rule is deliberately preserved: create the commit object,
/// safe-checkout the new tree while HEAD still points at the old baseline, then
/// move the current branch ref to the new commit.
pub fn execute_revert(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    let target_oid = if id.0.len() == 40 {
        git2::Oid::from_str(&id.0).or_else(|_| repo.revparse_single(&id.0).map(|obj| obj.id()))
    } else {
        repo.revparse_single(&id.0).map(|obj| obj.id())
    }
    .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo.find_commit(target_oid).map_err(|e| {
        GitError::Other(format!(
            "commit '{}' not found: {}",
            id.short(),
            e.message()
        ))
    })?;

    if commit.parent_count() > 1 {
        return Err(GitError::Other(format!(
            "Commit {} is a merge commit. Re-plan before executing.",
            id.short()
        )));
    }

    let head_ref = repo
        .head()
        .map_err(|e| GitError::Other(format!("HEAD lookup failed: {}", e.message())))?;
    let head_oid = head_ref
        .target()
        .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
    let refname = head_ref
        .name()
        .map_err(|e| GitError::Other(format!("HEAD name failed: {}", e.message())))?
        .to_string();
    if !refname.starts_with("refs/heads/") {
        return Err(GitError::Other(
            "HEAD is detached. Re-plan before executing.".to_string(),
        ));
    }

    let head_commit = repo
        .find_commit(head_oid)
        .map_err(|e| GitError::Other(format!("HEAD commit lookup failed: {}", e.message())))?;

    let mut index = repo
        .revert_commit(&commit, &head_commit, 0, None)
        .map_err(|e| GitError::Other(format!("revert in-memory failed: {}", e.message())))?;

    if index.has_conflicts() {
        return Err(GitError::Other(format!(
            "Revert of {} would produce conflicts. Re-plan before executing.",
            id.short()
        )));
    }

    let new_tree_oid = index
        .write_tree_to(repo)
        .map_err(|e| GitError::Other(format!("index.write_tree_to failed: {}", e.message())))?;
    let new_tree = repo
        .find_tree(new_tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    let committer = build_signature(repo)?;
    let summary_line: String = commit
        .summary()
        .ok()
        .flatten()
        .unwrap_or("(no message)")
        .chars()
        .take(72)
        .collect();
    let message = format!(
        "Revert \"{}\"\n\nThis reverts commit {}.\n",
        summary_line,
        commit.id()
    );

    let new_oid = repo
        .commit(
            None,
            &committer,
            &committer,
            &message,
            &new_tree,
            &[&head_commit],
        )
        .map_err(|e| GitError::Other(format!("commit creation failed: {}", e.message())))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(new_tree.as_object(), Some(&mut cb))
        .map_err(|e| {
            GitError::Other(format!(
                "checkout_tree after revert failed: {}",
                e.message()
            ))
        })?;

    repo.reference(
        &refname,
        new_oid,
        true,
        &format!("revert: {}", &new_oid.to_string()[..8]),
    )
    .map_err(|e| {
        GitError::Other(format!(
            "branch ref update (revert) failed: {}",
            e.message()
        ))
    })?;

    Ok(CommitId(new_oid.to_string()))
}

// ────────────────────────────────────────────────────────────
// PullOutcome  (T-HT-003)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// plan_pull  (T-HT-003)
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
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    if repo.find_branch(old_name, BranchType::Local).is_err() {
        blockers.push(format!("Branch '{}' does not exist.", old_name));
    }
    let existing = local_branch_names(repo)?;
    if let BranchRenameValidation::Invalid(reason) =
        validate_branch_rename(old_name, new_name, &existing)
    {
        blockers.push(reason.to_string());
    }
    if status.is_dirty() {
        warnings.push(
            "Working tree is dirty; branch rename is ref-only and will not touch files."
                .to_string(),
        );
    }
    warnings.push(
        "Remote branch names are not renamed automatically; only local branch config is carried over.".to_string(),
    );

    Ok(OperationPlan {
        title: format!("Rename branch '{}' to '{}'", old_name, new_name),
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
        recovery: format!(
            "This renames only the local ref. To undo: git branch -m {} {}",
            new_name, old_name
        ),
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
        title,
        current,
        predicted,
        warnings: Vec::new(),
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
        let (cl_blockers, cl_warnings) = crate::git::checklist::checklist(repo, &status)?;
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
        title,
        current,
        predicted,
        warnings,
        blockers,
        recovery,
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
///
/// # Warning conditions
///
/// - The branch has an upstream configured: the remote branch is NOT deleted
///   by this operation.
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
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Branch existence check.
    let branch_result = repo.find_branch(name, BranchType::Local);
    let branch = match branch_result {
        Ok(b) => b,
        Err(_) => {
            blockers.push(format!(
                "Branch '{}' does not exist in this repository.",
                name
            ));
            // Build minimal plan with blocker and return early.
            let predicted = StateSummary {
                head: head_display.clone(),
                dirty: current.dirty.clone(),
            };
            let recovery = format!(
                "Branch '{}' could not be found. Use `git branch` to list local branches.",
                name
            );
            return Ok(OperationPlan {
                title: format!("Delete branch '{}'", name),
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
            blockers.push(format!(
                "Branch '{}' is the currently checked-out branch. \
                 Checkout a different branch before deleting this one.",
                name
            ));
        }
    }

    // Detached HEAD + tip == HEAD check.
    if let Head::Detached { ref target } = head {
        let head_oid_res = git2::Oid::from_str(target);
        if let Ok(head_oid) = head_oid_res {
            if head_oid == tip_oid {
                blockers.push(format!(
                    "HEAD is detached and points to the same commit as '{}'. \
                     This branch cannot be deleted while HEAD is at its tip.",
                    name
                ));
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
        blockers.push(format!(
            "Branch '{}' has unmerged commits (tip {} is not reachable from HEAD). \
             Merge or discard the branch manually before deleting. \
             Force delete is not provided.",
            name, tip_short
        ));
    }

    // Upstream warning: remote branch is NOT deleted.
    let has_upstream = branch.upstream().is_ok();
    if has_upstream {
        warnings.push(format!(
            "Branch '{}' has an upstream tracking branch. \
             Only the local branch will be deleted; the remote branch is NOT removed.",
            name
        ));
    }

    // ── 4. Predicted StateSummary ─────────────────────────────
    // HEAD is unchanged; the deleted branch disappears from the ref list.
    let predicted = StateSummary {
        head: head_display.clone(),
        dirty: current.dirty.clone(),
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "To restore the deleted branch:\n  git branch {} {}\n\
         The branch tip commit '{}' remains in the object store until GC.",
        name, tip_short, tip_short
    );

    Ok(OperationPlan {
        title: format!("Delete branch '{}' (tip {})", name, tip_short),
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

    // ── 2.5 Pre-clean the branch's config section ─────────────
    // gh CLI is known to write duplicated `branch.<name>.*` keys (e.g.
    // github-pr-owner-number, one copy per `gh pr` invocation). libgit2's
    // Branch::delete() wipes the section key-by-key and aborts on the
    // duplicates ("could not find key … to delete") BEFORE deleting the
    // ref — so the first attempt fails and a retry succeeds. Remove the
    // section's entries tolerantly here so the ref deletion below cannot
    // be blocked by config garbage. Best-effort: a key that is already
    // gone (or lives in a read-only level) is not an error.
    {
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

// ────────────────────────────────────────────────────────────
// discard (W17-DISCARD, ADR-0046) — backup-then-discard
// ────────────────────────────────────────────────────────────

/// Normalise a user/UI-supplied path to the repository-relative, forward-slash
/// form that git status reports, so plan/execute and status comparisons line up.
fn discard_rel_path(repo: &Repository, raw: &str) -> String {
    let raw_path = Path::new(raw);
    // Strip a workdir prefix if an absolute path was given.
    let rel = repo
        .workdir()
        .and_then(|wd| std::fs::canonicalize(wd).ok())
        .and_then(|wd| {
            std::fs::canonicalize(raw_path)
                .ok()
                .and_then(|abs| abs.strip_prefix(&wd).ok().map(|p| p.to_path_buf()))
        })
        .unwrap_or_else(|| raw_path.to_path_buf());
    normalize_path(&rel).to_string_lossy().replace('\\', "/")
}

/// Analyse a discard of the given working-tree `paths` and return an
/// [`OperationPlan`] with `destructive: true` (ADR-0046).
///
/// **Semantics** (`git checkout -- <path>` equivalent): each target's working-tree
/// content is overwritten by the **index** content. The index (staged changes) and
/// all refs are left untouched.
///
/// # Blocker conditions
///
/// - `paths` is empty (nothing to discard).
/// - A target is a **conflicted** file (must be resolved via the conflict flow,
///   not stomped by discard).
/// - A target is an **untracked** file (discarding = deletion = `git clean`,
///   which is banned project-wide — the UI excludes these, this is the backstop).
/// - A target is not in the unstaged set at all (nothing to discard for it).
pub fn plan_discard(repo: &Repository, paths: &[String]) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = status_summary_display(&status);

    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Build the lookup sets from the current status (all repo-relative paths).
    let unstaged_set: std::collections::HashSet<String> = status
        .unstaged
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let untracked_set: std::collections::HashSet<String> = status
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let conflicted_set: std::collections::HashSet<String> = status
        .conflicted
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    let rels: Vec<String> = paths.iter().map(|p| discard_rel_path(repo, p)).collect();

    if rels.is_empty() {
        blockers.push("Nothing to discard: no files selected.".to_string());
    }

    // Count untracked targets — they are discarded by DELETING the file (after
    // an ODB backup), not by restoring from the index (ADR-0083).
    let mut untracked_targets = 0usize;
    for rel in &rels {
        if conflicted_set.contains(rel) {
            blockers.push(format!(
                "'{}' is conflicted. Resolve the conflict instead of discarding it.",
                rel
            ));
        } else if untracked_set.contains(rel) {
            untracked_targets += 1;
        } else if !unstaged_set.contains(rel) {
            blockers.push(format!("'{}' has no unstaged changes to discard.", rel));
        }
    }

    let target_count = rels.len();
    let predicted = StateSummary {
        head: head.display(),
        dirty: if blockers.is_empty() {
            format!("{} file(s) discarded", target_count)
        } else {
            dirty_display
        },
    };

    let title = if target_count == 1 {
        format!(
            "Discard changes to '{}'",
            rels.first().cloned().unwrap_or_default()
        )
    } else {
        format!("Discard changes to {} file(s)", target_count)
    };

    let recovery = "This discards your unstaged changes to the selected file(s): \
        tracked files are restored from the index, untracked files are deleted from \
        disk. Either way a backup blob of each file's current content is recorded in \
        the oplog (op=\"discard\") first; recover with `git cat-file -p <blob-sha>`."
        .to_string();

    // ADR-0083: untracked targets are DELETED (after an ODB backup). Surface this
    // as a warning so the confirm step is explicit about the irreversible-looking
    // (but recoverable) deletion.
    if untracked_targets > 0 {
        warnings.push(format!(
            "{} untracked file(s) will be deleted from disk (backed up to the oplog first; \
             recover with `git cat-file -p <blob-sha>`).",
            untracked_targets
        ));
    }

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
        preview_commits: Vec::new(),
        destructive: true,
    })
}

/// Execute a discard following the **mandatory** ADR-0046 order:
///
/// 1. **backup** — write each target's CURRENT working-tree content into the ODB
///    via `repo.blob()`, collecting `path → blob SHA`. If **any** backup fails,
///    the whole discard is aborted (no working-tree change is made).
/// 2. **apply** — *tracked* targets are restored from the index with
///    `checkout_index` + `force()` (`git checkout -- <path>` semantics); *untracked*
///    targets are DELETED from disk (ADR-0083 — recoverable via the step-1 backup,
///    so this is not `git clean`). The index and refs are never touched.
/// 3. **verify** — re-read status and confirm each target left the unstaged set
///    (tracked) or is gone from disk (untracked).
///
/// Returns the [`DiscardOutcome`] (the path→blob backup list) so the caller can
/// record it in the oplog as the recovery handle. The caller MUST have rejected
/// conflicted targets at plan time.
pub fn execute_discard(
    repo: &Repository,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<DiscardOutcome, GitError> {
    // ── 0. Refuse to run a plan that has blockers. ───────────
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(format!(
            "discard refused: plan has {} blocker(s)",
            plan.blockers.len()
        )));
    }
    preflight_check(repo, plan)?;

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repositories are not supported".to_string()))?
        .to_path_buf();

    let rels: Vec<String> = paths.iter().map(|p| discard_rel_path(repo, p)).collect();
    if rels.is_empty() {
        return Err(GitError::Other("discard: no target paths".to_string()));
    }

    // Classify targets up front: untracked targets are deleted, tracked targets
    // are restored from the index (ADR-0083).
    let status_before = working_tree_status(repo)?;
    let untracked_set: std::collections::HashSet<String> = status_before
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // ── 1. BACKUP — write each target's current WT content to the ODB. ──
    // Any failure aborts the whole discard BEFORE the working tree is touched.
    let mut backups: Vec<DiscardBackup> = Vec::with_capacity(rels.len());
    for rel in &rels {
        let abs = workdir.join(rel);
        // For an unstaged *deletion* the file is absent from the WT; back up an
        // empty blob so the recovery handle still exists and is uniform.
        let content: Vec<u8> = match std::fs::read(&abs) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                return Err(GitError::Other(format!(
                    "discard aborted: cannot read '{}' for backup: {}",
                    rel, e
                )));
            }
        };
        let oid = repo.blob(&content).map_err(|e| {
            GitError::Other(format!(
                "discard aborted: blob backup failed for '{}': {}",
                rel,
                e.message()
            ))
        })?;
        backups.push(DiscardBackup {
            path: rel.clone(),
            blob: oid.to_string(),
        });
    }

    // Partition into tracked (restore from index) vs untracked (delete).
    let (untracked_rels, tracked_rels): (Vec<&String>, Vec<&String>) =
        rels.iter().partition(|r| untracked_set.contains(*r));

    // ── 2a. checkout_index with path filter + force (restore WT from index). ──
    // update_index(false): the index (staged changes) is NEVER modified.
    if !tracked_rels.is_empty() {
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();
        cb.update_index(false);
        cb.disable_pathspec_match(true);
        for rel in &tracked_rels {
            cb.path(rel.as_str());
        }
        repo.checkout_index(None, Some(&mut cb)).map_err(|e| {
            GitError::Other(format!("discard: checkout_index failed: {}", e.message()))
        })?;
    }

    // ── 2b. DELETE untracked targets (ADR-0083; content backed up in step 1). ──
    for rel in &untracked_rels {
        let abs = workdir.join(rel);
        match std::fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(GitError::Other(format!(
                    "discard: failed to delete untracked file '{}': {}",
                    rel, e
                )));
            }
        }
    }

    // ── 3. VERIFY — tracked targets left the unstaged set; untracked are gone. ──
    let status = working_tree_status(repo)?;
    let still_unstaged: std::collections::HashSet<String> = status
        .unstaged
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    let still_untracked: std::collections::HashSet<String> = status
        .untracked
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let mut leftover: Vec<&String> = tracked_rels
        .iter()
        .copied()
        .filter(|r| still_unstaged.contains(*r))
        .collect();
    leftover.extend(
        untracked_rels
            .iter()
            .copied()
            .filter(|r| still_untracked.contains(*r)),
    );
    if !leftover.is_empty() {
        return Err(GitError::Other(format!(
            "discard verify failed: {} target(s) not discarded: {}",
            leftover.len(),
            leftover
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    Ok(DiscardOutcome { backups })
}

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
        title: format!(
            "{} {} on '{}' — {} → {}",
            label, kind_slug, branch, from_short, to_short
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
