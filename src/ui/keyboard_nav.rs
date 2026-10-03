//! Keyboard paths for Kagi's hand-made tab lists and rows (#944).
//!
//! A tab list is one Tab stop (roving tabindex): the cell the arrows last
//! moved to while the focus is in the list, else the selected cell — so Tab
//! and Shift+Tab leave the list in one press from wherever the arrows put the
//! focus, and coming back in lands on the selected cell (#960 review). The
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
//! A row list (a virtualized `gpui::list`) is one Tab stop too: the row last
//! focused while it is drawn, else the first row on screen. ↑/↓ move to the
//! neighbouring row, scrolling it into view first (#959). Its rows' focus
//! handles live in app state by each row's key, so a row scrolled out of view
//! keeps its handle, and a focused row that leaves the list (a filter, a read
//! landing) hands the focus on rather than leaving it nowhere.
//!
//! The focus ring is the one `Input` draws (`sync_gpui_component_theme` maps
//! gpui-component's `ring` to `color_branch`), shown only for keyboard focus
//! (`focus_visible`). It is a border kept transparent at rest, so taking focus
//! does not move anything; callers pad with [`inset`], which takes its width
//! out of their zoomed padding.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gpui::{
    actions, prelude::*, rgb, transparent_black, App, ClickEvent, Div, FocusHandle, KeyBinding,
    KeyDownEvent, ListState, Role, SharedString, Stateful, Window,
};

use super::theme::theme;

actions!(
    kagi_tab_list,
    [TabListPrev, TabListNext, TabListFirst, TabListLast]
);

actions!(kagi_row_list, [RowListPrev, RowListNext]);

const ROW_CONTEXT: &str = "KagiRowList";

const CONTEXT: &str = "KagiTabList";

/// The ring's width in px (`border_2`). Not scaled with the UI zoom: a fixed
/// 2px ring stays crisp and visible at 0.7×, where a scaled one would be a
/// blurred 1.4px.
pub(crate) const RING: f32 = 2.;

/// Padding of `n` (px at zoom 1.0) for an element that carries the ring: the
/// zoomed padding minus the ring's fixed width, so the element's outer size is
/// the one `scaled_px(n)` padding gave before the ring, at every zoom (#960
/// review). Subtracting the ring before scaling made a cell 2 − 2·zoom px off:
/// smaller at 1.5×, larger at 0.7×. The smallest inset in use is
/// `inset(4.)` at 0.7×, 0.8px.
pub(crate) fn inset(n: f32) -> gpui::Pixels {
    super::theme::scaled_px(n) - gpui::px(RING)
}

/// ←/→/Home/End inside a tab list. The list's context is deeper than the
/// app-wide `!Terminal && !Input` arrows (PR mode's pane cycling), so these
/// win only while a cell holds the focus.
///
/// Tab and Shift+Tab leave a tab list from wherever the arrows put the focus
/// (that cell is the Tab stop of the frame on screen), so the next frame's
/// stop goes back to the selected cell: they are watched before any binding
/// runs, and every list forgets its arrowed-to cell.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", TabListPrev, Some(CONTEXT)),
        KeyBinding::new("right", TabListNext, Some(CONTEXT)),
        KeyBinding::new("home", TabListFirst, Some(CONTEXT)),
        KeyBinding::new("end", TabListLast, Some(CONTEXT)),
    ]);
    // The row list's ↑/↓ outrank the app-wide `!Terminal && !Input` up/down
    // (stepping through a diff's files) the same way, and only inside a list.
    cx.bind_keys([
        KeyBinding::new("up", RowListPrev, Some(ROW_CONTEXT)),
        KeyBinding::new("down", RowListNext, Some(ROW_CONTEXT)),
    ]);
    cx.intercept_keystrokes(|event, _, _| {
        if event.keystroke.key == "tab" {
            ROVING.with(|lists| {
                lists.borrow_mut().retain(|roving| match roving.upgrade() {
                    Some(roving) => {
                        roving.set(None);
                        true
                    }
                    None => false,
                })
            });
        }
    })
    .detach();
}

thread_local! {
    /// Every tab list's arrowed-to cell, for the Tab watch above.
    static ROVING: RefCell<Vec<Weak<Cell<Option<usize>>>>> = const { RefCell::new(Vec::new()) };
}

/// The focus handles of one tab list's cells, one per slot, made as slots
/// appear and kept across frames; and the cell the arrows moved to, until
/// Tab leaves the list or a cell is clicked.
pub(crate) struct TabFocus {
    handles: RefCell<Vec<FocusHandle>>,
    roving: Rc<Cell<Option<usize>>>,
    /// Handles of cells closed since the last frame (see [`Self::closing`]).
    closed: RefCell<Vec<FocusHandle>>,
}

