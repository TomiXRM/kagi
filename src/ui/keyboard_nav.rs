//! Keyboard paths for Kagi's hand-made tab lists and rows (#944).
//!
//! A tab list is one Tab stop — its selected cell (roving tabindex) — and the
//! arrows move between its cells: ←/→ to the neighbour, Home/End to the
//! ends. With [`Activation::Automatic`] moving also selects; with
//! [`Activation::Manual`] it only moves focus and Enter/Space select, for a
//! list whose selection starts work (a mode that fetches on entry).
//!
//! Enter and Space press the focused cell or row through gpui's own keyboard
//! click (`on_click` fires on the key-up of an activation key pressed while
//! the element is focused). The key-down is stopped there, so it never reaches
//! the root's Enter fallback (confirm the modal, check out the selected
//! commit). Keys typed into an `Input` never reach these elements: the input
//! holds the focus, and the tab list's bindings are scoped to its own key
//! context.
//!
//! The focus ring is the one `Input` draws (`sync_gpui_component_theme` maps
//! gpui-component's `ring` to `color_branch`), shown only for keyboard focus
//! (`focus_visible`). It is a border kept transparent at rest, so taking focus
//! does not move anything; callers take its width out of their padding.

use std::rc::Rc;

use gpui::{
    actions, prelude::*, rgb, transparent_black, App, ClickEvent, Div, FocusHandle, KeyBinding,
    KeyDownEvent, Role, SharedString, Stateful, Window,
};

use super::theme::theme;

actions!(
    kagi_tab_list,
    [TabListPrev, TabListNext, TabListFirst, TabListLast]
);

const CONTEXT: &str = "KagiTabList";

/// The ring's width in px (`border_2`), taken out of the padding of every
/// element that has it.
pub(crate) const RING: f32 = 2.;

/// ←/→/Home/End inside a tab list. The list's context is deeper than the
/// app-wide `!Terminal && !Input` arrows (PR mode's pane cycling), so these
/// win only while a cell holds the focus.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", TabListPrev, Some(CONTEXT)),
        KeyBinding::new("right", TabListNext, Some(CONTEXT)),
        KeyBinding::new("home", TabListFirst, Some(CONTEXT)),
        KeyBinding::new("end", TabListLast, Some(CONTEXT)),
    ]);
}

/// The focus handles of one tab list's cells, one per slot, made on first
/// draw and kept across frames.
#[derive(Default)]
pub(crate) struct TabFocus(std::cell::OnceCell<Vec<FocusHandle>>);

impl TabFocus {
    fn handles(&self, slots: usize, cx: &App) -> Vec<FocusHandle> {
        self.0
            .get_or_init(|| (0..slots).map(|_| cx.focus_handle()).collect())
            .clone()
    }

    /// Focus the cell in `slot` (GUI E2E: Tier A cannot press Tab).
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focus(&self, slot: usize, window: &mut Window, cx: &mut App) {
        if let Some(handle) = self.0.get().and_then(|handles| handles.get(slot)) {
            handle.focus(window, cx);
        }
    }

