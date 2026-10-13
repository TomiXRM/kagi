use super::{ActiveModal, KagiApp};
use crate::ui::modals::TerminalPasteModal;

impl KagiApp {
    pub fn terminal_paste_modal(&self) -> Option<&TerminalPasteModal> {
        match &self.active_modal {
            Some(ActiveModal::TerminalPaste(modal)) => Some(modal),
            _ => None,
        }
    }

    pub(crate) fn set_terminal_paste_modal(&mut self, modal: TerminalPasteModal) {
        self.replace_modal_from_user(ActiveModal::TerminalPaste(modal));
    }

    pub(crate) fn clear_terminal_paste_modal(&mut self) {
        if self.terminal_paste_modal().is_some() {
            self.active_modal = None;
        }
    }
}
