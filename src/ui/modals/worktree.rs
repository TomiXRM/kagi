//! Worktree input and confirmation payloads for the single ActiveModal slot.

use super::ModalPlan;
use gpui::{Entity, SharedString};
use gpui_component::input::InputState;
use kagi_git::{CommitId, OperationPlan};
/// State for an unlock-worktree confirmation. The plan's warning carries the
/// recorded lock reason so the user sees why the lock was placed.
#[derive(Clone)]
pub struct UnlockWorktreeModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Worktree registry name the plan was built for.
    pub name: String,
}

/// State for a remove-worktree confirmation (issue #340). `delete_branch`
/// records whether the confirmed op also deletes the checked-out branch.
#[derive(Clone)]
pub struct RemoveWorktreeModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Worktree registry name the plan was built for.
    pub name: String,
    /// True when the op also deletes the branch (vs keeping it).
    pub delete_branch: bool,
}

/// Lock-worktree confirmation with the reason frozen by its input modal.
#[derive(Clone)]
pub struct LockWorktreeModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    pub name: String,
    /// Lock reason recorded in `git worktree lock --reason`.
    pub reason: String,
}

/// State for a prune-stale-worktrees confirmation (issue #340). Repo-wide;
/// the plan carries the dry-run preview (count + paths).
#[derive(Clone)]
pub struct PruneWorktreesModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

/// State for a repair-worktree-links confirmation (issue #340). Repo-wide.
#[derive(Clone)]
pub struct RepairWorktreesModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

/// State for an in-progress create-worktree confirmation.
#[derive(Clone)]
pub struct CreateWorktreeModal {
    /// The commit used as the start point for the new branch.
    pub at: CommitId,
    /// First line of the start commit message.
    pub start_title: String,
    /// New branch name (synced from `branch_state`).
    pub branch_input: String,
    /// Real branch-name input entity (lazy; None headless).
    pub branch_state: Option<Entity<InputState>>,
    /// Target worktree path (synced from `path_state`).
    pub path_input: String,
    /// Real path input entity (lazy; None headless).
    pub path_state: Option<Entity<InputState>>,
    /// True once the user has manually edited the path.
    pub path_touched: bool,
    /// True when this modal attaches an existing local branch to a worktree
    /// instead of creating a new branch first.
    pub allow_existing_branch: bool,
    /// Live plan regenerated from branch/path/start, or the explicit failure of
    /// the last replan (#510).
    pub plan: ModalPlan,
    /// Error message to show if execute or preflight failed. Plan failures live
    /// in `plan` instead, so they cannot leave a confirmable plan behind.
    pub error: Option<SharedString>,
}

/// Input-only step; reviewing the reason does not acquire a lock.
#[derive(Clone)]
pub struct WorktreeLockReasonModal {
    pub name: String,
    pub reason: String,
    pub input_state: Option<Entity<InputState>>,
    pub error: Option<SharedString>,
}
