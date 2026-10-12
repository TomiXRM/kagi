//! Shared, viewport-clamped context menus, including generic cascading submenus.
use gpui::{
    canvas, div, point, prelude::*, px, rgb, AnyElement, ClickEvent, Context, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Point, Role, SharedString, Window,
};
use gpui_component::tooltip::Tooltip;

use super::context_menu::{ItemState, MenuGroup, MenuItem, MenuItemKind};
use super::menu_keys::{
    MenuBack, MenuDismiss, MenuEscape, MenuFirst, MenuKeys, MenuLast, MenuNext, MenuOpen, MenuPrev,
    Step, MENU_CONTEXT,
};
use super::theme::{self, theme};
use super::KagiApp;

pub const MENU_MARGIN: f32 = 8.0;
pub const MENU_ROW_H: f32 = 24.0;
pub const MENU_HEADER_H: f32 = 30.0;
pub const MENU_GROUP_H: f32 = 18.0;
const MENU_SEPARATOR_H: f32 = 9.0;

/// The owner supplies persistent MenuKeys when it offers submenus. The overlay
/// owns presentation only; selecting a leaf still uses the owner's dispatcher.
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
    let backdrop = div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .occlude()
        .bg(rgb(theme().modal_overlay))
        .opacity(0.01);
    let dismiss = on_dismiss.clone();
    let dismiss_right = on_dismiss.clone();
    let backdrop = backdrop
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                dismiss(this, window, cx);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                dismiss_right(this, window, cx);
                cx.stop_propagation();
                cx.notify();
            }),
        );
    let panel = Panel {
        id: id.into(),
        prefix: item_id_prefix.into(),
        menu_w,
        danger_title,
        position,
        label: header,
        header: true,
        groups: &groups,
        keys,
        parent: None,
    };
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .occlude()
        .child(backdrop)
        .child(render_panels(&panel, &on_select, &on_dismiss, window, cx))
        .into_any_element()
}

struct Panel<'a, A> {
    id: SharedString,
    prefix: SharedString,
    menu_w: f32,
    danger_title: &'static str,
    position: Point<Pixels>,
    label: SharedString,
    header: bool,
    groups: &'a [MenuGroup<A>],
    keys: Option<&'a MenuKeys>,
    parent: Option<&'a MenuKeys>,
}

fn menu_height<A>(panel: &Panel<A>) -> f32 {
    let mut rows = 0;
    let mut groups = 0usize;
    let mut titles = 0;
    for group in panel.groups {
        let visible = group
            .items
            .iter()
            .filter(|item| item.state != ItemState::Hidden)
            .count();
        if visible == 0 {
            continue;
        }
        rows += visible;
        groups += 1;
        titles += usize::from(group.title.is_some());
    }
    (if panel.header { MENU_HEADER_H } else { 0. })
        + rows as f32 * MENU_ROW_H
        + titles as f32 * MENU_GROUP_H
        + groups.saturating_sub(1) as f32 * MENU_SEPARATOR_H
        + 16.
}

