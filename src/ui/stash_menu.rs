//! Stash context menu model and overlay renderer (ADR-0087).
//!
//! Sidebar interaction (user request): left-click a stash = **Pop** (consume);
//! right-click opens this menu with **Apply** (keep) and **Drop** (delete).

use gpui::{
    div, prelude::*, px, rgb, Context, IntoElement, MouseButton, Pixels, Point, SharedString,
    Window,
};

use super::{
    context_menu::{ItemState, MenuGroup, MenuItem},
    theme::{self, theme},
    KagiApp,
};

const MENU_W: f32 = 220.0;
const MENU_MARGIN: f32 = 8.0;
const MENU_ROW_H: f32 = 24.0;
const MENU_HEADER_H: f32 = 30.0;
const MENU_GROUP_H: f32 = 18.0;

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
    let viewport = window.viewport_size();
    let visible_items = groups
        .iter()
        .flat_map(|g| g.items.iter())
        .filter(|i| i.state != ItemState::Hidden)
        .count() as f32;
    let visible_groups = groups
        .iter()
        .filter(|g| g.items.iter().any(|i| i.state != ItemState::Hidden))
        .count() as f32;
    let z = theme::zoom();
    let menu_w = MENU_W * z;
    let menu_h =
        (MENU_HEADER_H + visible_items * MENU_ROW_H + visible_groups * MENU_GROUP_H + 16.0) * z;
    let viewport_w = f32::from(viewport.width);
    let viewport_h = f32::from(viewport.height);
    let raw_x = f32::from(state.position.x);
    let raw_y = f32::from(state.position.y);
    let x = if raw_x + menu_w + MENU_MARGIN > viewport_w {
        (viewport_w - menu_w - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_x.max(MENU_MARGIN)
    };
    let y = if raw_y + menu_h + MENU_MARGIN > viewport_h {
        (viewport_h - menu_h - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_y.max(MENU_MARGIN)
    };

    let dismiss = cx.listener(
        |this: &mut KagiApp, _e: &gpui::MouseDownEvent, _window, cx| {
            this.stash_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );
    let dismiss_r = cx.listener(
        |this: &mut KagiApp, _e: &gpui::MouseDownEvent, _window, cx| {
            this.stash_menu = None;
            cx.stop_propagation();
            cx.notify();
        },
    );

    let mut menu = div()
        .id("stash-context-menu")
        .occlude()
        .absolute()
        .top(px(y))
        .left(px(x))
        .w(theme::scaled_px(MENU_W))
        .max_h(px((viewport_h - MENU_MARGIN * 2.0).max(120.0)))
        .overflow_hidden()
        .rounded(theme::scaled_px(6.))
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().modal))
        .shadow_md()
        .child(
            div()
                .h(theme::scaled_px(MENU_HEADER_H))
                .px_3()
                .flex()
                .flex_row()
                .items_center()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(header),
        );

    for (group_ix, group) in groups.into_iter().enumerate() {
        if !group.items.iter().any(|i| i.state != ItemState::Hidden) {
            continue;
        }
        if let Some(title) = group.title {
            let title_color = if title == "Danger" {
                theme().color_warning
            } else {
                theme().text_muted
            };
            menu = menu.child(
                div()
                    .h(theme::scaled_px(MENU_GROUP_H))
                    .px_3()
                    .pt_1()
                    .text_xs()
                    .text_color(rgb(title_color))
                    .child(SharedString::from(title)),
            );
        }
        for (item_ix, mi) in group.items.into_iter().enumerate() {
            if mi.state == ItemState::Hidden {
                continue;
            }
            menu = menu.child(render_menu_item(group_ix, item_ix, state.clone(), mi, cx));
        }
    }

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .occlude()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.01)
                .on_mouse_down(MouseButton::Left, dismiss)
                .on_mouse_down(MouseButton::Right, dismiss_r),
        )
        .child(menu)
        .into_any_element()
}

fn render_menu_item(
    group_ix: usize,
    item_ix: usize,
    state: StashMenuState,
    mi: MenuItem<StashAction>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let action = mi.action.clone();
    let label_color = if mi.dangerous {
        theme().color_blocker
    } else {
        theme().text_main
    };
    let text = if mi.dangerous {
        SharedString::from(format!("⚠ {}", mi.label.as_ref()))
    } else {
        mi.label.clone()
    };
    let click = cx.listener(
        move |this: &mut KagiApp, _e: &gpui::ClickEvent, window, cx| {
            this.stash_menu = None;
            this.dispatch_stash_action(action.clone(), state.clone(), window, cx);
            cx.notify();
        },
    );
    div()
        .id(SharedString::from(format!(
            "stash-menu-item-{}-{}",
            group_ix, item_ix
        )))
        .h(theme::scaled_px(MENU_ROW_H))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_sm()
        .text_color(rgb(label_color))
        .overflow_hidden()
        .child(div().flex_1().truncate().child(text))
        .on_click(click)
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .into_any_element()
}
