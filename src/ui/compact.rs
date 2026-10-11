//! Window allocation only; repository reads and navigation remain session-owned.
use gpui::{px, Context, Pixels, Window};

use super::KagiApp;
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
}

impl KagiApp {
    pub(super) fn sync_compact_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let enabled = for_window(window);
        let owner = self.active_session();
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
