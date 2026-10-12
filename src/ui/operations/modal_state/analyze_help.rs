//! Typed user-control transitions for Analyze's help task (ADR-0093).
use super::super::super::{modals::ActiveModal, KagiApp};
use gpui::{Context, FocusHandle};

impl KagiApp {
    pub fn analyze_help_modal(&self) -> Option<&FocusHandle> {
        match self.active_modal.as_ref() {
            Some(ActiveModal::AnalyzeHelp(opener)) => Some(opener),
            _ => None,
        }
    }
    pub fn set_analyze_help_modal(&mut self, opener: FocusHandle) {
        self.replace_modal_from_user(ActiveModal::AnalyzeHelp(opener));
    }
    pub(crate) fn open_analyze_help_from_control(
        &mut self,
        opener: FocusHandle,
        cx: &mut Context<Self>,
    ) {
        self.modal_focus = Some(cx.focus_handle());
        self.set_analyze_help_modal(opener);
    }
    pub fn clear_analyze_help_modal(&mut self) {
        if let Some(opener) = self.analyze_help_modal().cloned() {
            self.active_modal = None;
            self.restore_control_focus(opener);
        }
    }
}
