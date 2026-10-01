//! Rebase / replay confirmation state (branch-menu "Integrate" group) and
//! how it presents on the shared plan card.

use gpui::SharedString;
use kagi_git::ops::OperationPlan;
use kagi_ui_core::i18n::Msg;

/// State for an in-progress rebase confirmation (branch-menu "Integrate"
/// group). One card for both ways of putting `branch` onto `onto`:
///
/// - `Operation::RebaseCurrentOnto` — the checked-out branch, via
///   `git rebase`. Single confirm: rebase is Guarded (ADR-0004), not
///   Destructive — it rewrites only the *local* branch, and a mid-rebase
///   conflict routes into the existing conflict editor (with its own Abort)
///   rather than silently losing anything.
/// - `Operation::ReplayOnto` (#344, ADR-0211) — any local branch, by ref
///   update only (`git replay`), no worktree touched. The plan is
///   `destructive` (history rewrite, ADR-0023), so `confirm_armed` gates a
///   second confirm.
#[derive(Clone)]
pub struct RebaseCurrentOntoModal {
    /// `RebaseCurrentOnto { onto }` or `ReplayOnto { branch, onto }`; what
    /// `start_rebase` runs.
    pub op: kagi_git::Operation,
    /// The rebase target (`git rebase <onto>`'s argument).
    pub onto: String,
    /// The branch being rebased (drives the confirm-button label, mirrors
    /// `MergePlanModal::into_branch`).
    pub branch: String,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Two-stage confirm gate for a `destructive` plan: `false` = first
    /// click pending, `true` = armed. Always `false` for `RebaseCurrentOnto`.
    pub confirm_armed: bool,
}

impl RebaseCurrentOntoModal {
    pub fn is_replay(&self) -> bool {
        matches!(self.op, kagi_git::Operation::ReplayOnto { .. })
    }

    /// Confirm-button label: rebase is one stage; replay swaps to the armed
    /// wording after the first confirm (ADR-0023).
    pub fn confirm_label(&self) -> String {
        if !self.is_replay() {
            Msg::PlanRebaseOnto.t().replace("{}", &self.branch)
        } else if self.confirm_armed {
            Msg::PlanReplayOntoArmed.t().to_string()
        } else {
            Msg::PlanReplayOnto.t().replace("{}", &self.branch)
        }
    }

    /// The plan to draw. A replay plan's `preview_commits` are the
    /// `update-ref --stdin` script the backend executes (`verify …` /
    /// `update …` lines), not commits: the card's commit list would draw
    /// them raw under "Commits to push". The user-facing list is the
    /// `ReplayUpdates` warning, so the card gets the plan without them.
    pub fn display_plan(&self) -> std::sync::Arc<OperationPlan> {
        if self.is_replay() && !self.plan.preview_commits.is_empty() {
            std::sync::Arc::new(OperationPlan {
                preview_commits: Vec::new(),
                ..(*self.plan).clone()
            })
        } else {
            self.plan.clone()
        }
    }
}
