use super::{ActiveModal, KagiApp};
use crate::ui::smart_commit::SmartCommitModal;

impl KagiApp {
    #[inline]
    pub fn smart_commit_modal(&self) -> Option<&SmartCommitModal> {
        match &self.active_modal {
            Some(ActiveModal::SmartCommit(modal)) => Some(modal),
            _ => None,
        }
    }

    #[inline]
    pub fn set_smart_commit_modal(&mut self, modal: SmartCommitModal) {
        // Preserve the initiating owner when consent advances to the picker.
        if self.smart_commit_modal().is_none() {
            self.smart_model_focus.owner = self.active_session();
        }
        if let SmartCommitModal::ModelPicker { models } = &modal {
            self.smart_model_focus.reset(models);
        }
        self.replace_modal_from_user(ActiveModal::SmartCommit(modal));
    }

    #[inline]
    pub fn clear_smart_commit_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::SmartCommit(_))) {
            self.active_modal = None;
            self.smart_model_focus.owner = None;
            self.focus_root_for_modal();
        }
    }
}
