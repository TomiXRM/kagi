//! Window allocation only; repository reads and navigation remain session-owned.
use gpui::{px, Context, Entity, Pixels, Window};

use super::{
    commit_panel_surfaces::{CommitPanelSurfaceView, CommitPanelSurfaces},
    BottomTab, KagiApp,
};
use crate::app::SessionId;

/// Logical viewport width, deliberately independent of density and zoom (ADR-0224).
pub(super) const BREAKPOINT: f32 = 1000.;
pub(super) const RAIL_WIDTH: f32 = 44.;

fn compact_width(width: Pixels) -> bool {
    width < px(BREAKPOINT)
}

pub(super) fn for_window(window: &Window) -> bool {
    compact_width(window.viewport_size().width)
}

#[derive(Default)]
pub(super) struct CompactState {
    pub enabled: bool,
    owner: Option<SessionId>,
    surfaces: Option<CommitPanelSurfaces>,
}

impl KagiApp {
    pub(super) fn sync_compact_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let enabled = for_window(window);
        let owner = self.active_session();
        if self.compact.owner != owner || self.home_in_front() {
            self.compact.surfaces = None;
        }
        if self.compact.owner != owner
            || self.home_in_front()
            || (self.compact.enabled && !enabled)
            || self.has_modal_or_visible_plan(cx)
            || self.menu_overlay.is_some()
            || self.conflict_body_visible()
        {
            super::compact_sidebar::dismiss(window, cx);
        }
        self.compact.owner = owner;
        self.compact.enabled = enabled;
    }

    /// Changes selects the original panel's files, not a duplicate draft/model.
    pub(super) fn open_compact_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui().commit_panel.is_none() {
            self.open_commit_panel(window, cx);
        }
        if let Some(ui) = self.ui_mut() {
            ui.commit_panel_open = false;
        }
        self.bottom_tab = BottomTab::Changes;
        self.bottom_panel_open = true;
    }

    pub(super) fn compact_surface(
        &mut self,
        composer: bool,
        cx: &mut Context<Self>,
    ) -> Option<Entity<CommitPanelSurfaceView>> {
        let panel = self.ui().commit_panel.clone()?;
        if self
            .compact
            .surfaces
            .as_ref()
            .is_none_or(|surfaces| surfaces.panel != panel.entity_id())
        {
            self.compact.surfaces = Some(CommitPanelSurfaces::new(&panel, cx));
        }
        let surfaces = self.compact.surfaces.as_ref()?;
        Some(if composer {
            surfaces.composer.clone()
        } else {
            surfaces.changes.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_breakpoint_is_exclusive_and_zoom_independent() {
        assert!(compact_width(px(940.)));
        assert!(compact_width(px(999.)));
        assert!(!compact_width(px(1000.)));
        assert!(!compact_width(px(1392.)));
    }
}
