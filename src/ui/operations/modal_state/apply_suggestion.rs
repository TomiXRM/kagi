//! Accessors for the apply-suggestion confirmation (#351).
use crate::ui::modals::{apply_suggestion::ApplySuggestionModal, ActiveModal};
use crate::ui::KagiApp;

impl KagiApp {
    #[inline]
    pub fn apply_suggestion_modal(&self) -> Option<&ApplySuggestionModal> {
        match &self.active_modal {
            Some(ActiveModal::ApplySuggestion(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_apply_suggestion_modal(&mut self, m: ApplySuggestionModal) {
        self.replace_modal_from_user(ActiveModal::ApplySuggestion(m));
    }
    #[inline]
    pub fn clear_apply_suggestion_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::ApplySuggestion(_))) {
            self.active_modal = None;
        }
    }
}