fn render_panels<A, S, D>(
    panel: &Panel<A>,
    on_select: &S,
    on_dismiss: &D,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement
where
    A: Clone + 'static,
    S: Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
    D: Fn(&mut KagiApp, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
{
    let mut layer = div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(render_panel(panel, on_select, on_dismiss, window, cx));
    if let Some(keys) = panel.keys {
        if let Some((slot, child_keys)) = keys.submenu() {
            let entry = panel.groups.iter().flat_map(|group| &group.items).nth(slot);
            if let (Some(item), Some(bounds)) = (entry, keys.bounds(slot)) {
                if let MenuItemKind::Submenu(groups) = &item.kind {
                    let control = row_control(panel, slot);
                    let prefix = SharedString::from(format!("{control}-submenu"));
                    let width = f32::from(theme::scaled_px(panel.menu_w));
                    let right = f32::from(bounds.right());
                    let x = if right + width + MENU_MARGIN > f32::from(window.viewport_size().width)
                    {
                        f32::from(bounds.left()) - width
                    } else {
                        right
                    };
                    let child = Panel {
                        id: prefix.clone(),
                        prefix,
                        menu_w: panel.menu_w,
                        danger_title: panel.danger_title,
                        position: point(px(x), bounds.top()),
                        label: item.label.clone(),
                        header: false,
                        groups,
                        keys: Some(&child_keys),
                        parent: Some(keys),
                    };
                    layer = layer.child(render_panels(&child, on_select, on_dismiss, window, cx));
                }
            }
        }
    }
    layer.into_any_element()
}

fn row_control<A>(panel: &Panel<A>, slot: usize) -> String {
    let mut start = 0;
    for (group_ix, group) in panel.groups.iter().enumerate() {
        if slot < start + group.items.len() {
            return format!("{}-{group_ix}-{}", panel.prefix, slot - start);
        }
        start += group.items.len();
    }
    unreachable!("submenu slot belongs to the rendered menu")
}

fn render_panel<A, S, D>(
    panel: &Panel<A>,
    on_select: &S,
    on_dismiss: &D,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement
where
    A: Clone + 'static,
    S: Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
    D: Fn(&mut KagiApp, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
{
    let position = kagi_ui_core::theme::clamp_menu_pos(
        panel.position,
        panel.menu_w,
        menu_height(panel),
        window.viewport_size(),
    );
    let opened = panel.keys.is_some_and(|keys| {
        keys.drawn(
            panel
                .groups
                .iter()
                .flat_map(|group| &group.items)
                .enumerate()
                .filter(|(_, item)| item.state != ItemState::Hidden)
                .map(|(slot, _)| slot),
        )
    });
    let mut menu = div()
        .id(panel.id.clone())
        .role(Role::Menu)
        .aria_label(panel.label.clone())
        .occlude()
        .absolute()
        .top(position.y)
        .left(position.x)
        .w(theme::scaled_px(panel.menu_w))
        .max_h(px((f32::from(window.viewport_size().height)
            - MENU_MARGIN * 2.)
            .max(120.)))
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme::scaled_px(6.))
        .border_1()
        .border_color(rgb(theme().elevated_border(theme().selected)))
        .bg(rgb(theme().elevated_surface(theme().modal)))
        .shadow_md();
    if panel.header {
        menu = menu.child(
            div()
                .h(theme::scaled_px(MENU_HEADER_H))
                .flex_shrink_0()
                .px_3()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(rgb(theme().selected))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(panel.label.clone()),
        );
    }
    menu = menu.child(render_items(panel, on_select, cx));
    if let Some(keys) = panel.keys {
        menu = with_menu_keys(
            menu.key_context(MENU_CONTEXT),
            keys,
            panel.parent,
            (*on_dismiss).clone(),
            opened,
            window,
            cx,
        );
    }
    menu = cancel_parent_hover(menu, panel.parent, cx);
    #[cfg(feature = "gui-e2e")]
    let menu = menu.child(super::e2e::measure_inside(panel.id.to_string()));
    menu.into_any_element()
}

fn cancel_parent_hover(
    menu: gpui::Stateful<gpui::Div>,
    parent: Option<&MenuKeys>,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    if let Some(parent) = parent {
        let parent = parent.clone();
        menu.on_hover(cx.listener(move |_, hovered: &bool, _, _| {
            if *hovered {
                parent.cancel_hover();
            }
        }))
    } else {
        menu
    }
}

fn render_items<A, S>(
    panel: &Panel<A>,
    on_select: &S,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div>
where
    A: Clone + 'static,
    S: Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
{
    let mut list = div()
        .id(SharedString::from(format!("{}-items", panel.id)))
        .flex()
        .flex_col()
        .flex_shrink(1.)
        .min_h(px(0.))
        .overflow_y_scroll();
    if let Some(keys) = panel.keys {
        list = list.track_scroll(keys.scroll());
    }
    let (mut slot, mut child, mut previous) = (0, 0, false);
    for (group_ix, group) in panel.groups.iter().enumerate() {
        if group
            .items
            .iter()
            .all(|item| item.state == ItemState::Hidden)
        {
            slot += group.items.len();
            continue;
        }
        if previous {
            list = list.child(separator());
            child += 1;
        }
        previous = true;
        if let Some(title) = group.title {
            list = list.child(
                div()
                    .h(theme::scaled_px(MENU_GROUP_H))
                    .flex_shrink_0()
                    .px_3()
                    .pt_1()
                    .text_xs()
                    .text_color(rgb(if title == panel.danger_title {
                        theme().color_warning
                    } else {
                        theme().text_muted
                    }))
                    .child(SharedString::from(title)),
            );
            child += 1;
        }
        for (item_ix, item) in group.items.iter().enumerate() {
            if item.state == ItemState::Hidden {
                slot += 1;
                continue;
            }
            let control = format!("{}-{group_ix}-{item_ix}", panel.prefix);
            if let Some(keys) = panel.keys {
                keys.place(slot, child);
                #[cfg(feature = "gui-e2e")]
                SLOT_CONTROLS.with(|map| {
                    map.borrow_mut().insert(slot, control.clone());
                });
            }
            list = list.child(render_item(
                control,
                item,
                slot,
                panel.keys,
                (*on_select).clone(),
                cx,
            ));
            slot += 1;
            child += 1;
        }
    }
    list
}

fn separator() -> impl IntoElement {
    div()
        .h(theme::scaled_px(MENU_SEPARATOR_H))
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(theme::scaled_px(1.))
                .bg(rgb(theme().selected)),
        )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn with_menu_keys(
    menu: gpui::Stateful<gpui::Div>,
    keys: &MenuKeys,
    parent: Option<&MenuKeys>,
    on_dismiss: impl Fn(&mut KagiApp, &mut Window, &mut Context<KagiApp>) + Clone + 'static,
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
    let parent = parent.cloned();
    let escape = on_dismiss.clone();
    let escape_parent = parent.clone();
    menu.on_action(move |_: &MenuPrev, window, cx| prev(window, cx))
        .on_action(move |_: &MenuNext, window, cx| next(window, cx))
        .on_action(move |_: &MenuFirst, window, cx| first(window, cx))
        .on_action(move |_: &MenuLast, window, cx| last(window, cx))
        .on_action(cx.listener(move |_, _: &MenuBack, window, cx| {
            if let Some(parent) = &parent {
                parent.close_submenu(window, cx);
            }
            cx.notify();
        }))
        .on_action(cx.listener(move |this, _: &MenuEscape, window, cx| {
            if let Some(parent) = &escape_parent {
                parent.close_submenu(window, cx);
            } else {
                escape(this, window, cx);
            }
            cx.notify();
        }))
        .on_action(cx.listener(move |this, _: &MenuDismiss, window, cx| {
            on_dismiss(this, window, cx);
            cx.notify();
        }))
}

fn render_item<A, S>(
    control: String,
    item: &MenuItem<A>,
    slot: usize,
    keys: Option<&MenuKeys>,
    on_select: S,
    cx: &mut Context<KagiApp>,
) -> AnyElement
where
    A: Clone + 'static,
    S: Fn(&mut KagiApp, A, &mut Window, &mut Context<KagiApp>) + 'static,
{
    let control: SharedString = control.into();
    let enabled = item.state == ItemState::Enabled;
    let mut row = base_row(&control, item);
    if let Some(keys) = keys {
        row = focus_row(row, keys, slot, cx);
    }
    if matches!(item.kind, MenuItemKind::Submenu(_)) {
        row = row.child("›").aria_expanded(
            keys.is_some_and(|keys| keys.submenu().is_some_and(|(at, _)| at == slot)),
        );
        if let Some(keys) = keys {
            row = submenu_row(row, keys, slot, cx);
        }
    } else if let MenuItemKind::Command(action) = &item.kind {
        if enabled {
            let action = action.clone();
            row = row.on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                on_select(this, action.clone(), window, cx);
                cx.notify();
            }));
        }
    }
    if let Some(keys) = keys.filter(|_| matches!(item.kind, MenuItemKind::Command(_))) {
        let keys = keys.clone();
        row = row.on_hover(cx.listener(move |_, hovered: &bool, window, cx| {
            if *hovered {
                keys.hover_submenu(None, window, cx);
            }
        }));
    }
    row = if enabled {
        row.hover(|style| style.bg(rgb(theme().selected)).cursor_default())
    } else {
        row.hover(|style| style.bg(rgb(theme().surface)))
            .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                builder.parent_node().set_disabled()
            })
    };
    disabled_reason(row, control, &item.state)
}

