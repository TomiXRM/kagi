//! Compact navigator presentation; repository evidence and operations stay in their owners.

use std::collections::HashMap;

use gpui::{
    div, prelude::*, px, rgb, svg, Anchor, AnyElement, App, Context, Div, Entity, FocusHandle,
    Focusable, IntoElement, RenderOnce, Role, ScrollHandle, Stateful, WeakEntity, Window,
};
use gpui_component::{
    popover::{Popover, PopoverState},
    Selectable,
};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::{
    keyboard_nav,
    theme::{self, theme},
    KagiApp, Msg,
};

const LABELS: [Msg; 7] = [
    Msg::A11ySidebarLocal,
    Msg::A11ySidebarRemote,
    Msg::A11ySidebarWorktrees,
    Msg::A11ySidebarTags,
    Msg::A11ySidebarStashes,
    Msg::WorkspacePrs,
    Msg::WorkspaceIssues,
];
const IDS: [&str; 7] = [
    "compact-rail-local",
    "compact-rail-remote",
    "compact-rail-worktrees",
    "compact-rail-tags",
    "compact-rail-stashes",
    "compact-rail-prs",
    "compact-rail-issues",
];
const ICONS: [&str; 7] = [
    "icons/git-branch.svg",
    "icons/cloud.svg",
    "icons/tree-pine.svg",
    "icons/tag.svg",
    "icons/inbox.svg",
    "icons/git-pull-request.svg",
    "icons/circle-dot.svg",
];

struct RailState {
    active: Option<usize>,
    owner: Option<crate::app::SessionId>,
    popovers: [Option<Entity<PopoverState>>; 7],
    content_focus: [Option<FocusHandle>; 7],
    triggers: keyboard_nav::FocusSlots,
    previous: Option<FocusHandle>,
    root: Option<FocusHandle>,
    reset: bool,
    focus_pending: bool,
    filter_pending: bool,
    scroll: ScrollHandle,
    revealed: Option<usize>,
}

#[derive(Default)]
struct RailCache(HashMap<gpui::WindowId, WeakEntity<RailState>>);
impl gpui::Global for RailCache {}

fn state(window: &Window, cx: &App) -> Option<Entity<RailState>> {
    cx.try_global::<RailCache>()?
        .0
        .get(&window.window_handle().window_id())?
        .upgrade()
}

fn render_state(window: &mut Window, cx: &mut App) -> Entity<RailState> {
    let cached = window.use_keyed_state("compact-sidebar-state", cx, |_, _| RailState {
        active: None,
        owner: None,
        popovers: std::array::from_fn(|_| None),
        content_focus: std::array::from_fn(|_| None),
        triggers: Default::default(),
        previous: None,
        root: None,
        reset: false,
        focus_pending: false,
        filter_pending: false,
        scroll: ScrollHandle::new(),
        revealed: None,
    });
    let cache = cx.default_global::<RailCache>();
    cache.0.retain(|_, state| state.upgrade().is_some());
    cache
        .0
        .insert(window.window_handle().window_id(), cached.downgrade());
    cached
}

impl RailState {
    fn owns_focus(&self, window: &Window, cx: &App) -> bool {
        (0..7).any(|index| self.triggers.get(index, cx).is_focused(window))
            || self
                .content_focus
                .iter()
                .flatten()
                .any(|focus| focus.contains_focused(window, cx))
    }
}

pub(super) fn is_open(window: &mut Window, cx: &mut Context<KagiApp>) -> bool {
    state(window, cx).is_some_and(|state| state.read(cx).active.is_some())
}

/// GitHub navigator filters exclusively own the shared chips over the dashboard.
pub(super) fn owns_list_filter(window: &Window, cx: &App) -> bool {
    state(window, cx).is_some_and(|state| matches!(state.read(cx).active, Some(5 | 6)))
}

/// Cmd-F opens the existing Graph pane, then focuses its rendered shared Input.
pub(super) fn focus_filter(window: &mut Window, cx: &mut Context<KagiApp>) {
    let Some(cached) = state(window, cx) else {
        return;
    };
    let index = cached
        .read(cx)
        .active
        .filter(|index| *index < 5)
        .unwrap_or(0);
    // The shortcut runs inside KagiApp's update; open_changed checks that same
    // owner's suspension state, so release its mutable lease before opening.
    let app = cx.weak_entity();
    window.defer(cx, move |window, cx| {
        open_changed(&cached.downgrade(), &app, index, true, window, cx);
        cached.update(cx, |state, _| state.filter_pending = true);
    });
    cx.notify();
}

/// Used by the shared sidebar focus owner: only the visible tree owns rows.
pub(super) fn active_pane(window: &mut Window, cx: &mut Context<KagiApp>) -> Option<usize> {
    state(window, cx)?
        .read(cx)
        .active
        .filter(|index| *index < 5)
}

