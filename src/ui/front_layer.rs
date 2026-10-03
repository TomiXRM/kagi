//! The visible keyboard owner, in the same order as the root's overlay renderers.

use gpui::App;

use super::commands::MenuOverlay;
use super::KagiApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontLayer {
    /// A window-global modal in the shared slot (`render_overlay.rs:308-605,647-665`).
    Modal,
    /// The Commit Panel confirmation, only on a drawn workspace panel (`render_overlay.rs:639-646`).
    CommitPlan,
    /// The Settings trap (`render.rs:693-698`, `home.rs:202-221`).
    Settings,
    /// A menu or other popover (`render.rs:681-694`, `render_overlay.rs:607-638`).
    Menu,
    /// No overlay intercepts the workspace or Home.
    None,
}

impl KagiApp {
    /// A retained plan behind Home is not an overlay. The workspace renders
    /// menus then menu_overlay (`render.rs:681-694`), followed by the modal
    /// layer (`render.rs:696-698`); Home renders menu_overlay and the same
    /// modal layer (`home.rs:202-221`). Platform dropdowns render last in
    /// the window shell (`mod.rs:3489-3491`).
    pub(crate) fn front_layer(&self, cx: &App) -> FrontLayer {
        if self.platform_menu_open.is_some() {
            return FrontLayer::Menu;
        }
        if self.has_active_modal() {
            return FrontLayer::Modal;
        }
        if !self.home_in_front()
            && self.ui().commit_panel_open
            && self
                .ui()
                .commit_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).state.plan_modal.is_some())
        {
            return FrontLayer::CommitPlan;
        }
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            return FrontLayer::Settings;
        }
        if self.menu_overlay.is_some() {
            return FrontLayer::Menu;
        }
        if !self.home_in_front()
            && (self.commit_menu.is_some()
                || self.branch_menu.is_some()
                || self.stash_menu.is_some()
                || self.tag_menu.is_some()
                || self.worktree_menu.is_some()
                || self.file_menu.is_some()
                || self.inspector_file_menu.is_some()
                || self.ui().pr_menu.is_some()
                || self.ui().filter_controls.menu.is_some()
                || self
                    .ui()
                    .conflict
                    .as_ref()
                    .is_some_and(|view| view.read(cx).file_menu.is_some())
                || self
                    .ui()
                    .editor_workspace
                    .as_ref()
                    .is_some_and(|view| view.read(cx).tree_menu.is_some())
                || (self.ui().commit_panel_open
                    && self
                        .ui()
                        .commit_panel
                        .as_ref()
                        .is_some_and(|panel| panel.read(cx).coauthor_menu.is_some())))
        {
            return FrontLayer::Menu;
        }
        FrontLayer::None
    }
}
