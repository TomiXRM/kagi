//! Shared context-menu overlay renderer.
//!
//! The commit, branch, and stash context menus were three near-identical copies
//! of the same positioned-overlay + grouped-item machinery (viewport clamping,
//! zoom-aware sizing, the occlude/click-through dismiss fix, group headers, and
//! per-item Enabled/Disabled/Hidden + dangerous styling). This module hosts that
//! logic once, generic over the menu's action type `A`; the three menu modules
//! keep their public `render_*_menu_overlay` entry points as thin wrappers that
//! supply the action-dispatch and state-clearing closures.

use gpui::{
    div, prelude::*, px, rgb, AnyElement, ClickEvent, Context, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Point, SharedString, Window,
};
use gpui_component::tooltip::Tooltip;

use super::context_menu::{ItemState, MenuGroup, MenuItem};
use super::theme::{self, theme};
use super::KagiApp;

pub const MENU_MARGIN: f32 = 8.0;
// W27-UIPOLISH: Zed-style compact density — tighter rows, group headers, and
// title bar than the previous 28/36/22 (≈14% shorter overall).
pub const MENU_ROW_H: f32 = 24.0;
pub const MENU_HEADER_H: f32 = 30.0;
pub const MENU_GROUP_H: f32 = 18.0;

/// Render a positioned context-menu overlay from a generic group list.
///
/// * `id` — unique element id for the menu box (e.g. `"commit-context-menu"`).
/// * `item_id_prefix` — element-id prefix for rows (`"commit-menu-item"`).
/// * `menu_w` — the menu's design width in unscaled px (differs per menu).
/// * `danger_title` — the group title that should render in the warning colour.
/// * `position` — cursor anchor in unscaled window px.
/// * `on_dismiss` — clears the owning menu state (run for the backdrop click).
/// * `on_select` — clears the owning state and dispatches the chosen action.
#[allow(clippy::too_many_arguments)]
pub fn render_menu_overlay<A>(
    id: &'static str,
    item_id_prefix: &'static str,
    menu_w: f32,
    danger_title: &'static str,
    position: Point<Pixels>,
    header: SharedString,
    groups: Vec<MenuGroup<A>>,
    on_dismiss: impl Fn(&mut KagiApp, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
    on_select: impl Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement
where
    A: Clone + 'static,
{
    let viewport = window.viewport_size();
    let visible_items = groups
        .iter()
        .flat_map(|group| group.items.iter())
        .filter(|item| item.state != ItemState::Hidden)
        .count() as f32;
    let visible_groups = groups
        .iter()
        .filter(|group| {
            group
                .items
                .iter()
                .any(|item| item.state != ItemState::Hidden)
        })
        .count() as f32;

    // The box is rendered with `scaled_px`, so its on-screen footprint is
    // `zoom()`-scaled. Off-screen clamping happens in raw window pixels (mouse
    // position is unscaled), so scale the width/height here to match.
    let z = theme::zoom();
    let menu_w_scaled = menu_w * z;
    let menu_h =
        (MENU_HEADER_H + visible_items * MENU_ROW_H + visible_groups * MENU_GROUP_H + 16.0) * z;
    let viewport_w = f32::from(viewport.width);
    let viewport_h = f32::from(viewport.height);
    let raw_x = f32::from(position.x);
    let raw_y = f32::from(position.y);
    let x = if raw_x + menu_w_scaled + MENU_MARGIN > viewport_w {
        (viewport_w - menu_w_scaled - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_x.max(MENU_MARGIN)
    };
    let y = if raw_y + menu_h + MENU_MARGIN > viewport_h {
        (viewport_h - menu_h - MENU_MARGIN).max(MENU_MARGIN)
    } else {
        raw_y.max(MENU_MARGIN)
    };

    let dismiss_left = {
        let on_dismiss = on_dismiss.clone();
        cx.listener(move |this: &mut KagiApp, _e: &MouseDownEvent, window, cx| {
            on_dismiss(this, window, cx);
            cx.stop_propagation();
            cx.notify();
        })
    };
    let dismiss_right = cx.listener(move |this: &mut KagiApp, _e: &MouseDownEvent, window, cx| {
        on_dismiss(this, window, cx);
        cx.stop_propagation();
        cx.notify();
    });

    let mut menu = div()
        .id(id)
        // Block mouse events from reaching the dismiss backdrop below — without
        // this, pressing a menu item fires the backdrop's on_mouse_down first,
        // the menu unmounts, and the item's on_click (down+up on the same
        // element) never completes (user-reported click-through bug).
        .occlude()
        .absolute()
        .top(px(y))
        .left(px(x))
        .w(theme::scaled_px(menu_w))
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
        if !group
            .items
            .iter()
            .any(|item| item.state != ItemState::Hidden)
        {
            continue;
        }
        if let Some(title) = group.title {
            let title_color = if title == danger_title {
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
        for (item_ix, item) in group.items.into_iter().enumerate() {
            if item.state == ItemState::Hidden {
                continue;
            }
            menu = menu.child(render_menu_item(
                item_id_prefix,
                group_ix,
                item_ix,
                item,
                on_select.clone(),
                cx,
            ));
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
                .on_mouse_down(MouseButton::Left, dismiss_left)
                .on_mouse_down(MouseButton::Right, dismiss_right),
        )
        .child(menu)
        .into_any_element()
}

fn render_menu_item<A>(
    item_id_prefix: &'static str,
    group_ix: usize,
    item_ix: usize,
    item: MenuItem<A>,
    on_select: impl Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + 'static,
    cx: &mut Context<KagiApp>,
) -> AnyElement
where
    A: Clone + 'static,
{
    let enabled = item.state == ItemState::Enabled;
    let action = item.action.clone();
    let label_color = match (&item.state, item.dangerous) {
        (ItemState::Enabled, true) => theme().color_blocker,
        (ItemState::Enabled, false) => theme().text_main,
        (ItemState::Disabled(_), true) => theme().color_blocker_muted,
        (ItemState::Disabled(_), false) => theme().text_muted,
        (ItemState::Hidden, _) => theme().text_muted,
    };
    let text = if item.dangerous {
        SharedString::from(format!("⚠ {}", item.label.as_ref()))
    } else {
        item.label.clone()
    };

    let click = cx.listener(move |this: &mut KagiApp, _e: &ClickEvent, window, cx| {
        on_select(this, action.clone(), window, cx);
        cx.notify();
    });

    let row = div()
        .id(SharedString::from(format!(
            "{}-{}-{}",
            item_id_prefix, group_ix, item_ix
        )))
        .h(theme::scaled_px(MENU_ROW_H))
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_sm()
        .text_color(rgb(label_color))
        .overflow_hidden()
        .child(div().flex_1().truncate().child(text));

    let row = if enabled {
        row.on_click(click)
            .hover(|style| style.bg(rgb(theme().selected)).cursor_pointer())
    } else {
        row.hover(|style| style.bg(rgb(theme().surface)))
    };

    match item.state {
        ItemState::Disabled(reason) => row
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            .into_any_element(),
        _ => row.into_any_element(),
    }
}