/// Width/tab/modal transitions must retire kit deferred-context registrations now.
pub(super) fn dismiss(window: &mut Window, cx: &mut Context<KagiApp>) {
    let Some(cached) = state(window, cx) else {
        return;
    };
    let (owned, root, popovers) = {
        let state = cached.read(cx);
        (
            state.owns_focus(window, cx),
            state.root.clone(),
            state.popovers.clone(),
        )
    };
    cached.update(cx, |state, _| {
        state.reset = true;
        state.active = None;
        state.previous = None;
        state.focus_pending = false;
        state.filter_pending = false;
    });
    for popover in popovers.into_iter().flatten() {
        popover.update(cx, |state, cx| state.dismiss(window, cx));
    }
    if owned {
        if let Some(root) = root {
            root.focus(window, cx);
        }
    }
    cached.update(cx, |state, _| {
        state.popovers = std::array::from_fn(|_| None);
        state.reset = false;
    });
}

/// A leaf dispatched its existing callback. Never override operation-assigned focus.
pub(super) fn dismiss_selection(window: &mut Window, cx: &mut Context<KagiApp>) {
    let Some(cached) = state(window, cx) else {
        return;
    };
    let popover = {
        let state = cached.read(cx);
        state.active.and_then(|index| state.popovers[index].clone())
    };
    if let Some(popover) = popover {
        popover.update(cx, |state, cx| state.dismiss(window, cx));
    }
}

/// Compact worktree activation is the existing menu's Open in New Tab action.
/// Hover/inspection and every other row callback remain in the shared renderer.
pub(super) fn select_worktree(
    app: &mut KagiApp,
    row: &super::sidebar::SidebarRow,
    cx: &mut Context<KagiApp>,
) -> bool {
    if let super::sidebar::SidebarRow::Worktree { path, .. } = row {
        if app.remote_view.is_none() {
            app.open_repository(path.clone(), cx);
            return true;
        }
    }
    false
}

/// Seven dense, named controls; the separator is inside the fixed outer width.
pub(super) fn render(app: &KagiApp, window: &mut Window, cx: &mut Context<KagiApp>) -> AnyElement {
    let cached = render_state(window, cx);
    let scroll = cached.update(cx, |state, _| {
        state.root = app.root_focus.clone();
        state.owner = app.active_session();
        let focused = state.triggers.focused(window);
        if focused != state.revealed {
            state.revealed = focused;
            if let Some(index) = focused {
                // Reveal once per focus transition, preserving later wheel scrolling.
                // The sixth child is the tall-window flexible separator.
                state.scroll.scroll_to_item(index + usize::from(index >= 5));
            }
        }
        state.scroll.clone()
    });
    let rail = div()
        .id("compact-rail")
        .relative()
        .w(theme::scaled_px(super::compact::RAIL_WIDTH))
        .min_w(theme::scaled_px(super::compact::RAIL_WIDTH))
        .max_w(theme::scaled_px(super::compact::RAIL_WIDTH))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar))
        .overflow_hidden()
        .child(super::e2e::measure_inside("compact-rail"))
        .child(
            div()
                .absolute()
                .right_0()
                .top_0()
                .w(px(1.))
                .h_full()
                .bg(rgb(theme().surface)),
        );
    let mut controls = div()
        .id("compact-rail-scroll")
        .w_full()
        .h_full()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .flex()
        .flex_col()
        .items_center()
        .py(theme::scaled_px(4.))
        .gap(theme::scaled_px(4.));
    for index in 0..5 {
        controls = controls.child(control(app, index, &cached, window, cx));
    }
    controls = controls.child(div().flex_1().min_h(px(0.)));
    for index in 5..7 {
        controls = controls.child(control(app, index, &cached, window, cx));
    }
    rail.child(controls).into_any_element()
}

fn content(app: &KagiApp, index: usize, window: &Window, cx: &mut Context<KagiApp>) -> AnyElement {
    let mut panel = div()
        .id(("compact-sidebar-content", index))
        .w(theme::scaled_px(320.)
            .min((window.viewport_size().width - theme::scaled_px(60.)).max(px(0.))))
        .h(theme::scaled_px(420.).min((window.viewport_size().height - px(32.)).max(px(0.))))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .bg(rgb(theme().sidebar))
        .border_1()
        .border_color(rgb(theme().surface))
        .rounded(theme::scaled_px(6.))
        .overflow_hidden()
        .on_key_down(keyboard_nav::stop_activation_keys);
    if index < 5 {
        panel = panel
            .child(super::sidebar_panes::render_filter(app, cx))
            .child(super::sidebar_panes::render_pane(app, index, true, cx));
    } else {
        let kind = if index == 5 {
            super::list_filter_strip::ListKind::Prs
        } else {
            super::list_filter_strip::ListKind::Issues
        };
        let count = if index == 5 {
            super::pr_nav::pr_list_order(app).len()
        } else {
            app.ui().issue_view(app.issue_host_login()).order.len()
        };
        panel = panel.child(super::list_filter_strip::render_strip(app, kind, count, cx));
        #[cfg(feature = "gui-e2e")]
        if index == 5 {
            if let Some(mode) = app.pr_mode() {
                super::e2e::compact_rail::record_pr_scroll(
                    window.window_handle().window_id(),
                    &mode.navigator_scroll,
                );
            }
        }
        panel = panel.child(if index == 5 {
            super::pr_nav::render_compact_pr_list(app, cx)
        } else {
            super::issues_mode::render_compact_issue_list(app, cx)
        });
    }
    super::e2e::measure_control("compact-sidebar-popover", panel)
}

