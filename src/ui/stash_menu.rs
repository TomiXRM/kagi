//! Stash context menu model and overlay renderer (ADR-0087).
//!
//! Sidebar interaction (user request): left-click a stash = **Pop** (consume);
//! right-click opens this menu with **Apply** (keep) and **Drop** (delete).

use gpui::{Context, Pixels, Point, SharedString, Window};

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    menu_overlay, KagiApp,
};

const MENU_W: f32 = 220.0;

/// State for the open stash context menu.
#[derive(Clone, Debug)]
pub struct StashMenuState {
    pub index: usize,
    pub message: String,
    pub position: Point<Pixels>,
}

/// Actions available on a stash entry. Pop is the left-click default; Apply and
/// Drop live in the right-click menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StashAction {
    Pop,
    Apply,
    Drop,
}

fn item(action: StashAction, label: &str, dangerous: bool) -> MenuItem<StashAction> {
    MenuItem {
        action,
        label: SharedString::from(label.to_string()),
        state: ItemState::Enabled,
        dangerous,
    }
}

/// Build the stash menu groups. Pop is included too (discoverability) even
/// though left-click already pops.
pub fn build_stash_menu() -> Vec<MenuGroup<StashAction>> {
    vec![
        MenuGroup {
            title: Some("Restore"),
            items: vec![
                item(StashAction::Pop, "Pop (apply and remove)", false),
                item(StashAction::Apply, "Apply (keep stash)", false),
            ],
        },
        MenuGroup {
            title: Some("Danger"),
            items: vec![item(StashAction::Drop, "Drop (delete stash)", true)],
        },
    ]
}

pub fn render_stash_menu_overlay(
    state: StashMenuState,
    header: SharedString,
    groups: Vec<MenuGroup<StashAction>>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let position = state.position;
    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.stash_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: StashAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.stash_menu = None;
        this.dispatch_stash_action(action, state.clone(), window, cx);
    };
    menu_overlay::render_menu_overlay(
        "stash-context-menu",
        "stash-menu-item",
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
