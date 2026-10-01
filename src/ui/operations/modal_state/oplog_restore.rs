//! Accessors for the Operation Log op-revert / restore-to-point card (#334).
use crate::ui::modals::{oplog_restore::OplogRestoreModal, ActiveModal};
use crate::ui::KagiApp;

impl KagiApp {
    #[inline]
    pub fn oplog_restore_modal(&self) -> Option<&OplogRestoreModal> {
        match &self.active_modal {
            Some(ActiveModal::OplogRestore(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_oplog_restore_modal(&mut self, m: OplogRestoreModal) {
        self.replace_modal_from_user(ActiveModal::OplogRestore(m));
    }
    #[inline]
    pub fn clear_oplog_restore_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::OplogRestore(_))) {
            self.active_modal = None;
        }
    }
}
