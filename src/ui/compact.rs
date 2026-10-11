//! Window allocation only: real drafts, staging and selection stay session-owned.
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{div, prelude::*, px, rgb, Bounds, Context, Entity, Pixels, Window};
use gpui_component::{popover::PopoverState, ElementExt};

use super::{
    badges::WipRowClick, commit_panel_surfaces::CommitPanelSurfaces, e2e, i18n::Msg, theme,
    BottomTab, KagiApp,
};
use crate::app::SessionId;

/// Logical viewport width, deliberately independent of density and zoom (ADR-0224).
pub(super) const BREAKPOINT: f32 = 1000.;
pub(super) const RAIL_WIDTH: f32 = 44.;

gpui::actions!(kagi_compact, [OpenCommitComposer]);

pub(super) fn for_window(window: &Window) -> bool {
    compact_width(window.viewport_size().width)
}

fn compact_width(width: Pixels) -> bool {
    width < px(BREAKPOINT)
}

#[derive(Default)]
pub(super) struct CompactState {
    pub enabled: bool,
    pub suspended: bool,
    owner: Option<SessionId>,
    pub popup_owner: Option<SessionId>,
    pub wip_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub wip_hovered: bool,
    surfaces: Option<CommitPanelSurfaces>,
    pub popup_state: Rc<RefCell<Option<Entity<PopoverState>>>>,
    pub preserve_pin: Rc<Cell<bool>>,
}

impl KagiApp {
    pub(super) fn sync_compact_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let enabled = for_window(window);
        let owner = self.active_session();
        if self.compact.owner != owner || self.home_in_front() {
            self.close_compact_composer(window, cx);
            super::compact_sidebar::dismiss(window, cx);
            self.compact.wip_bounds.set(None);
            self.compact.surfaces = None;
            self.compact.owner = owner;
        }
        if self.compact.enabled && !enabled {
            // Graph selection closes the legacy right-pane flag, not this pin.
            // Reattach the same owner/model to its desktop projection on resize.
            if self.compact.popup_owner.is_some()
                && self.compact.popup_owner == owner
                && self.ui().commit_panel.is_some()
            {
                self.with_ui(|ui| ui.commit_panel_open = true);
            }
            self.dismiss_compact_presentation(window, cx, true);
            super::compact_sidebar::dismiss(window, cx);
        }
        self.compact.enabled = enabled;
        self.compact.suspended =
            enabled && (self.has_modal_or_visible_plan(cx) || self.menu_overlay.is_some());
        if self.compact.suspended {
            super::compact_sidebar::dismiss(window, cx);
            self.dismiss_compact_presentation(window, cx, true);
        }
        if self.ui().commit_panel.is_none()
            || self.compact.popup_owner.is_none()
            || self.conflict_body_visible()
        {
            self.close_compact_composer(window, cx);
        }
    }

    /// Close the projection, never its panel entity or text-input owners.
    pub(super) fn close_compact_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.compact.popup_owner = None;
        self.dismiss_compact_presentation(window, cx, false);
    }

    fn dismiss_compact_presentation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        preserve: bool,
    ) {
        let state = self.compact.popup_state.borrow_mut().take();
        if let Some(state) = state {
            use gpui::Focusable as _;
            let focused = state.read(cx).focus_handle(cx).contains_focused(window, cx);
            // The root already owns the pin transition (including explicit
            // close); an old kit callback must not clear a freshly reopened pin.
            self.compact.preserve_pin.set(true);
            state.update(cx, |state, cx| state.dismiss(window, cx));
            self.compact.preserve_pin.set(false);
            if focused && !preserve {
                if let Some(root) = &self.root_focus {
                    root.focus(window, cx);
                }
            }
        }
    }

    pub(super) fn pin_compact_composer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        is_new: bool,
    ) {
        if for_window(window) {
            let owner = self.active_session();
            if is_new || self.compact.owner != owner {
                self.dismiss_compact_presentation(window, cx, true);
                self.compact.owner = owner;
            }
            super::compact_sidebar::dismiss(window, cx);
            self.compact.popup_owner = owner;
            self.bottom_tab = BottomTab::Changes;
            self.bottom_panel_open = true;
        }
    }

    /// Changes activation initializes the existing owner, but does not pin a popup.
    pub(super) fn open_compact_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ui().commit_panel.is_none() {
            self.open_commit_panel(window, cx);
            self.close_compact_composer(window, cx);
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
    ) -> Option<Entity<super::commit_panel_surfaces::CommitPanelSurfaceView>> {
        let panel = self.ui().commit_panel.as_ref()?;
        if self
            .compact
            .surfaces
            .as_ref()
            .is_none_or(|surfaces| surfaces.panel != panel.entity_id())
        {
            self.compact.surfaces = Some(CommitPanelSurfaces::new(panel, cx));
        }
        let surfaces = self.compact.surfaces.as_ref()?;
        Some(if composer {
            surfaces.composer.clone()
        } else {
            surfaces.changes.clone()
        })
    }

    /// Desktop exclusivity stays intact; a pinned composer survives graph selection.
    pub(super) fn commit_panel_surface_open(&self) -> bool {
        self.ui().commit_panel_open
            || (self.compact.enabled
                && self.compact.popup_owner.is_some()
                && self.compact.popup_owner == self.active_session())
    }

    pub(super) fn compact_wip_anchor(
        &self,
        click: &super::badges::WipRowClick,
        cx: &gpui::App,
    ) -> bool {
        match click {
            super::badges::WipRowClick::CommitPanel => self
                .ui()
                .commit_panel
                .as_ref()
                .is_none_or(|panel| panel.read(cx).foreign.is_none()),
            super::badges::WipRowClick::Worktree { path, .. } => self
                .ui()
                .commit_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).repo_path == *path),
        }
    }

    pub(super) fn compact_wip_visible(&self, row: usize) -> bool {
        let list = self.ui().commit_scroll_handle.0.borrow();
        let height = list.base_handle.bounds().size.height;
        let row_height = px(super::row_height(self.graph_compact));
        let top = row_height * row as f32 + list.base_handle.offset().y;
        height <= px(0.) || (top < height && top + row_height > px(0.))
    }

    pub(super) fn render_compact_wip(
        &self,
        mut row: gpui::Stateful<gpui::Div>,
        click: &WipRowClick,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        if !self.compact.enabled {
            return row;
        }
        if self.compact_wip_anchor(click, cx) {
            let measured = self.compact.wip_bounds.clone();
            let parent = cx.entity_id();
            row = row.on_prepaint(move |bounds, window, cx| {
                if measured.replace(Some(bounds)) != Some(bounds) {
                    cx.notify(parent);
                    window.request_animation_frame();
                }
            });
        }
        if matches!(click, WipRowClick::CommitPanel) {
            row = row
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if this.compact.wip_hovered != *hovered {
                        this.compact.wip_hovered = *hovered;
                        cx.notify();
                    }
                }))
                .child(e2e::measure_inside("compact-wip-row"))
                .when(self.compact.wip_hovered, |row| {
                    row.child(
                        div()
                            .absolute()
                            .right(theme::scaled_px(8.))
                            .top_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .px_2()
                            .rounded(theme::scaled_px(4.))
                            .bg(rgb(theme::theme().surface))
                            .text_sm()
                            .text_color(rgb(theme::theme().text_main))
                            .cursor_pointer()
                            .child(Msg::CompactCommitAffordance.t())
                            .child(e2e::measure_inside("compact-wip-affordance")),
                    )
                });
        }
        row
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
