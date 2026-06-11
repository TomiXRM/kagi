//! Checkout, create-branch, stash-push, stash-apply, and cherry-pick operation pipelines — T013, T014, T015, T016
//!
//! Implements the **plan → preflight → execute** pipeline for:
//! - `checkout` (ADR-0004, Guarded class): `plan_checkout` / `preflight_check` / `execute_checkout`
//! - `create-branch` (ADR-0004, Safe class): `plan_create_branch` / `execute_create_branch`
//! - `stash-push` (ADR-0004, Guarded class): `plan_stash_push` / `execute_stash_push`
//! - `stash-apply` (ADR-0004, Guarded class): `plan_stash_apply` / `execute_stash_apply`
//! - `cherry-pick` (ADR-0004/0005, Guarded class): `plan_cherry_pick` / `execute_cherry_pick`
//!
//! The checkout operation is **always safe-mode only**: `CheckoutBuilder::safe()` is the only
//! strategy used.  Force-checkout and any reset/clean APIs are intentionally absent.
//!
//! The create-branch operation uses `repo.branch(name, &commit, false)` — force=false is a
//! **literal constant** and must never be changed.
//!
//! The stash-apply operation uses `repo.stash_apply(index, None)` **only**.
//! `stash_pop` and `stash_drop` are **never** called — apply is chosen so the
//! stash entry is preserved (safe side).
//!
//! The cherry-pick operation uses `repo.cherrypick_commit(&commit, &head_commit, 0, None)`
//! **exclusively** for both plan and execute — the working-tree variant `repo.cherrypick()` is
//! **never used**.  This keeps the repo state clean (no CHERRYPICK state, no abort needed).
//!
//! # Public API
//!
//! - [`plan_checkout`]          — generate an [`OperationPlan`] for checkout
//! - [`preflight_check`]        — verify HEAD has not moved since planning
//! - [`execute_checkout`]       — perform the checkout (safe-mode only)
//! - [`plan_create_branch`]     — generate an [`OperationPlan`] for branch creation
//! - [`execute_create_branch`]  — create the branch (force=false, no checkout)
//! - [`plan_stash_push`]        — generate an [`OperationPlan`] for stash push
//! - [`execute_stash_push`]     — stash local modifications (INCLUDE_UNTRACKED)
//! - [`plan_stash_apply`]       — generate an [`OperationPlan`] for stash apply
//! - [`execute_stash_apply`]    — apply a stash entry (apply only, no pop/drop)
//! - [`preflight_check_stash`]  — verify HEAD + stash count unchanged since planning
//! - [`plan_cherry_pick`]       — generate an [`OperationPlan`] for cherry-pick (in-memory, no WT touch)
//! - [`execute_cherry_pick`]    — apply a cherry-pick commit (in-memory → commit → checkout_head safe)
//!
//! # Environment variables (test / headless use only)
//!
//! | Variable            | Effect |
//! |---------------------|--------|
//! | `KAGI_PLAN_CHECKOUT=<branch>`  | generate a plan for `<branch>` and emit a plan log |
//! | `KAGI_CREATE_BRANCH=<name>`    | generate a create-branch plan for HEAD and emit a plan log |
//! | `KAGI_STASH_PUSH=1`            | generate a stash-push plan and emit a plan log |
//! | `KAGI_STASH_APPLY=<index>`     | generate a stash-apply plan for `<index>` and emit a plan log |
//! | `KAGI_CHERRY_PICK=<sha>`       | generate a cherry-pick plan for `<sha>` and emit a plan log |
//! | `KAGI_AUTO_CONFIRM=1`          | **(TEST-ONLY)** if there are no blockers, proceed directly to execute after planning. **Never set this in normal use.** |

use git2::{BranchType, Repository, StashFlags};