fn control(
    app: &KagiApp,
    index: usize,
    cached: &Entity<RailState>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let open = cached.read(cx).active == Some(index);
    // Build with the already-borrowed parent, not app.update from kit's render.
    let body = open.then(|| content(app, index, window, cx));
    let changed = cached.downgrade();
    let weak_app = cx.weak_entity();
    let retained = cached.clone();
    let first_rows = if index < 5 {
        app.sidebar.focus.lists[index].clone()
    } else {
        None
    };
    let first_header = (index < 5).then(|| app.sidebar.focus.headers.get(index, cx));
    let filter = (index < 5)
        .then(|| {
            app.sidebar
                .filter
                .as_ref()
                .map(|input| input.focus_handle(cx))
        })
        .flatten();
    Popover::new(("compact-sidebar-popover", index))
        .appearance(false)
        .overlay_closable(true)
        .anchor(Anchor::TopLeft)
        .ml(theme::scaled_px(40.))
        .open(open)
        .on_open_change(move |open, window, cx| {
            open_changed(&changed, &weak_app, index, *open, window, cx);
        })
        .trigger(RailTrigger {
            index,
            selected: open,
            state: cached.downgrade(),
            focus: cached.read(cx).triggers.get(index, cx),
            suspended: app.compact.suspended,
        })
        .content(move |popover_state, window, cx| {
            let popover = cx.entity();
            let focus = popover_state.focus_handle(cx);
            let (take_focus, take_filter) = retained.update(cx, |state, _| {
                state.popovers[index] = Some(popover);
                state.content_focus[index] = Some(focus.clone());
                (
                    std::mem::take(&mut state.focus_pending),
                    std::mem::take(&mut state.filter_pending),
                )
            });
            if take_filter {
                if let Some(filter) = &filter {
                    filter.focus(window, cx);
                }
            } else if take_focus {
                focus.focus(window, cx);
                if let Some(rows) = first_rows.as_ref().filter(|rows| rows.has_stop()) {
                    rows.focus_stop(window, cx);
                } else if let Some(header) = &first_header {
                    header.focus(window, cx);
                }
            }
            div()
        })
        .children(body)
        .map(|popover| RailPopover {
            popover,
            index,
            cached: cached.clone(),
        })
        .into_any_element()
}

/// Retain kit state even while closed: a mouse-down followed immediately by a
/// width/tab transition must dismiss its registration before the first popup frame.
#[derive(IntoElement)]
struct RailPopover {
    popover: Popover,
    index: usize,
    cached: Entity<RailState>,
}

impl RenderOnce for RailPopover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // RenderOnce has no entity/element namespace of its own. This is the
        // exact key the wrapped kit Popover uses in the same render scope.
        let popover =
            window.use_keyed_state(("compact-sidebar-popover", self.index), cx, |_, cx| {
                PopoverState::new(false, cx)
            });
        let focus = popover.focus_handle(cx);
        self.cached.update(cx, |state, _| {
            state.popovers[self.index] = Some(popover);
            state.content_focus[self.index] = Some(focus);
        });
        RenderOnce::render(self.popover, window, cx)
    }
}

