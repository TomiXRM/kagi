//! Keyboard ownership follows the render order of overlays, bottom to top.

use gpui::App;

use super::commands::MenuOverlay;
use super::KagiApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrontLayer {
    Modal,
    CommitPlan,
    Settings,
    Menu,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LayerKind {
    ConflictFileMenu,
    EditorTreeMenu,
    CoauthorMenu,
    WorkspaceMenus,
    MenuOverlay,
    EarlyModal,
    PrMenu,
    FilterMenu,
    InspectorFileMenu,
    FileMenu,
    CommitPlan,
    SmartCommit,
    Update,
    PlatformMenu,
}

/// Rendering order (bottom → top). Coauthor lives in the body
/// (`commit_panel_render.rs:1359`); the other pane and workspace menus,
/// followed by MenuOverlay, are in `render.rs:725-738`.
/// Home draws MenuOverlay then the modal slice (`home.rs:213-231`).
/// `render_overlay.rs:311-680` iterates that slice; the shell appends the
/// platform dropdown (`render.rs:753-754`, `mod.rs:3522-3537` on Home).
pub(crate) const Z_ORDER: [LayerKind; 14] = [
    LayerKind::ConflictFileMenu,  // render.rs:727
    LayerKind::EditorTreeMenu,    // render.rs:728
    LayerKind::CoauthorMenu,      // commit_panel_render.rs:1359
    LayerKind::WorkspaceMenus,    // render.rs:729-736
    LayerKind::MenuOverlay,       // render.rs:737-738; home.rs:213-221
    LayerKind::EarlyModal,        // render_overlay.rs:313-612
    LayerKind::PrMenu,            // render_overlay.rs:613-620
    LayerKind::FilterMenu,        // render_overlay.rs:621-623
    LayerKind::InspectorFileMenu, // render_overlay.rs:624-637
    LayerKind::FileMenu,          // render_overlay.rs:638-642
    LayerKind::CommitPlan,        // render_overlay.rs:643-651
    LayerKind::SmartCommit,       // render_overlay.rs:652-656
    LayerKind::Update,            // render_overlay.rs:657-673
    LayerKind::PlatformMenu,      // render.rs:753-754; mod.rs:3536
];

impl LayerKind {
    pub(crate) const fn in_modal_layer(self) -> bool {
        matches!(
            self,
            Self::EarlyModal
                | Self::PrMenu
                | Self::FilterMenu
                | Self::InspectorFileMenu
                | Self::FileMenu
                | Self::CommitPlan
                | Self::SmartCommit
                | Self::Update
        )
    }

    fn visible(self, app: &KagiApp, cx: &App) -> bool {
        match self {
            Self::ConflictFileMenu => app.conflict_file_menu_visible(cx),
            Self::EditorTreeMenu => app.editor_tree_menu_visible(cx),
            Self::CoauthorMenu => app.coauthor_menu_visible(cx),
            Self::WorkspaceMenus => {
                app.commit_menu_visible()
                    || app.branch_menu_visible()
                    || app.stash_menu_visible()
                    || app.tag_menu_visible()
                    || app.worktree_menu_visible()
            }
            Self::MenuOverlay => app.menu_overlay.is_some(),
            Self::EarlyModal => app.early_modal_visible(),
            Self::PrMenu => app.pr_menu_visible(),
            Self::FilterMenu => app.filter_menu_visible(),
            Self::InspectorFileMenu => app.inspector_file_menu_visible(),
            Self::FileMenu => app.file_menu_visible(cx),
            Self::CommitPlan => app.commit_plan_visible(cx),
            Self::SmartCommit => app.smart_commit_modal().is_some(),
            Self::Update => app.update_modal().is_some(),
            Self::PlatformMenu => app.visible_platform_menu_section().is_some(),
        }
    }

    fn front_layer(self, app: &KagiApp) -> FrontLayer {
        match self {
            Self::MenuOverlay if matches!(app.menu_overlay, Some(MenuOverlay::Settings)) => {
                FrontLayer::Settings
            }
            Self::EarlyModal | Self::SmartCommit | Self::Update => FrontLayer::Modal,
            Self::CommitPlan => FrontLayer::CommitPlan,
            Self::ConflictFileMenu
            | Self::EditorTreeMenu
            | Self::CoauthorMenu
            | Self::WorkspaceMenus
            | Self::MenuOverlay
            | Self::PrMenu
            | Self::FilterMenu
            | Self::InspectorFileMenu
            | Self::FileMenu
            | Self::PlatformMenu => FrontLayer::Menu,
        }
    }
}

impl KagiApp {
    /// The root and its renderers share these gates: retained session-owned
    /// menus are not keyboard owners when their pane is not drawn.
    pub(crate) fn commit_menu_visible(&self) -> bool {
        !self.home_in_front()
            && self.commit_menu.as_ref().is_some_and(|menu| {
                self.view().details.get(menu.row_index).is_some()
                    && self.view().rows.get(menu.row_index).is_some()
            })
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

    /// The normal workspace body is on screen: not Home in front, and not
    /// replaced by Conflict Mode's body (`render.rs`, `conflict_body_visible`).
    /// The menus anchored in that body are drawn only with it (#976 review).
    pub(crate) fn workspace_body_drawn(&self) -> bool {
        !self.home_in_front() && !self.conflict_body_visible()
    }

    pub(crate) fn inspector_file_menu_visible(&self) -> bool {
        self.workspace_body_drawn() && self.inspector_file_menu.is_some()
    }

    pub(crate) fn pr_menu_visible(&self) -> bool {
        self.workspace_body_drawn() && self.ui().pr_menu.is_some()
    }

    pub(crate) fn file_menu_visible(&self, cx: &App) -> bool {
        self.file_menu.as_ref().is_some_and(|menu| {
            self.workspace_body_drawn()
                && self.active_session() == Some(menu.owner)
                && self
                    .ui()
                    .commit_panel
                    .as_ref()
                    .is_some_and(|panel| panel.read(cx).owner == menu.owner)
        })
    }

    pub(crate) fn filter_menu_visible(&self) -> bool {
        self.workspace_body_drawn() && super::list_filter_strip::menu_visible(self)
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
        self.workspace_body_drawn()
            && self.ui().commit_panel_open
            && self
                .ui()
                .commit_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).coauthor_menu_visible())
            && super::workspace::resolve_workspace(&self.workspace_inputs(true, true, false)).right
                == super::workspace::RightPane::CommitPanel
    }

    pub(crate) fn visible_platform_menu_section(
        &self,
    ) -> Option<(usize, &'static super::commands::MenuSection)> {
        let ix = self.platform_menu_open?;
        Some((ix, super::commands::linux_menu_sections().nth(ix)?))
    }
    /// The Commit Panel's plan confirmation, only where it is drawn: not with
    /// Home in front, and only while the panel is open.
    pub(crate) fn commit_plan_visible(&self, cx: &App) -> bool {
        !self.home_in_front()
            && self.ui().commit_panel_open
            && self
                .ui()
                .commit_panel
                .as_ref()
                .is_some_and(|panel| panel.read(cx).state.plan_modal.is_some())
    }

    /// A modal in the slot or a drawn commit plan, whatever is drawn above
    /// it (#976 review). Settings opens behind the modal layer, so it must
    /// not open — and must yield — while either exists, even when a platform
    /// menu dropdown over them is the front layer.
    pub(crate) fn has_modal_or_visible_plan(&self, cx: &App) -> bool {
        self.has_active_modal() || self.commit_plan_visible(cx)
    }

    /// Scan the same bottom-to-top sequence used by the modal renderer.
    pub(crate) fn front_layer(&self, cx: &App) -> FrontLayer {
        Z_ORDER
            .iter()
            .rev()
            .find(|kind| kind.visible(self, cx))
            .map_or(FrontLayer::None, |kind| kind.front_layer(self))
    }

    /// A platform dropdown is above Settings in Z_ORDER. Escape dismisses
    /// only that dropdown, then a second Escape can close Settings.
    pub(crate) fn close_front_menu(&mut self, cx: &mut gpui::Context<Self>) {
        if self.visible_platform_menu_section().is_some() {
            self.close_platform_menu(cx);
            return;
        }
        // Other menus still clear together, including stale invisible menus.
        for kind in Z_ORDER {
            self.close_menu_layer(kind, cx);
        }
        cx.notify();
    }

    /// Close just the platform dropdown; Update is its predecessor in Z_ORDER.
    pub(crate) fn close_platform_menu(&mut self, cx: &mut gpui::Context<Self>) {
        self.close_layers_above(LayerKind::Update, cx);
    }

    /// Close every menu drawn above `kind` in [`Z_ORDER`], so a layer that is
    /// about to take the keyboard (Settings, opened into `MenuOverlay`) is
    /// not left underneath a menu that keeps the front (#976 review). Modal
    /// layers are not closed here: they block Settings' admission instead.
    pub(crate) fn close_layers_above(&mut self, kind: LayerKind, cx: &mut gpui::Context<Self>) {
        let Some(at) = Z_ORDER.iter().position(|layer| *layer == kind) else {
            return;
        };
        for layer in &Z_ORDER[at + 1..] {
            self.close_menu_layer(*layer, cx);
        }
        cx.notify();
    }

    /// Is a layer drawn above the workspace context menus that leaves the
    /// keyboard focus where it was — a modal, a notice, a commit plan,
    /// another menu, an Info panel or the branch picker? Settings and the
    /// command palette are not: they move the focus into themselves (a trap,
    /// the search input) and give it back when they close (#991 review).
    pub(crate) fn workspace_menus_covered(&self, cx: &App) -> bool {
        let Some(at) = Z_ORDER
            .iter()
            .position(|layer| *layer == LayerKind::WorkspaceMenus)
        else {
            return false;
        };
        let takes_focus = matches!(
            self.menu_overlay,
            Some(MenuOverlay::Settings | MenuOverlay::CommandPalette)
        );
        Z_ORDER[at + 1..].iter().any(|layer| {
            layer.visible(self, cx) && !(*layer == LayerKind::MenuOverlay && takes_focus)
        })
    }

    /// The one closer table: how each menu layer is closed. Modal layers
    /// have no closer here (they are answered or cancelled through their own
    /// confirm / cancel paths).
    fn close_menu_layer(&mut self, kind: LayerKind, cx: &mut gpui::Context<Self>) {
        match kind {
            LayerKind::ConflictFileMenu => {
                if let Some(entity) = self.ui().conflict.clone() {
                    entity.update(cx, |view, cx| {
                        view.file_menu = None;
                        cx.notify();
                    });
                }
            }
            LayerKind::EditorTreeMenu => {
                if let Some(entity) = self.ui().editor_workspace.clone() {
                    entity.update(cx, |view, cx| view.close_tree_menu(cx));
                }
            }
            LayerKind::CoauthorMenu => {
                self.close_coauthor_menu(cx);
            }
            LayerKind::WorkspaceMenus => {
                self.commit_menu = None;
                self.branch_menu = None;
                self.stash_menu = None;
                self.tag_menu = None;
                self.worktree_menu = None;
            }
            LayerKind::MenuOverlay => self.menu_overlay = None,
            LayerKind::PrMenu => self.with_ui(|ui| ui.pr_menu = None),
            LayerKind::FilterMenu => self.with_ui(|ui| ui.filter_controls.menu = None),
            LayerKind::InspectorFileMenu => self.inspector_file_menu = None,
            LayerKind::FileMenu => self.file_menu = None,
            LayerKind::PlatformMenu => self.platform_menu_open = None,
            LayerKind::EarlyModal
            | LayerKind::CommitPlan
            | LayerKind::SmartCommit
            | LayerKind::Update => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayerKind as L, Z_ORDER};

    #[test]
    fn every_layer_occurs_exactly_once() {
        // Exhaustive list: a new enum variant must also be classified here.
        let all = [
            L::ConflictFileMenu,
            L::EditorTreeMenu,
            L::CoauthorMenu,
            L::WorkspaceMenus,
            L::MenuOverlay,
            L::EarlyModal,
            L::PrMenu,
            L::FilterMenu,
            L::InspectorFileMenu,
            L::FileMenu,
            L::CommitPlan,
            L::SmartCommit,
            L::Update,
            L::PlatformMenu,
        ];
        for kind in all {
            let _: () = match kind {
                L::ConflictFileMenu
                | L::EditorTreeMenu
                | L::CoauthorMenu
                | L::WorkspaceMenus
                | L::MenuOverlay
                | L::EarlyModal
                | L::PrMenu
                | L::FilterMenu
                | L::InspectorFileMenu
                | L::FileMenu
                | L::CommitPlan
                | L::SmartCommit
                | L::Update
                | L::PlatformMenu => (),
            };
            assert_eq!(
                Z_ORDER.iter().filter(|&&entry| entry == kind).count(),
                1,
                "{kind:?}"
            );
        }
        assert_eq!(Z_ORDER.len(), all.len());
        assert_eq!(Z_ORDER.iter().position(|k| *k == L::MenuOverlay), Some(4));
        assert_eq!(Z_ORDER.iter().position(|k| *k == L::PlatformMenu), Some(13));
        let modal = Z_ORDER.iter().filter(|k| k.in_modal_layer()).count();
        assert_eq!(modal, 8);
    }
}
