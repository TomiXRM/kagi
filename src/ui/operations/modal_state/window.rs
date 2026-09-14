//! Window-global variants in the shared modal slot (#643 / ADR-0197 S3a).

use super::super::super::modals::{ActiveModal, UpdateModal};
use super::super::super::remote_browse::RemoteBrowseModal;
use super::super::super::KagiApp;

impl KagiApp {
    #[inline]
    pub fn remote_browse(&self) -> Option<&RemoteBrowseModal> {
        match &self.active_modal {
            Some(ActiveModal::RemoteBrowse(modal)) => Some(modal),
            _ => None,
        }
    }

    #[inline]
    pub fn remote_browse_mut(&mut self) -> Option<&mut RemoteBrowseModal> {
        match &mut self.active_modal {
            Some(ActiveModal::RemoteBrowse(modal)) => Some(modal),
            _ => None,
        }
    }

    pub(crate) fn remote_browse_generation_is(&self, expected: u64) -> bool {
        self.remote_browse()
            .is_some_and(|modal| modal.generation == expected)
    }

    pub(crate) fn update_remote_browse_from_async(
        &mut self,
        expected: u64,
        update: impl FnOnce(&mut RemoteBrowseModal),
    ) -> bool {
        self.update_expected_modal(|active| match active {
            ActiveModal::RemoteBrowse(modal) if modal.generation == expected => {
                update(modal);
                Some(())
            }
            _ => None,
        })
        .is_some()
    }

    #[inline]
    pub fn set_remote_browse(&mut self, modal: RemoteBrowseModal) {
        self.replace_modal_from_user(ActiveModal::RemoteBrowse(modal));
    }

    #[inline]
    pub fn clear_remote_browse(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::RemoteBrowse(_))) {
            self.active_modal = None;
        }
    }

    #[inline]
    pub fn update_modal(&self) -> Option<&UpdateModal> {
        match &self.active_modal {
            Some(ActiveModal::Update(modal)) => Some(modal),
            _ => None,
        }
    }

    #[inline]
    pub fn set_update_modal(&mut self, modal: UpdateModal) {
        self.replace_modal_from_user(ActiveModal::Update(modal));
    }

    #[inline]
    pub fn clear_update_modal(&mut self) {
        if matches!(self.active_modal, Some(ActiveModal::Update(_))) {
            self.active_modal = None;
        }
    }

    pub fn open_update_modal(&mut self) {
        if self.update_available.is_some() {
            self.set_update_modal(UpdateModal::default());
        }
    }

    pub fn cancel_update_modal(&mut self) {
        self.clear_update_modal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::modals::{StashDropModal, TrustRepoModal};

    /// #492 / #643: repository confirmations are dropped on a switch, while
    /// window-global notices, Remote Browse, and Update survive.
    #[test]
    fn window_global_modals_survive_a_repo_switch() {
        assert!(!ActiveModal::AppNotice("done".to_string().into()).is_repo_scoped());
        assert!(!ActiveModal::RemoteBrowse(RemoteBrowseModal::new()).is_repo_scoped());
        assert!(!ActiveModal::Update(UpdateModal::default()).is_repo_scoped());
        assert!(ActiveModal::StashDrop(StashDropModal {
            plan: None,
            error: None,
            stash_index: 0,
        })
        .is_repo_scoped());
        assert!(ActiveModal::TrustRepo(TrustRepoModal {
            repo_path: std::path::PathBuf::from("/tmp/repo"),
        })
        .is_repo_scoped());
    }
}
