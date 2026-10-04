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
    MouseDownEvent, Pixels, Point, Role, SharedString, Window,
};
use gpui_component::tooltip::Tooltip;

use super::context_menu::{ItemState, MenuGroup, MenuItem};
use super::menu_keys::{
    MenuDismiss, MenuFirst, MenuKeys, MenuLast, MenuNext, MenuPrev, Step, MENU_CONTEXT,
};
use super::theme::{self, theme};
use super::KagiApp;

pub const MENU_MARGIN: f32 = 8.0;
// W27-UIPOLISH: Zed-style compact density — tighter rows, group headers, and
// title bar than the previous 28/36/22 (≈14% shorter overall).
pub const MENU_ROW_H: f32 = 24.0;
pub const MENU_HEADER_H: f32 = 30.0;
pub const MENU_GROUP_H: f32 = 18.0;
const MENU_SEPARATOR_H: f32 = 9.0;

/// Render a positioned context-menu overlay from a generic group list.
///
/// * `id` — unique element id for the menu box (e.g. `"commit-context-menu"`).
/// * `item_id_prefix` — element-id prefix for rows (`"commit-menu-item"`).
/// * `menu_w` — the menu's design width in unscaled px (differs per menu).
/// * `danger_title` — the group title that should render in the warning colour.
/// * `position` — cursor anchor in unscaled window px.
/// * `on_dismiss` — clears the owning menu state (run for the backdrop click).
/// * `on_select` — clears the owning state and dispatches the chosen action.
/// * `keys` — the context menus' keyboard (#985, see `menu_keys`); `None`
///   for a menu without it (the pointer and the window's Escape only).
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
    keys: Option<&MenuKeys>,
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
    let mut visible_groups = 0_usize;
    let mut titled_groups = 0_usize;
    for group in &groups {
        if group
            .items
            .iter()
            .any(|item| item.state != ItemState::Hidden)
        {
            visible_groups += 1;
            titled_groups += usize::from(group.title.is_some());
        }
    }

    // Include only drawn headings and boundaries, including untitled groups.
    let menu_h = MENU_HEADER_H
        + visible_items * MENU_ROW_H
        + titled_groups as f32 * MENU_GROUP_H
        + visible_groups.saturating_sub(1) as f32 * MENU_SEPARATOR_H
        + 16.0;
    let clamped = kagi_ui_core::theme::clamp_menu_pos(position, menu_w, menu_h, viewport);
    let (x, y) = (f32::from(clamped.x), f32::from(clamped.y));
    let viewport_h = f32::from(viewport.height);

    let dismiss_left = {
        let on_dismiss = on_dismiss.clone();
        cx.listener(move |this: &mut KagiApp, _e: &MouseDownEvent, window, cx| {
            on_dismiss(this, window, cx);
            cx.stop_propagation();
            cx.notify();
        })
    };
    // Tab / Shift+Tab close a keyboard menu as the backdrop does (#991
    // review); the next frame gives the focus back where it came from.
    let dismiss_key = keys.map(|_| {
        let on_dismiss = on_dismiss.clone();
        cx.listener(move |this: &mut KagiApp, _: &MenuDismiss, window, cx| {
            on_dismiss(this, window, cx);
            cx.notify();
        })
    });
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
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme::scaled_px(6.))
        .border_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(theme().modal))
        .shadow_md()
        .child(
            div()
                .h(theme::scaled_px(MENU_HEADER_H))
                .flex_shrink_0()
                .px_3()
                .flex()
                .flex_row()
                .items_center()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(header.clone()),
        )
        .role(Role::Menu)
        .aria_label(header);
    // #985: the menu's keyboard (see `menu_keys`). The enabled items' slots
    // count the drawn (not hidden) items in drawing order.
    let opened = keys.is_some_and(|keys| {
        keys.drawn(
            groups
                .iter()
                .flat_map(|group| group.items.iter())
                .filter(|item| item.state != ItemState::Hidden)
                .enumerate()
                .filter(|(_, item)| item.state == ItemState::Enabled)
                .map(|(slot, _)| slot),
        )
    });

    // The items scroll under the header when the menu is taller than the
    // window (#991 review: a key must not focus an item out of sight).
    let mut list = div()
        .id("menu-items")
        .flex()
        .flex_col()
        .flex_shrink(1.)
        .min_h(px(0.))
        .overflow_y_scroll();
    if let Some(keys) = keys {
        list = list.track_scroll(keys.scroll());
    }
    let mut child = 0_usize;
    let mut slot = 0_usize;
    let mut previous_group = false;
    for (group_ix, group) in groups.into_iter().enumerate() {
        if !group
            .items
            .iter()
            .any(|item| item.state != ItemState::Hidden)
        {
            continue;
        }
        if previous_group {
            list = list.child(
                div()
                    .h(theme::scaled_px(MENU_SEPARATOR_H))
                    .flex_shrink_0()
                    .px_2()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .h(theme::scaled_px(1.0))
                            .bg(rgb(theme().selected)),
                    ),
            );
            child += 1;
        }
        previous_group = true;
        if let Some(title) = group.title {
            let title_color = if title == danger_title {
                theme().color_warning
            } else {
                theme().text_muted
            };
            list = list.child(
                div()
                    .h(theme::scaled_px(MENU_GROUP_H))
                    .flex_shrink_0()
                    .px_3()
                    .pt_1()
                    .text_xs()
                    .text_color(rgb(title_color))
                    .child(SharedString::from(title)),
            );
            child += 1;
        }
        for (item_ix, item) in group.items.into_iter().enumerate() {
            if item.state == ItemState::Hidden {
                continue;
            }
            if let Some(keys) = keys {
                keys.place(child);
            }
            list = list.child(render_menu_item(
                item_id_prefix,
                group_ix,
                item_ix,
                item,
                on_select.clone(),
                keys.map(|keys| keys.item(slot, cx)),
                cx,
            ));
            child += 1;
            slot += 1;
        }
    }
    menu = menu.child(list);
    if let Some(keys) = keys {
        menu = with_menu_keys(menu.key_context(MENU_CONTEXT), keys, opened, window, cx);
    }
    if let Some(dismiss_key) = dismiss_key {
        menu = menu.on_action(dismiss_key);
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

/// The menu's ↑/↓/Home/End, and the focus on its first enabled item when
/// it has just `opened`.
fn with_menu_keys(
    menu: gpui::Stateful<gpui::Div>,
    keys: &MenuKeys,
    opened: bool,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    if opened {
        keys.step(Step::First, window, cx);
    }
    let step = |step: Step| {
        let keys = keys.clone();
        move |window: &mut Window, cx: &mut gpui::App| keys.step(step, window, cx)
    };
    let (prev, next, first, last) = (
        step(Step::Prev),
        step(Step::Next),
        step(Step::First),
        step(Step::Last),
    );
    menu.on_action(move |_: &MenuPrev, window, cx| prev(window, cx))
        .on_action(move |_: &MenuNext, window, cx| next(window, cx))
        .on_action(move |_: &MenuFirst, window, cx| first(window, cx))
        .on_action(move |_: &MenuLast, window, cx| last(window, cx))
}

fn render_menu_item<A>(
    item_id_prefix: &'static str,
    group_ix: usize,
    item_ix: usize,
    item: MenuItem<A>,
    on_select: impl Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + 'static,
    focus: Option<gpui::FocusHandle>,
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
        .role(Role::MenuItem)
        .aria_label(item.label.clone())
        .h(theme::scaled_px(MENU_ROW_H))
        .flex_shrink_0()
        .px_3()
        .flex()
        .flex_row()
        .items_center()
        .text_sm()
        .text_color(rgb(label_color))
        .overflow_hidden()
        .child(div().flex_1().truncate().child(text));

    let row = match (enabled, focus) {
        // #985: the menu's keyboard stops on enabled items only (↑/↓ rove;
        // Enter / Space press one through gpui's keyboard click on
        // `on_click`). Keyboard focus shows as the pointer's hover does.
        (true, Some(focus)) => row
            .track_focus(&focus.tab_index(0).tab_stop(false))
            .on_key_down(super::keyboard_nav::stop_activation_keys)
            .focus_visible(|style| style.bg(rgb(theme().selected))),
        _ => row,
    };
    let row = if enabled {
        row.on_click(click)
            .hover(|style| style.bg(rgb(theme().selected)).cursor_pointer())
    } else {
        // gpui has no aria_disabled setter; the node's own builder has one.
        row.hover(|style| style.bg(rgb(theme().surface)))
            .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                builder.parent_node().set_disabled();
            })
    };

    #[cfg(feature = "gui-e2e")]
    let control = format!("{item_id_prefix}-{group_ix}-{item_ix}");
    // A disabled item says why to assistive technology too, not only in its
    // pointer tooltip (#991 review).
    let row = match item.state {
        ItemState::Disabled(reason) => {
            #[cfg(feature = "gui-e2e")]
            record_description(&control, Some(reason.clone()));
            row.aria_description(reason.clone())
                .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
                .into_any_element()
        }
        _ => {
            #[cfg(feature = "gui-e2e")]
            record_description(&control, None);
            row.into_any_element()
        }
    };
    #[cfg(feature = "gui-e2e")]
    {
        super::e2e::measure_control(control, row)
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        row
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DESCRIPTIONS: std::cell::RefCell<std::collections::HashMap<String, Option<SharedString>>> =
        std::cell::RefCell::default();
}

/// What the item drawn as `control` last handed `aria_description`.
#[cfg(feature = "gui-e2e")]
fn record_description(control: &str, description: Option<SharedString>) {
    DESCRIPTIONS.with(|map| {
        map.borrow_mut().insert(control.to_string(), description);
    });
}

/// The `aria_description` the menu item `control` was last drawn with:
/// `None` when it was never drawn, `Some(None)` when it had none.
#[cfg(feature = "gui-e2e")]
pub fn recorded_item_description(control: &str) -> Option<Option<SharedString>> {
    DESCRIPTIONS.with(|map| map.borrow().get(control).cloned())
}
