//! Tag context menu model and overlay renderer (ADR-0140).
//!
//! Tags were the only sidebar ref with no menu at all: they could be created
//! from a commit's menu and then only looked at. Publishing one meant leaving
//! kagi for a terminal, which is exactly the gap this closes.

use gpui::{Context, Pixels, Point, SharedString, Window};

use super::i18n::Msg;
use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    menu_overlay, KagiApp,
};

const MENU_W: f32 = 260.0;

/// State for the open tag context menu.
#[derive(Clone, Debug)]
pub struct TagMenuState {
    pub name: String,
    pub position: Point<Pixels>,
    /// The remote a push would target, resolved when the menu opened so the
    /// label can name it. `None` when the repository has no remote — the Push
    /// item is then shown disabled with the reason rather than hidden, so the
    /// action is still discoverable.
    pub remote: Option<String>,
}

/// Actions available on a tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagAction {
    Push,
    CopyName,
}

/// Build the tag menu groups.
pub fn build_tag_menu(remote: Option<&str>) -> Vec<MenuGroup<TagAction>> {
    let push_label = match remote {
        Some(r) => Msg::TagPushTo.t().replace("{}", r),
        None => Msg::TagPush.t().to_string(),
    };
    vec![MenuGroup {
        title: None,
        items: vec![
            MenuItem {
                action: TagAction::Push,
                label: SharedString::from(push_label),
                state: match remote {
                    Some(_) => ItemState::Enabled,
                    None => ItemState::Disabled(Msg::TagNoRemote.t().into()),
                },
                dangerous: false,
            },
            MenuItem {
                action: TagAction::CopyName,
                label: SharedString::from(Msg::TagCopyName.t()),
                state: ItemState::Enabled,
                dangerous: false,
            },
        ],
    }]
}

pub fn render_tag_menu_overlay(
    state: TagMenuState,
    header: SharedString,
    groups: Vec<MenuGroup<TagAction>>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let position = state.position;
    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.tag_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: TagAction,
                          _window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.tag_menu = None;
        this.dispatch_tag_action(action, state.clone(), cx);
    };
    menu_overlay::render_menu_overlay(
        "tag-context-menu",
        "tag-menu-item",
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