    /// The slot whose cell holds the focus, if any.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<usize> {
        self.0
            .get()?
            .iter()
            .position(|handle| handle.is_focused(window))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activation {
    /// Moving to a cell selects it: selecting is cheap and starts nothing.
    Automatic,
    /// Moving only focuses; Enter/Space select.
    Manual,
}

type Select = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// One tab list being drawn: the slots on screen in order, the selected one
/// (none while something no cell names is on screen), and what selecting a
/// slot does.
pub(crate) struct TabList {
    handles: Vec<FocusHandle>,
    shown: Rc<Vec<usize>>,
    selected: Option<usize>,
    /// The list's one Tab stop: the selected cell, else the first.
    stop: Option<usize>,
    activation: Activation,
    select: Select,
    /// Where focus goes back to after a pointer click: the cell must not keep
    /// it, or the arrows would stop reaching what they did before the click.
    pointer_focus: Option<FocusHandle>,
}

impl TabList {
    /// `slots` handles are kept in `focus`; `shown` lists the slots drawn,
    /// in order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        focus: &TabFocus,
        slots: usize,
        shown: Vec<usize>,
        selected: Option<usize>,
        activation: Activation,
        pointer_focus: Option<FocusHandle>,
        select: impl Fn(usize, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Self {
        let stop = selected
            .filter(|slot| shown.contains(slot))
            .or_else(|| shown.first().copied());
        Self {
            handles: focus.handles(slots, cx),
            shown: Rc::new(shown),
            selected,
            stop,
            activation,
            select: Rc::new(select),
            pointer_focus,
        }
    }

    /// The list's container.
    pub(crate) fn list(&self, el: Stateful<Div>) -> Stateful<Div> {
        el.role(Role::TabList).key_context(CONTEXT)
    }

    /// The cell for `slot`, named `label`.
    pub(crate) fn cell(&self, slot: usize, label: &str, el: Stateful<Div>) -> Stateful<Div> {
        let selected = self.selected == Some(slot);
        let handle = self.handles[slot]
            .clone()
            .tab_index(0)
            .tab_stop(self.stop == Some(slot));
        let go = |target: fn(&[usize], usize) -> Option<usize>| {
            let (shown, handles, select) = (
                self.shown.clone(),
                self.handles.clone(),
                self.select.clone(),
            );
            let automatic = self.activation == Activation::Automatic;
            move |window: &mut Window, cx: &mut App| {
                let Some(next) = target(&shown, slot) else {
                    return;
                };
                handles[next].focus(window, cx);
                if automatic {
                    select(next, window, cx);
                }
            }
        };
        let (prev, next, first, last) = (
            go(|shown, slot| {
                let at = shown.iter().position(|&s| s == slot)?;
                at.checked_sub(1).map(|i| shown[i])
            }),
            go(|shown, slot| {
                let at = shown.iter().position(|&s| s == slot)?;
                shown.get(at + 1).copied()
            }),
            go(|shown, _| shown.first().copied()),
            go(|shown, _| shown.last().copied()),
        );
        let (select, pointer_focus) = (self.select.clone(), self.pointer_focus.clone());
        with_ring(
            el.role(Role::Tab)
                .aria_label(SharedString::from(label.to_string()))
                .aria_selected(selected)
                .track_focus(&handle),
        )
        .on_click(move |event: &ClickEvent, window, cx| {
            select(slot, window, cx);
            if !event.is_keyboard() {
                if let Some(focus) = &pointer_focus {
                    focus.focus(window, cx);
                }
            }
        })
        .on_key_down(stop_activation_keys)
        .on_action(move |_: &TabListPrev, window, cx| prev(window, cx))
        .on_action(move |_: &TabListNext, window, cx| next(window, cx))
        .on_action(move |_: &TabListFirst, window, cx| first(window, cx))
        .on_action(move |_: &TabListLast, window, cx| last(window, cx))
    }
}

/// A row that Tab reaches and Enter/Space press (its `on_click`).
pub(crate) fn focusable_row(el: Stateful<Div>) -> Stateful<Div> {
    with_ring(el.tab_index(0)).on_key_down(stop_activation_keys)
}

fn with_ring(el: Stateful<Div>) -> Stateful<Div> {
    el.border_2()
        .border_color(transparent_black())
        .focus_visible(|s| s.border_color(rgb(theme().color_branch)))
}

/// Enter / Space press the focused element (gpui's keyboard click, already
/// armed by the time this runs); nothing above it may act on them too.
fn stop_activation_keys(event: &KeyDownEvent, _: &mut Window, cx: &mut App) {
    let stroke = &event.keystroke;
    if (stroke.key == "enter" || stroke.key == "space") && !stroke.modifiers.modified() {
        cx.stop_propagation();
    }
}