use super::{GitError, Head, resolve_head, status::{working_tree_status, ChangeKind, FileStatus}};
use super::log::CommitId;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// One-line summary of repository state for display in the plan modal.
///
/// Example: `head = "branch: main"`, `dirty = "1 modified, 1 untracked"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSummary {
    /// Description of HEAD, e.g. `"branch: main"` or `"detached: a1b2c3d4"`.
    pub head: String,
    /// Description of working-tree cleanliness, e.g. `"clean"` or
    /// `"1 staged, 2 modified, 3 untracked"`.
    pub dirty: String,
}

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
        (!status.staged.is_empty())
            .then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty())
            .then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty())
            .then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty())
            .then(|| format!("{} conflicted", status.conflicted.len())),
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
    let branch_exists = repo
        .find_branch(branch, BranchType::Local)
        .is_ok();

    if !branch_exists {
        blockers.push(format!(
            "Branch '{}' does not exist in this repository.",
            branch
        ));
    }

    // Already-HEAD check (only meaningful when HEAD is attached).
    if let Head::Attached { branch: ref current_branch, .. } = head {
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
// plan_create_branch
// ────────────────────────────────────────────────────────────

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
        (!status.staged.is_empty())
            .then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty())
            .then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty())
            .then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty())
            .then(|| format!("{} conflicted", status.conflicted.len())),
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
    let mut blockers: Vec<String> = Vec::new();

    // Empty name.
    if name.is_empty() {
        blockers.push("Branch name must not be empty.".to_string());
    }

    // Invalid name (use git2 ref validation on the full ref path).
    if !name.is_empty()
        && !git2::Reference::is_valid_name(&format!("refs/heads/{}", name))
    {
        blockers.push(format!(
            "Branch name '{}' is not a valid git ref name \
             (no spaces, '..', or other invalid characters).",
            name
        ));
    }

    // Leading `-` is rejected explicitly: although git2 considers it a valid ref name,
    // it is ambiguous on the command line (may be interpreted as a flag).
    if !name.is_empty() && name.starts_with('-') {
        blockers.push(format!(
            "Branch name '{}' must not start with '-'.",
            name
        ));
    }

    // Already-exists check.
    if !name.is_empty() && repo.find_branch(name, BranchType::Local).is_ok() {
        blockers.push(format!(
            "A branch named '{}' already exists in this repository.",
            name
        ));
    }

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
pub fn execute_create_branch(
    repo: &Repository,
    name: &str,
    at: &CommitId,
) -> Result<(), GitError> {
    // Resolve the target commit.
    let oid = git2::Oid::from_str(&at.0)
        .map_err(|e| GitError::Other(format!("invalid commit id '{}': {}", at.0, e.message())))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", at.short(), e.message())))?;

    // Create the branch.  force=false is a literal constant — never change this.
    repo.branch(name, &commit, false)
        .map_err(|e| GitError::Other(format!("branch creation failed: {}", e.message())))?;

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
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Count existing stashes ────────────────────────────
    let stash_count = count_stashes(repo)?;

    // ── 3. Build current StateSummary ────────────────────────
    let head_display = head.display();

    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty())
            .then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty())
            .then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty())
            .then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty())
            .then(|| format!("{} conflicted", status.conflicted.len())),
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
    if !status.is_dirty() {
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

    // Untracked files included in stash (warning, not blocker).
    if !status.untracked.is_empty() {
        warnings.push(format!(
            "{} untracked file(s) will be included in the stash \
             (equivalent to `git stash push -u`).",
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
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_push
// ────────────────────────────────────────────────────────────

/// Execute a stash push: save all local modifications (including untracked
/// files) to a new stash entry.
///
/// Uses `repo.stash_save2(&sig, message, Some(StashFlags::INCLUDE_UNTRACKED))`.
/// The signature is read from the repository config (`user.name` / `user.email`);
/// if either is absent, falls back to `"kagi <kagi@local>"`.
///
/// **stash_pop and stash_drop are never called in this module.**
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
) -> Result<(), GitError> {
    // Build the signature from repo config, with fallback.
    let sig = build_signature(repo)?;

    repo.stash_save2(&sig, message, Some(StashFlags::INCLUDE_UNTRACKED))
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
pub fn plan_stash_apply(
    repo: &mut Repository,
    index: usize,
) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Collect stash entries ─────────────────────────────
    let stashes = collect_stash_entries(repo)?;
    let stash_count = stashes.len();

    // ── 3. Build current StateSummary ────────────────────────
    let head_display = head.display();

    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty())
            .then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty())
            .then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty())
            .then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty())
            .then(|| format!("{} conflicted", status.conflicted.len())),
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
    })
}