fn base_row<A>(control: &SharedString, item: &MenuItem<A>) -> gpui::Stateful<gpui::Div> {
    let color = match (item.state == ItemState::Enabled, item.dangerous) {
        (true, true) => theme().color_blocker,
        (true, false) => theme().text_main,
        (false, true) => theme().color_blocker_muted,
        (false, false) => theme().text_muted,
    };
    let text = if item.dangerous {
        SharedString::from(format!("⚠ {}", item.label))
    } else {
        item.label.clone()
    };
    div()
        .id(control.clone())
        .role(Role::MenuItem)
        .aria_label(item.label.clone())
        .h(theme::scaled_px(MENU_ROW_H))
        .flex_shrink_0()
        .px_3()
        .flex()
        .items_center()
        .text_sm()
        .text_color(rgb(color))
        .overflow_hidden()
        .child(div().flex_1().truncate().child(text))
}

pub(crate) fn focus_row(
    row: gpui::Stateful<gpui::Div>,
    keys: &MenuKeys,
    slot: usize,
    cx: &gpui::App,
) -> gpui::Stateful<gpui::Div> {
    let probe = keys.clone();
    row.track_focus(&keys.item(slot, cx).tab_index(0).tab_stop(false))
        .on_key_down(super::keyboard_nav::stop_activation_keys)
        .focus_visible(|style| style.bg(rgb(theme().selected)))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, _, _| probe.record_bounds(slot, bounds),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

fn submenu_row(
    mut row: gpui::Stateful<gpui::Div>,
    keys: &MenuKeys,
    slot: usize,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let click = keys.clone();
    let right = keys.clone();
    let hover = keys.clone();
    row = row
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
            click.cancel_hover();
            click.open_submenu(slot, true);
            cx.notify();
        }))
        .on_action(cx.listener(move |_, _: &MenuOpen, _, cx| {
            right.cancel_hover();
            right.open_submenu(slot, true);
            cx.notify();
        }))
        .on_hover(cx.listener(move |_, hovered: &bool, window, cx| {
            if *hovered {
                hover.hover_submenu(Some(slot), window, cx);
            }
        }));
    row
}

