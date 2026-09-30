//! Accessors for the repository-health fix confirmation (#358).
use crate::ui::modals::{repo_health::RepoHealthModal, ActiveModal};
use crate::ui::KagiApp;

impl KagiApp {
    #[inline]
    pub fn repo_health_modal(&self) -> Option<&RepoHealthModal> {
        match &self.active_modal {
            Some(ActiveModal::RepoHealth(m)) => Some(m),
            _ => None,
        }
    }
    #[inline]
    pub fn set_repo_health_modal(&mut self, m: RepoHealthModal) {
        self.replace_modal_from_user(ActiveModal::RepoHealth(m));
    }
    #[inline]
    pub fn clear_repo_health_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::RepoHealth(_))) {
            self.active_modal = None;
        }
    }
}
