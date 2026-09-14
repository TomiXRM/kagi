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
        self.replace_active_modal(ActiveModal::SmartCommit(modal));
    }

    #[inline]
    pub fn clear_smart_commit_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::SmartCommit(_))) {
            self.active_modal = None;
        }
    }
}