fn open_changed(
    cached: &WeakEntity<RailState>,
    app: &WeakEntity<KagiApp>,
    index: usize,
    open: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if open
        && app
            .upgrade()
            .is_none_or(|app| app.read(cx).compact.suspended)
    {
        let popover = cached
            .upgrade()
            .and_then(|cached| cached.read(cx).popovers[index].clone());
        if let Some(popover) = popover {
            popover.update(cx, |state, cx| state.dismiss(window, cx));
        }
        return;
    }
    let focused = window.focused(cx);
    let owner = cached.upgrade().and_then(|cached| cached.read(cx).owner);
    let replaced = if open {
        cached.upgrade().and_then(|cached| {
            let state = cached.read(cx);
            state
                .active
                .filter(|active| *active != index)
                .and_then(|active| state.popovers[active].clone())
        })
    } else {
        None
    };
    let previous = cached
        .update(cx, |state, cx| {
            if open {
                if state.active.is_none() && state.previous.is_none() {
                    state.previous = focused.clone();
                }
                state.active = Some(index);
                state.focus_pending = true;
                None
            } else if state.active == Some(index) {
                let restore = !state.reset && (state.owns_focus(window, cx) || focused.is_none());
                state.active = None;
                state.focus_pending = false;
                state.filter_pending = false;
                state.previous.take().filter(|_| restore)
            } else {
                None
            }
        })
        .ok()
        .flatten();
    if let Some(popover) = replaced {
        popover.update(cx, |state, cx| state.dismiss(window, cx));
    }
    let cached = cached.clone();
    let app = app.clone();
    window.defer(cx, move |window, cx| {
        if open
            && cached
                .upgrade()
                .is_none_or(|cached| cached.read(cx).active != Some(index))
        {
            return;
        }
        let _ = app.update(cx, |app, cx| {
            if app.active_session() != owner
                || app.home_in_front()
                || (open && app.compact.suspended)
            {
                return;
            }
            if open {
                match index {
                    5 => app.show_pr_mode(cx),
                    6 => app.show_issues_mode(cx),
                    _ => app.show_graph_mode(cx),
                }
            } else if let Some(previous) = previous {
                restore_focus(app, previous, focused.as_ref(), window, cx);
            }
            cx.notify();
        });
    });
}

fn restore_focus(
    app: &KagiApp,
    previous: FocusHandle,
    focused: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    // A callback may have assigned modal/input/PTY focus since close.
    let unchanged = focused.map_or_else(
        || window.focused(cx).is_none(),
        |focus| focus.is_focused(window),
    );
    if unchanged {
        let destination = if app.active_modal.is_some() {
            app.root_focus.as_ref()
        } else {
            Some(&previous)
        };
        if let Some(destination) = destination {
            destination.focus(window, cx);
        }
    }
}

#[derive(IntoElement)]
struct RailTrigger {
    index: usize,
    selected: bool,
    state: WeakEntity<RailState>,
    focus: FocusHandle,
    suspended: bool,
}

impl Selectable for RailTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for RailTrigger {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let pointer = self.state.clone();
        let index = self.index;
        keyboard_nav::focusable(div().id(IDS[index]))
            .role(Role::Button)
            .aria_label(LABELS[index].t())
            .aria_expanded(self.selected)
            .track_focus(&self.focus.tab_index(0).tab_stop(true))
            .w(theme::scaled_px(36.))
            .h(theme::scaled_px(32.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::scaled_px(4.))
            .text_color(rgb(theme().text_main))
            .cursor_pointer()
            .when(self.selected, |el| el.bg(rgb(theme().surface)))
            .hover(|style| style.bg(rgb(theme().surface)))
            .child(
                svg()
                    .path(ICONS[index])
                    .size(theme::scaled_px(18.))
                    .flex_shrink_0()
                    // GPUI Svg paints only its own text colour, not the parent's.
                    .text_color(rgb(theme().text_main)),
            )
            .child(super::e2e::measure_inside(IDS[index]))
            .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                let focused = window.focused(cx);
                let _ = pointer.update(cx, |state, _| {
                    if state.active.is_none() {
                        state.previous = focused;
                    }
                });
            })
            .on_click(move |event, window, cx| {
                if self.suspended {
                    cx.stop_propagation();
                    return;
                }
                if event.is_keyboard() {
                    let focused = window.focused(cx);
                    let _ = self.state.update(cx, |state, _| {
                        if state.active.is_none() {
                            state.previous = focused;
                        }
                    });
                    let popover = self
                        .state
                        .upgrade()
                        .and_then(|state| state.read(cx).popovers[index].clone());
                    if let Some(popover) = popover {
                        popover.update(cx, |state, cx| {
                            if state.is_open() {
                                state.dismiss(window, cx);
                            } else {
                                state.show(window, cx);
                            }
                        });
                    }
                }
            })
            .hover_tooltip_builder(
                ("compact-sidebar-help", index),
                super::sidebar::name_tooltip(LABELS[index].t().into()),
            )
    }
}

/// Apply the existing keyboard/ring helper to a real PR/Issue row, not a proxy.
#[derive(IntoElement)]
pub(super) struct Leaf {
    pub id: gpui::ElementId,
    pub label: String,
    pub row: Stateful<Div>,
    pub app: WeakEntity<KagiApp>,
}

impl RenderOnce for Leaf {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        keyboard_nav::focusable(self.row)
            .role(Role::Button)
            .aria_label(self.label)
            .track_focus(&focus.tab_index(0).tab_stop(true))
            .on_click(move |_, window, cx| {
                let _ = self.app.update(cx, |_, cx| dismiss_selection(window, cx));
            })
    }
}
