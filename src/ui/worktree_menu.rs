//! Worktree context menu model and overlay renderer.
//!
//! Right-clicking a linked worktree in the sidebar opens this menu. Its single
//! action today is **Unlock worktree…** (enabled only while the worktree is
//! locked); the main worktree never opens the menu.

use std::path::{Path, PathBuf};

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
    /// #473: the worktree's working-tree path, when the menu was opened from a
    /// place that knows it (the graph's WIP row). `None` from the sidebar, which
    /// only carries the registry name — the path-based items are then omitted.
    pub path: Option<PathBuf>,
    pub position: Point<Pixels>,
}

/// Actions available on a linked worktree (issue #340).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorktreeAction {
    Unlock,
    /// Remove the worktree; `delete_branch` also deletes its branch.
    Remove {
        delete_branch: bool,
    },
    Lock,
    /// Repo-wide: prune stale worktree admin entries.
    Prune,
    /// Repo-wide: repair broken worktree `.git` links.
    Repair,
    /// #473: open this worktree as its own tab — the old WIP-row click, demoted
    /// to a menu item now that the click shows the worktree's changes in place.
    OpenInNewTab,
    /// #473: reveal the worktree directory in the platform file manager.
    Reveal,
    /// #473: copy the worktree's absolute path to the clipboard.
    CopyPath,
}

/// Build the worktree menu groups. Unlock is enabled only while locked, Lock
/// only while unlocked; the two remove variants and the repo-wide prune/repair
/// route through their plan → confirm modals.
///
/// #473: when `path` is known (the graph's WIP row), a leading group offers
/// "Open in new tab" / "Reveal" / "Copy path". The sidebar passes `None` and
/// gets exactly the menu it had before.
pub fn build_worktree_menu(locked: bool, path: Option<&Path>) -> Vec<MenuGroup<WorktreeAction>> {
    let unlock_state = if locked {
        ItemState::Enabled
    } else {
        ItemState::Disabled(SharedString::from(Msg::MenuWorktreeNotLocked.t()))
    };
    let lock_state = if locked {
        ItemState::Disabled(SharedString::from(Msg::MenuWorktreeAlreadyLocked.t()))
    } else {
        ItemState::Enabled
    };
    let mut groups = Vec::new();
    if path.is_some() {
        groups.push(MenuGroup {
            title: None,
            items: vec![
                MenuItem {
                    action: WorktreeAction::OpenInNewTab,
                    label: SharedString::from(Msg::MenuOpenWorktreeInNewTab.t()),
                    state: ItemState::Enabled,
                    dangerous: false,
                },
                MenuItem {
                    action: WorktreeAction::Reveal,
                    label: SharedString::from(if cfg!(target_os = "macos") {
                        Msg::EditorTreeRevealFinder.t()
                    } else {
                        Msg::EditorTreeRevealFile.t()
                    }),
                    state: ItemState::Enabled,
                    dangerous: false,
                },
                MenuItem {
                    action: WorktreeAction::CopyPath,
                    label: SharedString::from(Msg::EditorTreeCopyPath.t()),
                    state: ItemState::Enabled,
                    dangerous: false,
                },
            ],
        });
    }
    groups.extend([
        MenuGroup {
            title: None,
            items: vec![
                MenuItem {
                    action: WorktreeAction::Remove {
                        delete_branch: false,
                    },
                    label: SharedString::from(Msg::MenuRemoveWorktreeKeepBranch.t()),
                    state: ItemState::Enabled,
                    dangerous: true,
                },
                MenuItem {
                    action: WorktreeAction::Remove {
                        delete_branch: true,
                    },
                    label: SharedString::from(Msg::MenuRemoveWorktreeAndBranch.t()),
                    state: ItemState::Enabled,
                    dangerous: true,
                },
            ],
        },
        MenuGroup {
            title: None,
            items: vec![
                MenuItem {
                    action: WorktreeAction::Lock,
                    label: SharedString::from(Msg::MenuLockWorktree.t()),
                    state: lock_state,
                    dangerous: false,
                },
                MenuItem {
                    action: WorktreeAction::Unlock,
                    label: SharedString::from(Msg::MenuUnlockWorktree.t()),
                    state: unlock_state,
                    dangerous: false,
                },
            ],
        },
        MenuGroup {
            title: None,
            items: vec![
                MenuItem {
                    action: WorktreeAction::Prune,
                    label: SharedString::from(Msg::MenuPruneWorktrees.t()),
                    state: ItemState::Enabled,
                    dangerous: false,
                },
                MenuItem {
                    action: WorktreeAction::Repair,
                    label: SharedString::from(Msg::MenuRepairWorktrees.t()),
                    state: ItemState::Enabled,
                    dangerous: false,
                },
            ],
        },
    ]);
    groups
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
