//! Accessor methods for the single `active_modal` field (ADR-0076 /
//! issue #13 P7). For each former `Option<XModal>` field on `KagiApp`
//! these provide the per-modal `X()`, `X_mut()`, `set_X()`, `clear_X()`
//! and `take_X()` shims so call sites keep their per-modal names while
//! the storage is a single mutually-exclusive enum. Generated, behaviour-
//! preserving: `clear_X` only clears when X is the active modal, matching
//! the old `self.X = None` semantics.

use super::super::modals::ActiveModal;
use super::super::modals::{
    AmendPlanModal, BranchPlanModal, CheckoutPlanModal, CherryPickModal, ConflictContinuePlanModal,
    CreateBranchModal, CreateWorktreeModal, DeleteBranchModal, DiscardModal, HistoryPlanModal,
    MergePlanModal, PopPlanModal, PullPlanModal, PushPlanModal, RenameBranchModal, RevertModal,
    SetUpstreamModal, StashApplyModal, StashDropModal, StashPushModal, SwitchToLatestPlanModal,
    TrackingCheckoutPlanModal, UndoPlanModal,
};
use super::super::KagiApp;

// Accessors for the single `active_modal: Option<ActiveModal>` field
// (ADR-0076 / ADR-0093). Per-variant `*_modal()` (read), `set_*`, `clear_*`,
// and (only where a mutating caller exists) `*_mut`. The `take_*` shape and
// unused `*_mut` shims were removed in the 2026-06-20 rearch sweep; call sites
// that need to consume match on `active_modal` directly.
impl KagiApp {
    #[inline]
    pub fn plan_modal(&self) -> Option<&CheckoutPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Checkout(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn plan_modal_mut(&mut self) -> Option<&mut CheckoutPlanModal> {
        match &mut self.active_modal {
            Some(ActiveModal::Checkout(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_plan_modal(&mut self, m: CheckoutPlanModal) {
        self.active_modal = Some(ActiveModal::Checkout(m));
    }
    #[inline]
    pub fn clear_plan_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Checkout(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn pull_modal(&self) -> Option<&PullPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Pull(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_pull_modal(&mut self, m: PullPlanModal) {
        self.active_modal = Some(ActiveModal::Pull(m));
    }
    #[inline]
    pub fn clear_pull_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Pull(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn undo_modal(&self) -> Option<&UndoPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Undo(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_undo_modal(&mut self, m: UndoPlanModal) {
        self.active_modal = Some(ActiveModal::Undo(m));
    }
    #[inline]
    pub fn clear_undo_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Undo(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn amend_modal(&self) -> Option<&AmendPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Amend(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_amend_modal(&mut self, m: AmendPlanModal) {
        self.active_modal = Some(ActiveModal::Amend(m));
    }
    #[inline]
    pub fn clear_amend_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Amend(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn pop_modal(&self) -> Option<&PopPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Pop(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_pop_modal(&mut self, m: PopPlanModal) {
        self.active_modal = Some(ActiveModal::Pop(m));
    }
    #[inline]
    pub fn clear_pop_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Pop(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn stash_drop_modal(&self) -> Option<&StashDropModal> {
        match &self.active_modal {
            Some(ActiveModal::StashDrop(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_stash_drop_modal(&mut self, m: StashDropModal) {
        self.active_modal = Some(ActiveModal::StashDrop(m));
    }
    #[inline]
    pub fn clear_stash_drop_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::StashDrop(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn push_modal(&self) -> Option<&PushPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Push(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_push_modal(&mut self, m: PushPlanModal) {
        self.active_modal = Some(ActiveModal::Push(m));
    }
    #[inline]
    pub fn clear_push_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Push(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn branch_plan_modal(&self) -> Option<&BranchPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::BranchPlan(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_branch_plan_modal(&mut self, m: BranchPlanModal) {
        self.active_modal = Some(ActiveModal::BranchPlan(m));
    }
    #[inline]
    pub fn clear_branch_plan_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::BranchPlan(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn set_upstream_modal(&self) -> Option<&SetUpstreamModal> {
        match &self.active_modal {
            Some(ActiveModal::SetUpstream(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_upstream_modal_mut(&mut self) -> Option<&mut SetUpstreamModal> {
        match &mut self.active_modal {
            Some(ActiveModal::SetUpstream(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_set_upstream_modal(&mut self, m: SetUpstreamModal) {
        self.active_modal = Some(ActiveModal::SetUpstream(m));
    }
    #[inline]
    pub fn clear_set_upstream_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::SetUpstream(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn rename_branch_modal(&self) -> Option<&RenameBranchModal> {
        match &self.active_modal {
            Some(ActiveModal::RenameBranch(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn rename_branch_modal_mut(&mut self) -> Option<&mut RenameBranchModal> {
        match &mut self.active_modal {
            Some(ActiveModal::RenameBranch(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_rename_branch_modal(&mut self, m: RenameBranchModal) {
        self.active_modal = Some(ActiveModal::RenameBranch(m));
    }
    #[inline]
    pub fn clear_rename_branch_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::RenameBranch(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn merge_modal(&self) -> Option<&MergePlanModal> {
        match &self.active_modal {
            Some(ActiveModal::Merge(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_merge_modal(&mut self, m: MergePlanModal) {
        self.active_modal = Some(ActiveModal::Merge(m));
    }
    #[inline]
    pub fn clear_merge_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Merge(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn tracking_checkout_modal(&self) -> Option<&TrackingCheckoutPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::TrackingCheckout(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_tracking_checkout_modal(&mut self, m: TrackingCheckoutPlanModal) {
        self.active_modal = Some(ActiveModal::TrackingCheckout(m));
    }
    #[inline]
    pub fn clear_tracking_checkout_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::TrackingCheckout(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn switch_to_latest_modal(&self) -> Option<&SwitchToLatestPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::SwitchToLatest(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_switch_to_latest_modal(&mut self, m: SwitchToLatestPlanModal) {
        self.active_modal = Some(ActiveModal::SwitchToLatest(m));
    }
    #[inline]
    pub fn clear_switch_to_latest_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::SwitchToLatest(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn create_branch_modal(&self) -> Option<&CreateBranchModal> {
        match &self.active_modal {
            Some(ActiveModal::CreateBranch(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn create_branch_modal_mut(&mut self) -> Option<&mut CreateBranchModal> {
        match &mut self.active_modal {
            Some(ActiveModal::CreateBranch(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_create_branch_modal(&mut self, m: CreateBranchModal) {
        self.active_modal = Some(ActiveModal::CreateBranch(m));
    }
    #[inline]
    pub fn clear_create_branch_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::CreateBranch(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn create_worktree_modal(&self) -> Option<&CreateWorktreeModal> {
        match &self.active_modal {
            Some(ActiveModal::CreateWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn create_worktree_modal_mut(&mut self) -> Option<&mut CreateWorktreeModal> {
        match &mut self.active_modal {
            Some(ActiveModal::CreateWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_create_worktree_modal(&mut self, m: CreateWorktreeModal) {
        self.active_modal = Some(ActiveModal::CreateWorktree(m));
    }
    #[inline]
    pub fn clear_create_worktree_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::CreateWorktree(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn stash_push_modal(&self) -> Option<&StashPushModal> {
        match &self.active_modal {
            Some(ActiveModal::StashPush(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn stash_push_modal_mut(&mut self) -> Option<&mut StashPushModal> {
        match &mut self.active_modal {
            Some(ActiveModal::StashPush(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_stash_push_modal(&mut self, m: StashPushModal) {
        self.active_modal = Some(ActiveModal::StashPush(m));
    }
    #[inline]
    pub fn clear_stash_push_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::StashPush(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn stash_apply_modal(&self) -> Option<&StashApplyModal> {
        match &self.active_modal {
            Some(ActiveModal::StashApply(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn stash_apply_modal_mut(&mut self) -> Option<&mut StashApplyModal> {
        match &mut self.active_modal {
            Some(ActiveModal::StashApply(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_stash_apply_modal(&mut self, m: StashApplyModal) {
        self.active_modal = Some(ActiveModal::StashApply(m));
    }
    #[inline]
    pub fn clear_stash_apply_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::StashApply(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn cherry_pick_modal(&self) -> Option<&CherryPickModal> {
        match &self.active_modal {
            Some(ActiveModal::CherryPick(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_cherry_pick_modal(&mut self, m: CherryPickModal) {
        self.active_modal = Some(ActiveModal::CherryPick(m));
    }
    #[inline]
    pub fn clear_cherry_pick_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::CherryPick(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn revert_modal(&self) -> Option<&RevertModal> {
        match &self.active_modal {
            Some(ActiveModal::Revert(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_revert_modal(&mut self, m: RevertModal) {
        self.active_modal = Some(ActiveModal::Revert(m));
    }
    #[inline]
    pub fn clear_revert_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Revert(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn history_modal(&self) -> Option<&HistoryPlanModal> {
        match &self.active_modal {
            Some(ActiveModal::History(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_history_modal(&mut self, m: HistoryPlanModal) {
        self.active_modal = Some(ActiveModal::History(m));
    }
    #[inline]
    pub fn clear_history_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::History(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn delete_branch_modal(&self) -> Option<&DeleteBranchModal> {
        match &self.active_modal {
            Some(ActiveModal::DeleteBranch(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_delete_branch_modal(&mut self, m: DeleteBranchModal) {
        self.active_modal = Some(ActiveModal::DeleteBranch(m));
    }
    #[inline]
    pub fn clear_delete_branch_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::DeleteBranch(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn discard_modal(&self) -> Option<&DiscardModal> {
        match &self.active_modal {
            Some(ActiveModal::Discard(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_discard_modal(&mut self, m: DiscardModal) {
        self.active_modal = Some(ActiveModal::Discard(m));
    }
    #[inline]
    pub fn clear_discard_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Discard(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn conflict_continue_modal(&self) -> Option<&ConflictContinuePlanModal> {
        match &self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn conflict_continue_modal_mut(&mut self) -> Option<&mut ConflictContinuePlanModal> {
        match &mut self.active_modal {
            Some(ActiveModal::ConflictContinue(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_conflict_continue_modal(&mut self, m: ConflictContinuePlanModal) {
        self.active_modal = Some(ActiveModal::ConflictContinue(m));
    }
    #[inline]
    pub fn clear_conflict_continue_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ConflictContinue(_))) {
            self.active_modal = None;
        }
    }
}
