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
    /// The root and its renderers share these gates: retained session-owned
    /// menus are not keyboard owners when their pane is not drawn.
    pub(crate) fn commit_menu_visible(&self) -> bool {
        !self.home_in_front() && self.commit_menu.is_some()
    }

    pub(crate) fn branch_menu_visible(&self) -> bool {
        !self.home_in_front() && self.branch_menu.is_some()
    }

    pub(crate) fn stash_menu_visible(&self) -> bool {
        !self.home_in_front() && self.stash_menu.is_some()
    }

    pub(crate) fn tag_menu_visible(&self) -> bool {
        !self.home_in_front() && self.tag_menu.is_some()
    }

    pub(crate) fn worktree_menu_visible(&self) -> bool {
        !self.home_in_front() && self.worktree_menu.is_some()
    }

    pub(crate) fn inspector_file_menu_visible(&self) -> bool {
        !self.home_in_front() && self.inspector_file_menu.is_some()
    }

    pub(crate) fn pr_menu_visible(&self) -> bool {
        !self.home_in_front() && self.ui().pr_menu.is_some()
    }

    pub(crate) fn file_menu_visible(&self, cx: &App) -> bool {
        self.file_menu.as_ref().is_some_and(|menu| {
            !self.home_in_front()
                && self.active_session() == Some(menu.owner)
                && self
                    .ui()
                    .commit_panel
                    .as_ref()
                    .is_some_and(|panel| panel.read(cx).owner == menu.owner)
        })
    }

    pub(crate) fn filter_menu_visible(&self) -> bool {
        !self.home_in_front() && super::list_filter_strip::menu_visible(self)
    }

    pub(crate) fn conflict_file_menu_visible(&self, cx: &App) -> bool {
        !self.home_in_front()
            && !self.ui().conflict_merge_pending
            && self.ui().conflict.as_ref().is_some_and(|entity| {
                let view = entity.read(cx);
                view.file_menu.is_some() && view.mode.is_some()
            })
    }

    pub(crate) fn editor_tree_menu_visible(&self, cx: &App) -> bool {
        !self.home_in_front()
            && self
                .ui()
                .editor_workspace
                .as_ref()
                .is_some_and(|entity| super::editor_tree_menu::menu_visible(entity.read(cx)))
    }

    pub(crate) fn coauthor_menu_visible(&self, cx: &App) -> bool {
        !self.home_in_front()
            && self.ui().commit_panel_open
            && self
                .ui()
                .commit_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).coauthor_menu_visible())
    }

    pub(crate) fn platform_menu_visible(&self) -> bool {
        self.platform_menu_open
            .is_some_and(|ix| super::commands::linux_menu_sections().nth(ix).is_some())
    }

    /// Rendered in the workspace, never on Home.
    pub(crate) fn workspace_menu_visible(&self, cx: &App) -> bool {
        self.commit_menu_visible()
            || self.branch_menu_visible()
            || self.stash_menu_visible()
            || self.tag_menu_visible()
            || self.worktree_menu_visible()
            || self.file_menu_visible(cx)
            || self.inspector_file_menu_visible()
            || self.pr_menu_visible()
            || self.filter_menu_visible()
            || self.conflict_file_menu_visible(cx)
            || self.editor_tree_menu_visible(cx)
            || self.coauthor_menu_visible(cx)
    }

    /// A retained plan behind Home is not an overlay. The workspace renders
    /// menus then menu_overlay; Home renders menu_overlay and the same modal
    /// layer. Platform dropdowns render last in the window shell.
    pub(crate) fn front_layer(&self, cx: &App) -> FrontLayer {
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
        if self.platform_menu_visible()
            || self.menu_overlay.is_some()
            || self.workspace_menu_visible(cx)
        {
            return FrontLayer::Menu;
        }
        FrontLayer::None
    }

    /// Clear every menu, including a stale invisible one retained on this tab.
    /// Escape is not an action on a repository, so there is no reason to leave
    /// a second menu behind the first one.
    pub(crate) fn close_front_menu(&mut self, cx: &mut gpui::Context<Self>) {
        self.platform_menu_open = None;
        self.menu_overlay = None;
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = None;
        self.tag_menu = None;
        self.worktree_menu = None;
        self.file_menu = None;
        self.inspector_file_menu = None;
        self.with_ui(|ui| {
            ui.pr_menu = None;
            ui.filter_controls.menu = None;
        });
        if let Some(entity) = self.ui().conflict.clone() {
            entity.update(cx, |view, cx| {
                view.file_menu = None;
                cx.notify();
            });
        }
        if let Some(entity) = self.ui().editor_workspace.clone() {
            entity.update(cx, |view, cx| view.close_tree_menu(cx));
        }
        self.close_coauthor_menu(cx);
        cx.notify();
    }
}
