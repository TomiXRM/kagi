//! Worktree context menu model and overlay renderer.
//!
//! Right-clicking a linked worktree in the sidebar opens this menu. Its single
//! action today is **Unlock worktree…** (enabled only while the worktree is
//! locked); the main worktree never opens the menu.

use gpui::{Context, Pixels, Point, SharedString, Window};

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    i18n::Msg,
    menu_overlay, KagiApp,
};

const MENU_W: f32 = 260.0;

/// State for the open worktree context menu.
#[derive(Clone, Debug)]
pub struct WorktreeMenuState {
    /// Worktree registry name (`git worktree list` identifier).
    pub name: String,
    /// Whether the worktree is currently locked.
    pub locked: bool,
    pub position: Point<Pixels>,
}

/// Actions available on a linked worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeAction {
    Unlock,
}

/// Build the worktree menu groups. Unlock is disabled (with a reason) when the
/// worktree has no lock.
pub fn build_worktree_menu(locked: bool) -> Vec<MenuGroup<WorktreeAction>> {
    let state = if locked {
        ItemState::Enabled
    } else {
        ItemState::Disabled(SharedString::from(Msg::MenuWorktreeNotLocked.t()))
    };
    vec![MenuGroup {
        title: None,
        items: vec![MenuItem {
            action: WorktreeAction::Unlock,
            label: SharedString::from(Msg::MenuUnlockWorktree.t()),
            state,
            dangerous: false,
        }],
    }]
}

pub fn render_worktree_menu_overlay(
    state: WorktreeMenuState,
    header: SharedString,
    groups: Vec<MenuGroup<WorktreeAction>>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let position = state.position;
    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.worktree_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: WorktreeAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.worktree_menu = None;
        this.dispatch_worktree_action(action, state.clone(), window, cx);
    };
    menu_overlay::render_menu_overlay(
        "worktree-context-menu",
        "worktree-menu-item",
        MENU_W,
        "Danger",
        position,
        header,
        groups,
        on_dismiss,
        on_select,
        window,
        cx,
    )
}
