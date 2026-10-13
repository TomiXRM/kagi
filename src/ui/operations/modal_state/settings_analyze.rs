//! Settings-owned alert accessors for the single modal slot.
use super::super::super::{modals::ActiveModal, settings_analyze::AnalyzeResetModal, KagiApp};
impl KagiApp {
    pub(crate) fn analyze_reset_modal(&self) -> Option<&AnalyzeResetModal> {
        match &self.active_modal {
            Some(ActiveModal::AnalyzeReset(modal)) => Some(modal),
            _ => None,
        }
    }
    pub(crate) fn set_analyze_reset_modal(&mut self, modal: AnalyzeResetModal) {
        self.replace_modal_from_user(ActiveModal::AnalyzeReset(modal));
    }
    pub(crate) fn clear_analyze_reset_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::AnalyzeReset(_))) {
            self.active_modal = None;
        }
    }
}
