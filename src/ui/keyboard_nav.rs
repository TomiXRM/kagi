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
//! A row list (a virtualized `gpui::list` or `uniform_list`, see
//! [`RowScroll`]) is one Tab stop too: the row last focused while it is
//! drawn, else the first row on screen. ↑/↓ move to the
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
    KeyDownEvent, ListState, Role, ScrollStrategy, SharedString, Stateful, UniformListScrollHandle,
    Window,
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
                lists
                    .borrow_mut()
                    .retain(|list| match list.roving.upgrade() {
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

/// One tab list as [`ROVING`] sees it.
struct Watched {
    roving: Weak<Cell<Option<u64>>>,
    cells: Weak<RefCell<Vec<(u64, FocusHandle)>>>,
}

thread_local! {
    /// Every tab list's arrowed-to cell, for the Tab watch above and
    /// [`forget_roving_without_focus`].
    static ROVING: RefCell<Vec<Watched>> = const { RefCell::new(Vec::new()) };
}

/// A list's arrowed-to cell is its Tab stop only while that cell holds the
/// focus. Focus that left another way than Tab — a pointer click on another
/// control — would otherwise bring the next Tab back to a cell the user
/// never selected (#968), so the stop goes back to the selected cell. Called
/// at the top of every frame; it reads only lists with an arrowed-to cell.
pub(crate) fn forget_roving_without_focus(window: &Window) {
    ROVING.with(|lists| {
        for list in lists.borrow().iter() {
            let Some(roving) = list.roving.upgrade() else {
                continue;
            };
            let Some(key) = roving.get() else {
                continue;
            };
            let focused = list.cells.upgrade().is_some_and(|cells| {
                cells
                    .borrow()
                    .iter()
                    .any(|(k, handle)| *k == key && handle.is_focused(window))
            });
            if !focused {
                roving.set(None);
            }
        }
    });
}

/// The focus handles of one tab list's cells, each kept with its cell's
/// identity across frames — a slot number for a list of fixed cells, the
/// tab's own identity for the repository strip — and the identity of the
/// cell the arrows moved to, while it holds the focus. A cell that moves to
/// another slot (a tab added before it, or closed before it) keeps its
/// handle, a focus on it and the arrowed-to mark, with nothing to adjust
/// (#961 review). A cell that goes away keeps its handle until
/// [`Self::release_closed`], which hands a focus left on it to the window.
pub(crate) struct TabFocus {
    cells: Rc<RefCell<Vec<(u64, FocusHandle)>>>,
    roving: Rc<Cell<Option<u64>>>,
    /// Handles of cells gone since the last frame.
    closed: RefCell<Vec<FocusHandle>>,
}

impl Default for TabFocus {
    fn default() -> Self {
        let roving = Rc::new(Cell::new(None));
        let cells = Rc::new(RefCell::new(Vec::new()));
        ROVING.with(|lists| {
            lists.borrow_mut().push(Watched {
                roving: Rc::downgrade(&roving),
                cells: Rc::downgrade(&cells),
            })
        });
        Self {
            cells,
            roving,
            closed: RefCell::default(),
        }
    }
}

impl TabFocus {
    /// The handles of the cells `keys` names, in that order: a known cell
    /// keeps its handle, a new one gets one, and a cell no longer named
    /// leaves (its handle kept for [`Self::release_closed`]).
    pub(crate) fn sync(&self, keys: &[u64], cx: &App) -> Vec<FocusHandle> {
        let mut cells = self.cells.borrow_mut();
        let same =
            cells.len() == keys.len() && cells.iter().zip(keys).all(|((k, _), key)| k == key);
        if !same {
            let mut old = std::mem::take(&mut *cells);
            for &key in keys {
                let handle = match old.iter().position(|(k, _)| *k == key) {
                    Some(at) => old.swap_remove(at).1,
                    None => cx.focus_handle(),
                };
                cells.push((key, handle));
            }
            self.closed
                .borrow_mut()
                .extend(old.into_iter().map(|(_, handle)| handle));
            if self.roving.get().is_some_and(|key| !keys.contains(&key)) {
                self.roving.set(None);
            }
        }
        cells.iter().map(|(_, handle)| handle.clone()).collect()
    }

    /// A focus left on a cell that went away since the last frame (a tab
    /// closed, or the list gone) would stay with nothing on screen tracking
    /// it, out of reach of the window's keys: it goes to `fallback`.
    pub(crate) fn release_closed(
        &self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let lost = {
            let mut closed = self.closed.borrow_mut();
            let lost = closed.iter().any(|handle| handle.is_focused(window));
            closed.clear();
            lost
        };
        if let Some(fallback) = fallback.filter(|_| lost) {
            fallback.focus(window, cx);
        }
    }

    /// The cell's handle holding the focus, if any.
    pub(crate) fn focused_cell(&self, window: &Window) -> Option<FocusHandle> {
        self.cells
            .borrow()
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(_, handle)| handle.clone())
    }

    /// The list is not drawn for now (Home is not in front): a focus on any
    /// of its cells goes to `fallback`.
    pub(crate) fn yield_focus(
        &self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let held = self
            .cells
            .borrow()
            .iter()
            .any(|(_, handle)| handle.is_focused(window));
        if let Some(fallback) = fallback.filter(|_| held) {
            fallback.focus(window, cx);
        }
    }

    /// Focus the cell in `slot` as last drawn (GUI E2E: Tier A cannot press
    /// Tab).
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focus(&self, slot: usize, window: &mut Window, cx: &mut App) {
        let handle = self.cells.borrow().get(slot).map(|(_, h)| h.clone());
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }

    /// The slot, as last drawn, whose cell holds the focus, if any.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<usize> {
        self.cells
            .borrow()
            .iter()
            .position(|(_, handle)| handle.is_focused(window))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activation {
    /// Moving to a cell selects it: selecting is cheap and starts nothing.
    Automatic,
    /// Moving only focuses; Enter/Space select.
    Manual,
}

/// The identities of `n` cells that never move: their slot numbers.
pub(crate) fn slot_keys(n: usize) -> Vec<u64> {
    (0..n as u64).collect()
}

type Select = Rc<dyn Fn(usize, &mut Window, &mut App)>;

/// One tab list being drawn: the slots on screen in order, the selected one
/// (none while something no cell names is on screen), and what selecting a
/// slot does.
pub(crate) struct TabList {
    handles: Vec<FocusHandle>,
    /// Each slot's cell identity (see [`TabFocus`]).
    keys: Rc<Vec<u64>>,
    shown: Rc<Vec<usize>>,
    selected: Option<usize>,
    /// The list's one Tab stop: the cell the arrows moved to, else the
    /// selected cell, else the first.
    stop: Option<usize>,
    roving: Rc<Cell<Option<u64>>>,
    activation: Activation,
    select: Select,
    /// Where focus goes back to after a pointer click: the cell must not keep
    /// it, or the arrows would stop reaching what they did before the click.
    pointer_focus: Option<FocusHandle>,
}

impl TabList {
    /// One cell per entry of `keys`, its identity (a slot number when the
    /// cells never move: [`slot_keys`]); their handles are kept in `focus`.
    /// `shown` lists the slots drawn, in order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        focus: &TabFocus,
        keys: Vec<u64>,
        shown: Vec<usize>,
        selected: Option<usize>,
        activation: Activation,
        pointer_focus: Option<FocusHandle>,
        select: impl Fn(usize, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Self {
        let handles = focus.sync(&keys, cx);
        let roving = focus
            .roving
            .get()
            .and_then(|key| keys.iter().position(|&k| k == key));
        let stop = [roving, selected]
            .into_iter()
            .flatten()
            .find(|slot| shown.contains(slot))
            .or_else(|| shown.first().copied());
        Self {
            handles,
            keys: Rc::new(keys),
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
            let (shown, handles, keys, select) = (
                self.shown.clone(),
                self.handles.clone(),
                self.keys.clone(),
                self.select.clone(),
            );
            let roving = self.roving.clone();
            let automatic = self.activation == Activation::Automatic;
            move |window: &mut Window, cx: &mut App| {
                let Some(next) = target(&shown, slot) else {
                    return;
                };
                handles[next].focus(window, cx);
                roving.set(Some(keys[next]));
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

/// How a row list scrolls and what it drew last frame: a `gpui::list`'s
/// [`ListState`], or a `uniform_list`'s handle with the range of items its
/// processor was last asked for (#981) — a `uniform_list` keeps no bounds per
/// item, but it draws exactly the items it asks for. Cheap to clone.
#[derive(Clone)]
pub(crate) enum RowScroll {
    List(ListState),
    Uniform(UniformListScrollHandle, std::ops::Range<usize>),
}

impl RowScroll {
    /// The entry at the top of the list's viewport.
    fn top(&self) -> usize {
        match self {
            Self::List(state) => state.logical_scroll_top().item_ix,
            // The first item it drew is the one at the top (the handle's own
            // `logical_scroll_top_index` is test-only in gpui).
            Self::Uniform(_, drawn) => drawn.start,
        }
    }

    /// Whether entry `ix` was drawn last frame.
    fn drawn(&self, ix: usize) -> bool {
        match self {
            Self::List(state) => state.bounds_for_item(ix).is_some(),
            Self::Uniform(_, drawn) => drawn.contains(&ix),
        }
    }

    /// Scroll entry `ix` into view, as little as it takes.
    fn reveal(&self, ix: usize) {
        match self {
            Self::List(state) => state.scroll_to_reveal_item(ix),
            // Not strict: it scrolls only when the item is out of view.
            Self::Uniform(handle, _) => handle.scroll_to_item(ix, ScrollStrategy::Top),
        }
    }
}

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
    /// The focus is in [`Self::current`] — on the row or on a control inside
    /// it — as of the last time the focus moved.
    holds: bool,
    /// The list was not drawn last frame (see [`Self::yield_focus`]).
    away: bool,
    /// A focused row left the list on a rebuild frame; the next frame, which
    /// has bounds, hands the focus on (see [`Self::rows`]).
    pending: bool,
}

impl RowFocus {
    /// Bring the handles in line with `keys` and pick the list's Tab stop. A
    /// focused row that has left the list hands the focus to that stop, or
    /// to `fallback` when no row is left.
    pub(crate) fn rows(
        &mut self,
        keys: RowKeys,
        scroll: &RowScroll,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> RowList {
        self.away = false;
        let focused = window.focused(cx);
        let moved = focused != self.seen;
        let rebuild = !Rc::ptr_eq(&keys, &self.keys);
        // The row holding the focus: the row itself (a row ↓ just focused is
        // not drawn yet, so match its handle), or a control inside a drawn
        // row, its Open button (#961 review).
        let focused_row = if moved || rebuild {
            focused.as_ref().and_then(|focus| {
                self.handles
                    .iter()
                    .position(|handle| handle == focus)
                    .or_else(|| {
                        self.handles
                            .iter()
                            .position(|handle| handle.contains_focused(window, cx))
                    })
            })
        } else {
            None
        };
        if moved {
            if focused_row.is_some() {
                self.current = focused_row;
            }
            self.holds = focused_row.is_some();
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
        // screen, so Tab always finds a row that exists. None when no row is
        // on screen (#961 review): only headings or notes are, after the
        // rows or before the next ones. The first row at or after the top
        // entry is the only candidate — a later one is further down — and it
        // counts only if drawn.
        let top = scroll.top();
        let first = self.keys.partition_point(|&(_, ix)| ix < top);
        // On the frame the rows were rebuilt the list has no bounds yet (it
        // was reset or put back at its scroll top, and lays the rows out in
        // this frame), so no row is known to be drawn: the list has no stop
        // and nothing moves (#961 review — the top may hold only headings and
        // notes). The next frame, asked for here, has bounds and reconciles:
        // the stop is the remembered row if drawn, else the first drawn row,
        // and a focus whose row left the list moves there (or to the window).
        if rebuild {
            window.request_animation_frame();
        }
        let drawn = |at: usize| !rebuild && scroll.drawn(self.keys[at].1);
        // The row holding the focus — itself or a control inside it, its
        // Open button (#961 review) — scrolled out of the drawn range by the
        // wheel is unmounted: no ↑/↓, no ring. Not on the frame the focus
        // moved, as ↓ focuses a row the list only draws on the next one, nor
        // on a rebuild frame, which cannot tell what is drawn yet.
        let scrolled_away =
            !moved && !rebuild && self.holds && self.current.is_some_and(|at| !drawn(at));
        let stop = self
            .current
            .filter(|&at| drawn(at))
            .or_else(|| (first < self.keys.len() && drawn(first)).then_some(first));
        if rebuild {
            self.pending |= lost;
        } else if std::mem::take(&mut self.pending) || scrolled_away {
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
            scroll: scroll.clone(),
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

    /// The list is not on screen for now (Home is not in front). On the
    /// first frame away the frame on screen is still the list's, so a focus
    /// on a row or on a control inside one (a row's Open button, #961
    /// review) is found there and handed to `fallback`. The rows and the
    /// remembered row stay for when the list is drawn again. Later frames
    /// away do nothing: no row is drawn for the focus to reach.
    pub(crate) fn yield_focus(
        &mut self,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if std::mem::replace(&mut self.away, true) {
            return;
        }
        if self
            .handles
            .iter()
            .any(|handle| handle.contains_focused(window, cx))
        {
            if let Some(fallback) = fallback {
                fallback.focus(window, cx);
            }
        }
    }

    /// Whether the focus is on a row or a control inside one, in the frame
    /// on screen.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.handles
            .iter()
            .any(|handle| handle.contains_focused(window, cx))
    }

    /// The key of the row holding the focus, if any.
    pub(crate) fn focused(&self, window: &Window) -> Option<&str> {
        self.keys
            .iter()
            .zip(self.handles.iter())
            .find(|(_, handle)| handle.is_focused(window))
            .map(|((key, _), _)| key.as_str())
    }

    /// Focus the first row, scrolling it into view (a pane opened from the
    /// keyboard, #981). False when there is no row.
    pub(crate) fn focus_first(
        &self,
        scroll: &RowScroll,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some((handle, (_, ix))) = self.handles.first().zip(self.keys.first()) else {
            return false;
        };
        scroll.reveal(*ix);
        handle.focus(window, cx);
        true
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
    scroll: RowScroll,
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

    /// Whether a drawn row is the list's Tab stop this frame (none while no
    /// row is known to be on screen).
    pub(crate) fn has_stop(&self) -> bool {
        self.stop.is_some()
    }

    /// The row at `at` (its place among the rows): Tab reaches it when it is
    /// the list's stop, ↑/↓ move from it, Enter/Space press it (`on_click`).
    pub(crate) fn row(&self, at: usize, el: Stateful<Div>) -> Stateful<Div> {
        let handle = self.handles[at]
            .clone()
            .tab_index(0)
            .tab_stop(self.stop == Some(at));
        let step = |delta: isize| {
            let (rows, handles, scroll) =
                (self.rows.clone(), self.handles.clone(), self.scroll.clone());
            move |window: &mut Window, cx: &mut App| {
                let Some(to) = at.checked_add_signed(delta).filter(|&to| to < rows.len()) else {
                    return;
                };
                scroll.reveal(rows[to].1);
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

/// A control that Tab reaches and Enter/Space press (its `on_click`).
pub(crate) fn focusable(el: Stateful<Div>) -> Stateful<Div> {
    with_ring(el.tab_index(0)).on_key_down(stop_activation_keys)
}

/// Focus handles for a fixed set of controls, one per slot, made as slots
/// appear and kept across frames. Cheap to clone: a renderer that may not
/// read `KagiApp` (it runs during the app's own update) is handed a clone.
#[derive(Clone, Default)]
pub struct FocusSlots(Rc<RefCell<Vec<FocusHandle>>>);

impl FocusSlots {
    /// The handle of `slot`.
    pub(crate) fn get(&self, slot: usize, cx: &App) -> FocusHandle {
        let mut handles = self.0.borrow_mut();
        while handles.len() <= slot {
            handles.push(cx.focus_handle());
        }
        handles[slot].clone()
    }

    /// The slot holding the focus, if any.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focused(&self, window: &Window) -> Option<usize> {
        self.0
            .borrow()
            .iter()
            .position(|handle| handle.is_focused(window))
    }

    /// Focus `slot` (GUI E2E: Tier A cannot press Tab).
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn focus(&self, slot: usize, window: &mut Window, cx: &mut App) {
        let handle = self.0.borrow().get(slot).cloned();
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }
}

/// A switch the keyboard and assistive technology reach (#970). gpui-
/// component's `Switch` draws it and keeps its pointer handling; around it a
/// Tab stop (`focus`) named `label`, with the `Switch` role and its
/// checked state, which Enter / Space flip (gpui's keyboard click). Only a
/// keyboard click is taken here: a pointer click is the `Switch`'s, and
/// taking it too would write (and reload) the same new state twice.
/// `on_toggle` gets the new state. The ring sits in a negative
/// margin, so the control takes the room it took before.
pub(crate) fn switch(
    id: &'static str,
    focus: FocusHandle,
    label: SharedString,
    checked: bool,
    on_toggle: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let toggled = if checked {
        gpui::Toggled::True
    } else {
        gpui::Toggled::False
    };
    // What assistive technology is given, for Tier A.
    #[cfg(feature = "gui-e2e")]
    record_switch(id, &label, toggled == gpui::Toggled::True);
    let on_toggle = Rc::new(on_toggle);
    let pointer = on_toggle.clone();
    let inner = gpui_component::switch::Switch::new(id)
        .checked(checked)
        .on_click(move |next: &bool, window, cx| pointer(*next, window, cx));
    let wrapper = with_ring(
        gpui::div()
            .id(SharedString::from(format!("{id}-key")))
            .track_focus(&focus.tab_index(0).tab_stop(true)),
    )
    .m(-gpui::px(RING))
    .rounded_full()
    .role(Role::Switch)
    .aria_label(label)
    .aria_toggled(toggled)
    .on_key_down(stop_activation_keys)
    .on_click(move |event: &ClickEvent, window, cx| {
        if event.is_keyboard() {
            on_toggle(!checked, window, cx);
        }
    })
    .child(inner);
    super::e2e::measure_control(format!("{id}-key"), wrapper)
}

// The switches as last drawn: id → (label, checked), for Tier A (#970).
#[cfg(feature = "gui-e2e")]
thread_local! {
    static SWITCHES: RefCell<std::collections::HashMap<&'static str, (String, bool)>> =
        RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
fn record_switch(id: &'static str, label: &str, checked: bool) {
    SWITCHES.with(|m| m.borrow_mut().insert(id, (label.to_string(), checked)));
}

/// What the last drawn frame named switch `id` and the checked state it
/// gave assistive technology.
#[cfg(feature = "gui-e2e")]
pub(crate) fn recorded_switch(id: &str) -> Option<(String, bool)> {
    SWITCHES.with(|m| m.borrow().get(id).cloned())
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