// ────────────────────────────────────────────────────────────
// execute_stash_apply
// ────────────────────────────────────────────────────────────

/// Apply the stash entry at `index` to the working tree.
///
/// Uses `repo.stash_apply(index, None)`.
///
/// **stash_pop and stash_drop are never called in this module.**
/// The stash entry at `index` is preserved after this call.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure (including apply
/// conflicts — in that case the stash entry remains intact).
pub fn execute_stash_apply(
    repo: &mut Repository,
    index: usize,
) -> Result<(), GitError> {
    repo.stash_apply(index, None)
        .map_err(|e| GitError::Other(format!("stash apply failed: {}", e.message())))?;
    Ok(())
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
            if expected_stash_count == 1 { "y" } else { "ies" },
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

/// Build a `git2::Signature` from the repository config.
///
/// Falls back to `"kagi <kagi@local>"` if either `user.name` or `user.email`
/// is not configured.
fn build_signature(repo: &Repository) -> Result<git2::Signature<'static>, GitError> {
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
        (!status.staged.is_empty())
            .then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty())
            .then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty())
            .then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty())
            .then(|| format!("{} conflicted", status.conflicted.len())),
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
            "HEAD is detached. Cherry-pick is only supported when HEAD is on a branch."
                .to_string(),
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
            repo.revparse_single(&id.0)
                .map(|obj| obj.id())
        })
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.0, e.message())))?;

    let commit = repo
        .find_commit(target_oid)
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.short(), e.message())))?;

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
        let recovery = format!(
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
        );
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
        let recovery = format!(
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
        );
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
        .map_err(|e| GitError::Other(format!("diff_tree_to_tree for preview failed: {}", e.message())))?;

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
        let recovery = format!(
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
             The previous HEAD sha is recorded in the reflog:\n  git reflog"
        );
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
            branch_name,
            summary_line
        ),
        dirty: "clean".to_string(),
    };

    let recovery = format!(
        "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
         The previous HEAD sha is recorded in the reflog:\n  git reflog"
    );

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

    let commit = repo
        .find_commit(target_oid)
        .map_err(|e| GitError::Other(format!("commit '{}' not found: {}", id.short(), e.message())))?;

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

    // ── 8. Create the new commit on HEAD ─────────────────────
    let new_oid = repo
        .commit(
            Some("HEAD"),
            &original_author,
            &committer,
            &original_message,
            &new_tree,
            &[&head_commit],
        )
        .map_err(|e| GitError::Other(format!("commit creation failed: {}", e.message())))?;

    // ── 9. Update the git index to match the new HEAD tree ───
    // repo.commit() creates the commit object but does NOT update the git
    // index on disk.  We must synchronise it so that the index matches the
    // new HEAD tree before updating the working tree.  Without this, the
    // working tree appears dirty (staged/unstaged mismatches).
    {
        let mut git_index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        git_index
            .read_tree(&new_tree)
            .map_err(|e| GitError::Other(format!("index.read_tree failed: {}", e.message())))?;
        git_index
            .write()
            .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;
    }

    // ── 10. Sync working tree to new HEAD (safe mode) ─────────
    // checkout_head(safe) + recreate_missing: the WT was clean before
    // execution, so safe mode will not refuse.  recreate_missing ensures that
    // new files introduced by the cherry-pick are written to disk (safe mode
    // alone does not guarantee that files absent from both baseline and workdir
    // are created).
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    cb.recreate_missing(true);
    repo.checkout_head(Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_head after cherry-pick failed: {}", e.message())))?;

    Ok(CommitId(new_oid.to_string()))
}