fn disabled_reason(
    row: gpui::Stateful<gpui::Div>,
    _control: SharedString,
    state: &ItemState,
) -> AnyElement {
    #[cfg(feature = "gui-e2e")]
    record_description(
        &_control,
        match state {
            ItemState::Disabled(reason) => Some(reason.clone()),
            _ => None,
        },
    );
    let row = if let ItemState::Disabled(reason) = state {
        let reason = reason.clone();
        #[cfg(feature = "gui-e2e")]
        let tooltip_control = _control.clone();
        row.aria_description(reason.clone())
            .tooltip(move |window, cx| {
                #[cfg(feature = "gui-e2e")]
                TOOLTIPS.with(|map| {
                    map.borrow_mut()
                        .insert(tooltip_control.to_string(), reason.clone());
                });
                Tooltip::new(reason.clone()).build(window, cx)
            })
            .into_any_element()
    } else {
        row.into_any_element()
    };
    #[cfg(feature = "gui-e2e")]
    {
        super::e2e::measure_control(_control.to_string(), row)
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        row
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DESCRIPTIONS: std::cell::RefCell<std::collections::HashMap<String, Option<SharedString>>> = std::cell::RefCell::default();
    static TOOLTIPS: std::cell::RefCell<std::collections::HashMap<String, SharedString>> = std::cell::RefCell::default();
    static SLOT_CONTROLS: std::cell::RefCell<std::collections::HashMap<usize, String>> = std::cell::RefCell::default();
}

#[cfg(feature = "gui-e2e")]
fn record_description(control: &str, description: Option<SharedString>) {
    DESCRIPTIONS.with(|map| {
        map.borrow_mut().insert(control.to_string(), description);
    });
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_item_description(control: &str) -> Option<Option<SharedString>> {
    DESCRIPTIONS.with(|map| map.borrow().get(control).cloned())
}

/// Text handed to the real Tooltip builder when the pointer requests it.
#[cfg(feature = "gui-e2e")]
pub fn recorded_item_tooltip(control: &str) -> Option<SharedString> {
    TOOLTIPS.with(|map| map.borrow().get(control).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_slot_control(slot: usize) -> Option<String> {
    SLOT_CONTROLS.with(|map| map.borrow().get(&slot).cloned())
}
