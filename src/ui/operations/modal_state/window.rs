//! Window-global variants in the shared modal slot (#643 / ADR-0197 S3a).

use super::super::super::modals::{ActiveModal, UpdateModal};
use super::super::super::remote_browse::{RemoteBrowseModal, RemoteBrowseStage};
use super::super::super::KagiApp;
use gpui::{AppContext as _, Context, Focusable as _, Window};
use gpui_component::input::InputState;

impl KagiApp {
    /// ADR-0197 決定 1: the two **window-global single slots** that can only
    /// describe the tab on screen, closed when that tab is left. Everything else
    /// a tab shows is owned by its session and survives the switch.
    ///
    /// - The plan slot (`Sessions::{revision, state, plan_owner}`) is one per
    ///   window, not per session: a plan (or a planning request) started in A
    ///   must not stay confirmable once B is on screen.
    /// - A repo-scoped confirmation in `active_modal` carries no owner; parking
    ///   it would let a confirmation planned in A be applied to B (#492).
    pub(crate) fn close_window_slots_of_departing_tab(&mut self) {
        self.sidebar.swipe.cancel();
        self.app_sessions.invalidate_plan();
        self.drop_repo_scoped_modal();
    }

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
            self.set_update_modal(UpdateModal);
        }
    }

    pub fn cancel_update_modal(&mut self) {
        self.clear_update_modal();
    }

    pub(super) fn sync_remote_browse_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.remote_browse_mut() else {
            return;
        };
        if m.stage == RemoteBrowseStage::Browse {
            // #755: Connect's retained inputs are no longer drawn. Only reclaim
            // their focus; a rejected/stale completion must not steal another's.
            let input_focused = [&m.host_state, &m.port_state, &m.identity_state]
                .into_iter()
                .flatten()
                .any(|input| input.read(cx).focus_handle(cx).is_focused(window));
            if input_focused {
                if let Some(root) = self.root_focus.as_ref() {
                    window.focus(root, cx);
                }
            }
            return;
        }
        if m.host_state.is_none() {
            let st = cx.new(|cx| InputState::new(window, cx).placeholder("user@host"));
            st.update(cx, |s, cx| s.focus(window, cx));
            m.host_state = Some(st);
        }
        if m.port_state.is_none() {
            m.port_state =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("22 (optional)")));
        }
        if m.identity_state.is_none() {
            m.identity_state =
                Some(cx.new(|cx| {
                    InputState::new(window, cx).placeholder("~/.ssh/id_ed25519 (optional)")
                }));
        }
        let hv = m
            .host_state
            .as_ref()
            .map(|st| st.read(cx).value().to_string())
            .unwrap_or_default();
        if hv != m.host_input {
            m.host_input = hv;
            m.error = None;
        }
        let pv = m
            .port_state
            .as_ref()
            .map(|st| st.read(cx).value().to_string())
            .unwrap_or_default();
        if pv != m.port_input {
            m.port_input = pv;
        }
        let iv = m
            .identity_state
            .as_ref()
            .map(|st| st.read(cx).value().to_string())
            .unwrap_or_default();
        if iv != m.identity_input {
            m.identity_input = iv;
        }
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
            stash_index: Some(0),
        })
        .is_repo_scoped());
        assert!(ActiveModal::TrustRepo(TrustRepoModal {
            repo_path: std::path::PathBuf::from("/tmp/repo"),
        })
        .is_repo_scoped());
    }
}