impl Default for TabFocus {
    fn default() -> Self {
        let roving = Rc::new(Cell::new(None));
        ROVING.with(|lists| lists.borrow_mut().push(Rc::downgrade(&roving)));
        Self {
            handles: RefCell::default(),
            roving,
            closed: RefCell::default(),
        }
    }
}

impl TabFocus {
    fn handles(&self, slots: usize, cx: &App) -> Vec<FocusHandle> {
        let mut handles = self.handles.borrow_mut();
        while handles.len() < slots {
            handles.push(cx.focus_handle());
        }
        handles[..slots].to_vec()
    }

    /// The cell in `slot` is about to be closed. The cells after it move up
    /// a slot, so its handle is drawn again for the next cell: kept here, it
    /// lets [`Self::release_from`] tell a focus left on the closed cell from
    /// one on the cell that took its place (#961 review).
    pub(crate) fn closing(&self, slot: usize) {
        if let Some(handle) = self.handles.borrow().get(slot) {
            self.closed.borrow_mut().push(handle.clone());
        }
    }

    /// Only the first `slots` cells are drawn now (a tab closed, or the list
    /// is gone), and the cells marked [`Self::closing`] are gone: one of them
    /// holding the focus would keep it with nothing on screen tracking it,
    /// out of reach of the window's keys, or pass it to the next cell, so
    /// the focus goes to `fallback`.
    pub(crate) fn release_from(
        &self,
        slots: usize,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let closed_focused = {
            let mut closed = self.closed.borrow_mut();
            let focused = closed.iter().any(|handle| handle.is_focused(window));
            closed.clear();
            focused
        };
        let lost = closed_focused
            || self
                .handles
                .borrow()
                .iter()
                .skip(slots)
                .any(|handle| handle.is_focused(window));
        if let Some(fallback) = fallback.filter(|_| lost) {
            fallback.focus(window, cx);
        }
    }

    /// Focus the cell in `slot` (GUI E2E: Tier A cannot press Tab).
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focus(&self, slot: usize, window: &mut Window, cx: &mut App) {
        let handle = self.handles.borrow().get(slot).cloned();
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }

