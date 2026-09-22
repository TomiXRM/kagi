//! Typed worktree transitions within the existing ActiveModal owner.
use crate::ui::modals::{worktree::*, ActiveModal};
use crate::ui::KagiApp;
use gpui::{AppContext as _, Context, Window};
use gpui_component::input::InputState;

impl KagiApp {
    pub fn worktree_lock_reason_modal(&self) -> Option<&WorktreeLockReasonModal> {
        match &self.active_modal {
            Some(ActiveModal::WorktreeLockReason(modal)) => Some(modal),
            _ => None,
        }
    }

    pub fn worktree_lock_reason_modal_mut(&mut self) -> Option<&mut WorktreeLockReasonModal> {
        match &mut self.active_modal {
            Some(ActiveModal::WorktreeLockReason(modal)) => Some(modal),
            _ => None,
        }
    }

    pub fn set_worktree_lock_reason_modal(&mut self, modal: WorktreeLockReasonModal) {
        self.replace_modal_from_user(ActiveModal::WorktreeLockReason(modal));
    }

    pub fn clear_worktree_lock_reason_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::WorktreeLockReason(_))) {
            self.active_modal = None;
        }
    }

    pub(super) fn sync_worktree_lock_reason_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(modal) = self.worktree_lock_reason_modal_mut() else {
            return;
        };
        if modal.input_state.is_none() {
            let initial = modal.reason.clone();
            let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
            input.update(cx, |input, cx| input.focus(window, cx));
            modal.input_state = Some(input);
        }
        if let Some(input) = &modal.input_state {
            let reason = input.read(cx).value().to_string();
            if modal.reason != reason {
                modal.reason = reason;
                modal.error = None;
            }
        }
    }

    #[inline]
    pub fn unlock_worktree_modal(&self) -> Option<&UnlockWorktreeModal> {
        match &self.active_modal {
            Some(ActiveModal::UnlockWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_unlock_worktree_modal(&mut self, m: UnlockWorktreeModal) {
        self.replace_modal_from_user(ActiveModal::UnlockWorktree(m));
    }
    #[inline]
    pub fn clear_unlock_worktree_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::UnlockWorktree(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn unlock_worktree_modal_mut(&mut self) -> Option<&mut UnlockWorktreeModal> {
        match &mut self.active_modal {
            Some(ActiveModal::UnlockWorktree(m)) => Some(m),
            _ => None,
        }
    }
    // ── issue #340: remove / lock / prune / repair worktree modals ──
    #[inline]
    pub fn remove_worktree_modal(&self) -> Option<&RemoveWorktreeModal> {
        match &self.active_modal {
            Some(ActiveModal::RemoveWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_remove_worktree_modal(&mut self, m: RemoveWorktreeModal) {
        self.replace_modal_from_user(ActiveModal::RemoveWorktree(m));
    }
    #[inline]
    pub fn clear_remove_worktree_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::RemoveWorktree(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn remove_worktree_modal_mut(&mut self) -> Option<&mut RemoveWorktreeModal> {
        match &mut self.active_modal {
            Some(ActiveModal::RemoveWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn lock_worktree_modal(&self) -> Option<&LockWorktreeModal> {
        match &self.active_modal {
            Some(ActiveModal::LockWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_lock_worktree_modal(&mut self, m: LockWorktreeModal) {
        self.replace_modal_from_user(ActiveModal::LockWorktree(m));
    }
    #[inline]
    pub fn clear_lock_worktree_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::LockWorktree(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn lock_worktree_modal_mut(&mut self) -> Option<&mut LockWorktreeModal> {
        match &mut self.active_modal {
            Some(ActiveModal::LockWorktree(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn prune_worktrees_modal(&self) -> Option<&PruneWorktreesModal> {
        match &self.active_modal {
            Some(ActiveModal::PruneWorktrees(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_prune_worktrees_modal(&mut self, m: PruneWorktreesModal) {
        self.replace_modal_from_user(ActiveModal::PruneWorktrees(m));
    }
    #[inline]
    pub fn clear_prune_worktrees_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::PruneWorktrees(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn prune_worktrees_modal_mut(&mut self) -> Option<&mut PruneWorktreesModal> {
        match &mut self.active_modal {
            Some(ActiveModal::PruneWorktrees(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn repair_worktrees_modal(&self) -> Option<&RepairWorktreesModal> {
        match &self.active_modal {
            Some(ActiveModal::RepairWorktrees(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_repair_worktrees_modal(&mut self, m: RepairWorktreesModal) {
        self.replace_modal_from_user(ActiveModal::RepairWorktrees(m));
    }
    #[inline]
    pub fn clear_repair_worktrees_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::RepairWorktrees(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn repair_worktrees_modal_mut(&mut self) -> Option<&mut RepairWorktreesModal> {
        match &mut self.active_modal {
            Some(ActiveModal::RepairWorktrees(m)) => Some(m),
            _ => None,
        }
    }
}
