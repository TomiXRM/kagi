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
    CreateBranchModal, CreateWorktreeModal, DeleteBranchModal, DiscardModal, EditorDirtyGuardModal,
    HistoryPlanModal, MergePlanModal, PopPlanModal, PullPlanModal, PushPlanModal,
    RenameBranchModal, RevertModal, SetUpstreamModal, StashApplyModal, StashDropModal,
    StashPushModal, SwitchToLatestPlanModal, TrackingCheckoutPlanModal, UndoPlanModal,
};
use super::super::KagiApp;
use gpui::{AppContext as _, Context, Window};
use gpui_component::input::InputState;

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
    pub fn editor_dirty_guard_modal(&self) -> Option<&EditorDirtyGuardModal> {
        match &self.active_modal {
            Some(ActiveModal::EditorDirtyGuard(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_editor_dirty_guard_modal(&mut self, m: EditorDirtyGuardModal) {
        self.active_modal = Some(ActiveModal::EditorDirtyGuard(m));
    }
    #[inline]
    pub fn clear_editor_dirty_guard_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::EditorDirtyGuard(_))) {
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

// Moved from `src/ui/mod.rs` (T-HOTSPOT-UIMOD-001): lazy InputState creation +
// per-frame string sync for the input-bearing modals.
impl KagiApp {
    /// Lazily create + sync the real text inputs of the create-branch /
    /// create-worktree / stash-push modals (gpui-component `InputState`).
    ///
    /// The old hand-rolled inputs (KeyDown capture + a fake `_` caret) had no
    /// caret, no IME, no click focus and re-planned on every frame
    /// (user-reported). `InputState` needs a `Window`, which open_* callers
    /// (incl. headless) don't all have — so creation happens here, on the
    /// first render after the modal opens, and the modal's plain-`String`
    /// field is kept in sync for the plan/confirm/headless paths.
    pub(crate) fn sync_modal_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ── Create-branch ───────────────────────────────────
        if let Some(m) = self.create_branch_modal_mut() {
            if m.input_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("branch-name"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }

        // ── Remote SSH connect form (host / port / identity) ─
        if let Some(m) = self.remote_browse_modal.as_mut() {
            if m.host_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("user@host"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.host_state = Some(st);
            }
            if m.port_state.is_none() {
                m.port_state =
                    Some(cx.new(|cx| InputState::new(window, cx).placeholder("22 (optional)")));
            }
            if m.identity_state.is_none() {
                m.identity_state = Some(cx.new(|cx| {
                    InputState::new(window, cx).placeholder("~/.ssh/id_ed25519 (optional)")
                }));
            }
            let hv = m
                .host_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if hv != m.host_input {
                m.host_input = hv;
                m.error = None;
            }
            let pv = m
                .port_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if pv != m.port_input {
                m.port_input = pv;
            }
            let iv = m
                .identity_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if iv != m.identity_input {
                m.identity_input = iv;
            }
        }

        // ── Create-worktree (branch + path fields) ──────────
        // Auto-path: while the user has not touched the path field, it
        // follows the branch name (same behaviour as before).
        let mut set_path: Option<String> = None;
        if let Some(m) = self.create_worktree_modal_mut() {
            if m.branch_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("branch-name"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.branch_state = Some(st);
            }
            if m.path_state.is_none() {
                let initial = m.path_input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("worktree path")
                        .default_value(initial)
                });
                m.path_state = Some(st);
            }
            let branch_v = m
                .branch_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            let path_v = m
                .path_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            let mut dirty = false;
            if path_v != m.path_input {
                // Path text differs from what we last wrote → user edit.
                m.path_input = path_v;
                m.path_touched = true;
                dirty = true;
            }
            if branch_v != m.branch_input {
                m.branch_input = branch_v.clone();
                if !m.path_touched {
                    set_path = Some(branch_v);
                }
                dirty = true;
            }
            if dirty {
                m.error = None;
            }
            if dirty && set_path.is_none() {
                self.schedule_modal_replan(cx);
            }
        }
        if let Some(branch) = set_path {
            // Recompute the suggested path outside the &mut borrow.
            let auto = self.default_worktree_path(if branch.is_empty() {
                "new-branch"
            } else {
                &branch
            });
            if let Some(m) = self.create_worktree_modal_mut() {
                m.path_input = auto.clone();
                if let Some(st) = m.path_state.clone() {
                    st.update(cx, |s, cx| s.set_value(auto, window, cx));
                }
            }
            self.schedule_modal_replan(cx);
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        // ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001 (correction #1): moved
        // ONTO the `CommitPanelView` entity (`sync_inputs`), so the parent never
        // reads the child's commit input each frame (the re-entrancy-in-render
        // surface ADR-0118 forbids).

        // ── Stash push (message) ────────────────────────────
        if let Some(m) = self.stash_push_modal_mut() {
            if m.input_state.is_none() {
                let st = cx
                    .new(|cx| InputState::new(window, cx).placeholder("stash message (optional)"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }

        if let Some(m) = self.set_upstream_modal_mut() {
            if m.input_state.is_none() {
                let initial = m.input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("origin/branch")
                        .default_value(initial)
                });
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }

        if let Some(m) = self.rename_branch_modal_mut() {
            if m.input_state.is_none() {
                let initial = m.input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("branch-name")
                        .default_value(initial)
                });
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }

        // ── T-CONFLICT-UI-001/005/UX-015: Conflict Editor code editors ──
        // ADR-0118: the sync logic moved into the entity (it owns `editor_inputs`
        // + `editing` + `mode`). Drive it via `update_in` (it needs a `Window` to
        // create `InputState`). Safe here: this runs on the parent render-sync
        // path, NOT a leased `ConflictView` listener.
        if let Some(entity) = self.conflict.clone() {
            entity.update(cx, |v, cx| v.sync_editor_inputs(window, cx));
        }
    }
}