    /// The slot whose cell holds the focus, if any.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<usize> {
        self.handles
            .borrow()
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
    /// The list's one Tab stop: the cell the arrows moved to, else the
    /// selected cell, else the first.
    stop: Option<usize>,
    roving: Rc<Cell<Option<usize>>>,
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
        let stop = [focus.roving.get(), selected]
            .into_iter()
            .flatten()
            .find(|slot| shown.contains(slot))
            .or_else(|| shown.first().copied());
        Self {
            handles: focus.handles(slots, cx),
            shown: Rc::new(shown),
            selected,
            stop,
            roving: focus.roving.clone(),
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
            let roving = self.roving.clone();
            let automatic = self.activation == Activation::Automatic;
            move |window: &mut Window, cx: &mut App| {
                let Some(next) = target(&shown, slot) else {
                    return;
                };
                handles[next].focus(window, cx);
                roving.set(Some(next));
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
        let roving = self.roving.clone();
        with_ring(
            el.role(Role::Tab)
                .aria_label(SharedString::from(label.to_string()))
                .aria_selected(selected)
                .track_focus(&handle),
        )
        .on_click(move |event: &ClickEvent, window, cx| {
            select(slot, window, cx);
            if !event.is_keyboard() {
                roving.set(None);
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

/// The rows of one virtualized list as built with its entries (#959): each
/// row's key and its entry in the list. Built only when the entries are, so
/// a frame that draws the same rows hands the same `Rc` back.
pub(crate) type RowKeys = Rc<[(String, usize)]>;

/// The focus handles of one virtualized list's rows and the row last focused
/// (#959). The handles are made again only when the rows change — not on
/// every frame (#937) — and a row keeps its handle while its key stays.
#[derive(Default)]
pub(crate) struct RowFocus {
    keys: RowKeys,
    handles: Rc<[FocusHandle]>,
    /// The window's focus when last looked, so the rows are searched only
    /// when it moves or the rows change.
    seen: Option<FocusHandle>,
    /// The row last focused, as its place among `keys`.
    current: Option<usize>,
}

impl RowFocus {
    /// Bring the handles in line with `keys` and pick the list's Tab stop. A
    /// focused row that has left the list hands the focus to that stop, or
    /// to `fallback` when no row is left.
    pub(crate) fn rows(
        &mut self,
        keys: RowKeys,
        state: &ListState,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> RowList {
        let focused = window.focused(cx);
        let moved = focused != self.seen;
        let rebuild = !Rc::ptr_eq(&keys, &self.keys);
        let focused_row = if moved || rebuild {
            focused
                .as_ref()
                .and_then(|focus| self.handles.iter().position(|handle| handle == focus))
        } else {
            None
        };
        if moved {
            if focused_row.is_some() {
                self.current = focused_row;
            }
            self.seen = focused;
        }
        let mut lost = false;
        if rebuild {
            let old: HashMap<&str, &FocusHandle> = self
                .keys
                .iter()
                .zip(self.handles.iter())
                .map(|((key, _), handle)| (key.as_str(), handle))
                .collect();
            let handles: Rc<[FocusHandle]> = keys
                .iter()
                .map(|(key, _)| match old.get(key.as_str()) {
                    Some(handle) => (*handle).clone(),
                    None => cx.focus_handle(),
                })
                .collect();
            let current = self.current.and_then(|at| {
                let key = &self.keys[at].0;
                keys.iter().position(|(k, _)| k == key)
            });
            lost = focused_row.is_some() && current.is_none();
            (self.keys, self.handles, self.current) = (keys, handles, current);
        }
        // The remembered row while it is drawn; else the first row on
        // screen, so Tab always finds a row that exists.
        let top = state.logical_scroll_top().item_ix;
        let stop = self
            .current
            .filter(|&at| state.bounds_for_item(self.keys[at].1).is_some())
            .or_else(|| {
                let at = self.keys.partition_point(|&(_, ix)| ix < top);
                (at < self.keys.len()).then_some(at)
            })
            .or((!self.keys.is_empty()).then_some(0));
        if lost {
            match stop {
                Some(at) => self.handles[at].focus(window, cx),
                None => {
                    if let Some(fallback) = fallback {
                        fallback.focus(window, cx);
                    }
                }
            }
            self.current = stop;
        }
        RowList {
            rows: self.keys.clone(),
            handles: self.handles.clone(),
            stop,
            state: state.clone(),
        }
    }

    /// The list is not drawn (still loading, or failed): no row is left, so
    /// a focused row hands the focus to `fallback`, and the rows are
    /// forgotten.
    pub(crate) fn release(
        &mut self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.handles.iter().any(|handle| handle.is_focused(window)) {
            if let Some(fallback) = fallback {
                fallback.focus(window, cx);
            }
        }
        *self = Self::default();
    }

    /// The list is not on screen for now (Home is not in front): a focused
    /// row hands the focus to `fallback`, and the rows and the remembered
    /// row stay for when the list is drawn again. No allocation: it runs on
    /// every frame Home is away.
    pub(crate) fn yield_focus(
        &self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.handles.iter().any(|handle| handle.is_focused(window)) {
            if let Some(fallback) = fallback {
                fallback.focus(window, cx);
            }
        }
    }

    /// The key of the row holding the focus, if any.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<String> {
        self.keys
            .iter()
            .zip(self.handles.iter())
            .find(|(_, handle)| handle.is_focused(window))
            .map(|((key, _), _)| key.clone())
    }

    /// Focus the row `key` (GUI E2E: Tier A cannot press Tab into a list
    /// without walking the whole window).
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focus(&self, key: &str, window: &mut Window, cx: &mut App) {
        if let Some(at) = self.keys.iter().position(|(k, _)| k == key) {
            self.handles[at].focus(window, cx);
        }
    }
}

/// One virtualized list's rows being drawn.
pub(crate) struct RowList {
    rows: RowKeys,
    handles: Rc<[FocusHandle]>,
    stop: Option<usize>,
    state: ListState,
}

impl RowList {
    /// The list's container.
    pub(crate) fn list(&self, el: Stateful<Div>) -> Stateful<Div> {
        el.key_context(ROW_CONTEXT)
    }

    /// How many rows the list has.
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The row at `at` (its place among the rows): Tab reaches it when it is
    /// the list's stop, ↑/↓ move from it, Enter/Space press it (`on_click`).
    pub(crate) fn row(&self, at: usize, el: Stateful<Div>) -> Stateful<Div> {
        let handle = self.handles[at]
            .clone()
            .tab_index(0)
            .tab_stop(self.stop == Some(at));
        let step = |delta: isize| {
            let (rows, handles, state) =
                (self.rows.clone(), self.handles.clone(), self.state.clone());
            move |window: &mut Window, cx: &mut App| {
                let Some(to) = at.checked_add_signed(delta).filter(|&to| to < rows.len()) else {
                    return;
                };
                state.scroll_to_reveal_item(rows[to].1);
                handles[to].focus(window, cx);
            }
        };
        let (prev, next) = (step(-1), step(1));
        with_ring(el.track_focus(&handle))
            .on_key_down(stop_activation_keys)
            .on_action(move |_: &RowListPrev, window, cx| prev(window, cx))
            .on_action(move |_: &RowListNext, window, cx| next(window, cx))
    }
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
