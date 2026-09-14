//! Editor-pane modal accessors (dirty guard, filesystem prompt, delete
//! confirmation) over the single `active_modal` field.
//!
//! Split out of `modal_state.rs` on a feature boundary (#707 review): that
//! file is the one owner of these accessors and was at its LOC ceiling, so
//! it grows by submodule rather than by pushing accessors into feature
//! files where `active_modal` would be touched directly.

use super::super::super::modals::ActiveModal;
use super::super::super::modals::{
    EditorDeleteConfirmModal, EditorDirtyGuardModal, EditorFsPromptModal,
};
use super::super::super::KagiApp;

impl KagiApp {
    #[inline]
    pub fn editor_dirty_guard_modal(&self) -> Option<&EditorDirtyGuardModal> {
        match &self.active_modal {
            Some(ActiveModal::EditorDirtyGuard(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_editor_dirty_guard_modal(&mut self, m: EditorDirtyGuardModal) {
        self.replace_modal_from_user(ActiveModal::EditorDirtyGuard(m));
    }
    #[inline]
    pub fn clear_editor_dirty_guard_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::EditorDirtyGuard(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn editor_fs_prompt_modal(&self) -> Option<&EditorFsPromptModal> {
        match &self.active_modal {
            Some(ActiveModal::EditorFsPrompt(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn editor_fs_prompt_modal_mut(&mut self) -> Option<&mut EditorFsPromptModal> {
        match &mut self.active_modal {
            Some(ActiveModal::EditorFsPrompt(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_editor_fs_prompt_modal(&mut self, m: EditorFsPromptModal) {
        self.replace_modal_from_user(ActiveModal::EditorFsPrompt(m));
    }
    #[inline]
    pub fn clear_editor_fs_prompt_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::EditorFsPrompt(_))) {
            self.active_modal = None;
        }
    }
    #[inline]
    pub fn editor_delete_confirm_modal(&self) -> Option<&EditorDeleteConfirmModal> {
        match &self.active_modal {
            Some(ActiveModal::EditorDeleteConfirm(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_editor_delete_confirm_modal(&mut self, m: EditorDeleteConfirmModal) {
        self.replace_modal_from_user(ActiveModal::EditorDeleteConfirm(m));
    }
    #[inline]
    pub fn clear_editor_delete_confirm_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::EditorDeleteConfirm(_))) {
            self.active_modal = None;
        }
    }
}
