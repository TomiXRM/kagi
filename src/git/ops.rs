//! Checkout operation pipeline — T013
//!
//! Implements the **plan → preflight → execute** pipeline for the `checkout`
//! operation (ADR-0004, Guarded class).  The operation is **always safe-mode
//! only**: `CheckoutBuilder::safe()` is the only strategy used.  Force-checkout
//! and any reset/clean APIs are intentionally absent from this module.
//!
//! # Public API
//!
//! - [`plan_checkout`]   — generate an [`OperationPlan`] (blockers / warnings)
//! - [`preflight_check`] — verify HEAD has not moved since planning
//! - [`execute_checkout`] — perform the checkout (safe-mode only)
//!
//! # Environment variables (test / headless use only)
//!
//! | Variable            | Effect |
//! |---------------------|--------|
//! | `KAGI_PLAN_CHECKOUT=<branch>` | generate a plan for `<branch>` and emit a plan log |
//! | `KAGI_AUTO_CONFIRM=1`         | **(TEST-ONLY)** if there are no blockers, proceed directly to execute after planning. **Never set this in normal use.** |

use git2::{BranchType, Repository};

use super::{GitError, Head, resolve_head, status::working_tree_status};

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

/// A complete plan describing what a checkout operation will do, including
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
    /// means the Checkout button must be hidden.
    pub blockers: Vec<String>,
    /// Plain-text recovery guidance shown to the user before they confirm.
    pub recovery: String,
    /// The HEAD state captured *at plan time*, used by [`preflight_check`] to
    /// detect whether the repo has changed between planning and execution.
    pub(crate) head_at_plan: Head,
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
