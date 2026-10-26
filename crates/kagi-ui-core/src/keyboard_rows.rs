//! Shared retained focus and reveal for virtualized row lists.
use super::{stop_activation_keys, with_ring};
use gpui::{
    actions, prelude::*, App, Div, FocusHandle, KeyBinding, ListOffset, ListState, ScrollStrategy,
    Stateful, UniformListScrollHandle, Window,
};
use std::{collections::HashMap, rc::Rc};
actions!(
    kagi_row_list,
    [
        RowListPrev,
        RowListNext,
        RowListFirst,
        RowListLast,
        RowListPageUp,
        RowListPageDown
    ]
);
const ROW_CONTEXT: &str = "KagiRowList";
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", RowListPrev, Some(ROW_CONTEXT)),
        KeyBinding::new("down", RowListNext, Some(ROW_CONTEXT)),
        KeyBinding::new("home", RowListFirst, Some(ROW_CONTEXT)),
        KeyBinding::new("end", RowListLast, Some(ROW_CONTEXT)),
        KeyBinding::new("pageup", RowListPageUp, Some(ROW_CONTEXT)),
        KeyBinding::new("pagedown", RowListPageDown, Some(ROW_CONTEXT)),
        KeyBinding::new("cmd-up", RowListFirst, Some(ROW_CONTEXT)),
        KeyBinding::new("cmd-down", RowListLast, Some(ROW_CONTEXT)),
    ]);
}

/// The rows of one virtualized list as built with its entries (#959): each
/// row's key and its entry in the list. Built only when the entries are, so
/// a frame that draws the same rows hands the same `Rc` back.
pub type RowKeys = Rc<[(String, usize)]>;

/// How a row list scrolls and what it drew last frame: a `gpui::list`'s
/// [`ListState`], a `uniform_list`'s handle with the range of items its
/// processor was last asked for (#981), or Kagi-owned variable-height geometry.
/// A `uniform_list` keeps no per-item bounds, but draws exactly the items it
/// asks for. Cheap to clone.
#[derive(Clone)]
pub enum RowScroll {
    List(ListState),
    Uniform(UniformListScrollHandle, std::ops::Range<usize>),
    /// A uniform list whose last focusable row reveals a trailing note too.
    UniformWithTail {
        handle: UniformListScrollHandle,
        drawn: std::ops::Range<usize>,
        last_row: usize,
        tail: usize,
    },
    Owned(crate::variable_list::VariableListState),
}

/// Reveal a lazy-list row, anchoring an unmeasured destination before layout.
pub fn reveal_list_row(state: &ListState, ix: usize) {
    state.scroll_to_reveal_item(ix);
    if state.bounds_for_item(ix).is_none() {
        state.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: gpui::px(0.),
        });
    }
}

impl RowScroll {
    /// The entry at the top of the list's viewport.
    fn top(&self) -> usize {
        match self {
            Self::List(state) => state.logical_scroll_top().item_ix,
            // The first item it drew is the one at the top (the handle's own
            // `logical_scroll_top_index` is test-only in gpui).
            Self::Uniform(_, drawn) | Self::UniformWithTail { drawn, .. } => drawn.start,
            Self::Owned(state) => state.logical_scroll_top().item_ix,
        }
    }

    /// Whether entry `ix` was drawn last frame.
    fn drawn(&self, ix: usize) -> bool {
        match self {
            Self::List(state) => state.bounds_for_item(ix).is_some(),
            Self::Uniform(_, drawn) | Self::UniformWithTail { drawn, .. } => drawn.contains(&ix),
            Self::Owned(state) => state.bounds_for_item(ix).is_some(),
        }
    }

    /// Scroll entry `ix` into view, as little as it takes.
    fn reveal(&self, ix: usize) {
        match self {
            Self::List(state) => reveal_list_row(state, ix),
            // Not strict: it scrolls only when the item is out of view.
            Self::Uniform(handle, _) => handle.scroll_to_item(ix, ScrollStrategy::Top),
            Self::UniformWithTail {
                handle,
                last_row,
                tail,
                ..
            } => {
                handle.scroll_to_item(
                    if ix == *last_row { *tail } else { ix },
                    ScrollStrategy::Top,
                );
            }
            Self::Owned(state) => state.scroll_to_reveal_item(ix),
        }
    }

    /// How far Page Up / Page Down move, counted from `first`, the first row
    /// at the top.
    fn page_size(&self, keys: &[(String, usize)], first: usize) -> usize {
        match self {
            Self::List(state) => {
                let viewport = state.viewport_bounds();
                page(
                    keys.iter()
                        .skip(first)
                        .filter_map(|(_, ix)| state.bounds_for_item(*ix))
                        .take_while(|bounds| bounds.top() < viewport.bottom())
                        .filter(|bounds| bounds.bottom() > viewport.top())
                        .count(),
                )
            }
            Self::Uniform(_, drawn) | Self::UniformWithTail { drawn, .. } => {
                uniform_page(keys, first, drawn)
            }
            Self::Owned(state) => {
                let viewport = state.viewport_bounds();
                page(
                    keys.iter()
                        .skip(first)
                        .filter_map(|(_, ix)| state.bounds_for_item(*ix))
                        .take_while(|bounds| bounds.top() < viewport.bottom())
                        .filter(|bounds| bounds.bottom() > viewport.top())
                        .count(),
                )
            }
        }
    }
}

/// A page: the rows on screen less one, at least one.
fn page(visible: usize) -> usize {
    visible.saturating_sub(1).max(1)
}

/// A page of a `uniform_list` that drew the items `drawn`.
fn uniform_page(keys: &[(String, usize)], first: usize, drawn: &std::ops::Range<usize>) -> usize {
    page(
        keys.iter()
            .skip(first)
            .take_while(|(_, ix)| drawn.contains(ix))
            .count(),
    )
}

/// The focus handles of one virtualized list's rows and the row last focused
/// (#959). The handles are made again only when the rows change — not on
/// every frame (#937) — and a row keeps its handle while its key stays.
#[derive(Default)]
pub struct RowFocus {
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
    pub fn rows(
        &mut self,
        keys: RowKeys,
        scroll: &RowScroll,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) -> RowList {
        let (moved, rebuild, lost) = self.sync_keys(keys, window, cx);
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
            page_size: scroll.page_size(&self.keys, first),
        }
    }

    fn sync_keys(&mut self, keys: RowKeys, window: &Window, cx: &App) -> (bool, bool, bool) {
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
        (moved, rebuild, lost)
    }

    /// The list is not drawn (still loading, or failed): no row is left, so
    /// a focused row hands the focus to `fallback`, and the rows are
    /// forgotten.
    pub fn release(&mut self, fallback: Option<&FocusHandle>, window: &mut Window, cx: &mut App) {
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
    pub fn yield_focus(
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
    pub fn holds_focus(&self, window: &Window, cx: &App) -> bool {
        self.handles
            .iter()
            .any(|handle| handle.contains_focused(window, cx))
    }

    /// The key of the row holding the focus, if any.
    pub fn focused(&self, window: &Window) -> Option<&str> {
        self.keys
            .iter()
            .zip(self.handles.iter())
            .find(|(_, handle)| handle.is_focused(window))
            .map(|((key, _), _)| key.as_str())
    }

    /// Focus the first row, scrolling it into view (a pane opened from the
    /// keyboard, #981). False when there is no row.
    pub fn focus_first(&self, scroll: &RowScroll, window: &mut Window, cx: &mut App) -> bool {
        let Some((handle, (_, ix))) = self.handles.first().zip(self.keys.first()) else {
            return false;
        };
        scroll.reveal(*ix);
        handle.focus(window, cx);
        true
    }

    /// Reveal and restore the row last focused, or the first surviving row.
    pub fn focus_remembered(&self, scroll: &RowScroll, window: &mut Window, cx: &mut App) {
        let at = self.current.unwrap_or(0);
        if let Some((handle, (_, ix))) = self.handles.get(at).zip(self.keys.get(at)) {
            scroll.reveal(*ix);
            handle.focus(window, cx);
        }
    }

    /// Restore focus to the row `key`, including after an owned popup closes.
    pub fn focus(&self, key: &str, window: &mut Window, cx: &mut App) {
        if let Some(at) = self.keys.iter().position(|(k, _)| k == key) {
            self.handles[at].focus(window, cx);
        }
    }
}

/// One virtualized list's rows being drawn.
pub struct RowList {
    rows: RowKeys,
    handles: Rc<[FocusHandle]>,
    stop: Option<usize>,
    scroll: RowScroll,
    page_size: usize,
}

impl RowList {
    pub fn focus_handle(&self, at: usize) -> &FocusHandle {
        &self.handles[at]
    }

    pub fn is_tab_stop(&self, at: usize) -> bool {
        self.stop == Some(at)
    }

    /// The remembered row containing focus, including its inline controls.
    pub fn focused_stop(&self, window: &Window, cx: &App) -> Option<usize> {
        self.stop
            .filter(|&at| self.handles[at].contains_focused(window, cx))
    }

    /// The list's container.
    pub fn list(&self, el: Stateful<Div>) -> Stateful<Div> {
        el.key_context(ROW_CONTEXT)
    }

    /// How many rows the list has.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether a drawn row is the list's Tab stop this frame (none while no
    /// row is known to be on screen).
    pub fn has_stop(&self) -> bool {
        self.stop.is_some()
    }

    /// Whether row `at`'s focus handle is `handle`.
    pub fn is_handle(&self, at: usize, handle: &FocusHandle) -> bool {
        self.handles.get(at) == Some(handle)
    }

    /// Focus the list's Tab stop, if it has one.
    pub fn focus_stop(&self, window: &mut Window, cx: &mut App) {
        if let Some(at) = self.stop {
            self.handles[at].focus(window, cx);
        }
    }

    /// Select a clicked row through the same focus owner as keyboard movement.
    pub fn focus_row(&self, at: usize, window: &mut Window, cx: &mut App) {
        if let Some(handle) = self.handles.get(at) {
            handle.focus(window, cx);
        }
    }

    /// Reveal a control inside a virtualized row without changing its focus.
    pub fn reveal_entry(&self, entry: usize) {
        self.scroll.reveal(entry);
    }

    /// This list as a `uniform_list` draws it this frame, `visible` being
    /// the items its processor was asked for: those are on screen now, so the
    /// stop moves to the first of them when the one picked from last frame's
    /// range has scrolled out (#987 review — a wheel, a divider drag or a
    /// resize changes the range after the stop was picked).
    pub fn as_drawn(&self, visible: &std::ops::Range<usize>) -> RowList {
        let on = |at: usize| visible.contains(&self.rows[at].1);
        let first = self.rows.partition_point(|&(_, ix)| ix < visible.start);
        let stop = self
            .stop
            .filter(|&at| on(at))
            .or_else(|| (first < self.rows.len() && on(first)).then_some(first));
        RowList {
            rows: self.rows.clone(),
            handles: self.handles.clone(),
            stop,
            scroll: self.scroll.clone(),
            page_size: uniform_page(&self.rows, first, visible),
        }
    }

    /// The row at `at` (its place among the rows): Tab reaches it when it is
    /// the list's stop, ↑/↓ move from it, Enter/Space press it (`on_click`).
    pub fn row(&self, at: usize, el: Stateful<Div>) -> Stateful<Div> {
        let handle = self.handles[at]
            .clone()
            .tab_index(0)
            .tab_stop(self.is_tab_stop(at));
        self.navigation(at, with_ring(el.track_focus(&handle)))
    }

    /// A list whose arrows both focus and select the destination entry.
    pub fn selecting_row(
        &self,
        at: usize,
        el: Stateful<Div>,
        on_move: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
    ) -> Stateful<Div> {
        let handle = self.handles[at]
            .clone()
            .tab_index(0)
            .tab_stop(self.is_tab_stop(at));
        self.navigation_with(at, with_ring(el.track_focus(&handle)), on_move)
    }

    /// Arrow navigation shared by a row's independently focused controls.
    pub fn navigation(&self, at: usize, el: Stateful<Div>) -> Stateful<Div> {
        self.navigation_with(at, el, |_, _, _| {})
    }

    fn navigation_with(
        &self,
        at: usize,
        el: Stateful<Div>,
        on_move: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
    ) -> Stateful<Div> {
        let move_to = |to: usize| {
            let (rows, handles, scroll) =
                (self.rows.clone(), self.handles.clone(), self.scroll.clone());
            let on_move = on_move.clone();
            move |window: &mut Window, cx: &mut App| {
                let to = to.min(rows.len() - 1);
                scroll.reveal(rows[to].1);
                handles[to].focus(window, cx);
                on_move(rows[to].1, window, cx);
            }
        };
        let (page, last) = (self.page_size, self.rows.len() - 1);
        let prev = move_to(at.saturating_sub(1));
        let next = move_to(at.saturating_add(1));
        let first = move_to(0);
        let end = move_to(last);
        let page_up = move_to(at.saturating_sub(page));
        let page_down = move_to(at.saturating_add(page));
        el.on_key_down(stop_activation_keys)
            .on_action(move |_: &RowListPrev, window, cx| prev(window, cx))
            .on_action(move |_: &RowListNext, window, cx| next(window, cx))
            .on_action(move |_: &RowListFirst, window, cx| first(window, cx))
            .on_action(move |_: &RowListLast, window, cx| end(window, cx))
            .on_action(move |_: &RowListPageUp, window, cx| page_up(window, cx))
            .on_action(move |_: &RowListPageDown, window, cx| page_down(window, cx))
    }
}

#[cfg(test)]
mod uniform_tail_tests {
    use super::*;

    #[test]
    fn final_focusable_row_reveals_note_without_making_it_focusable() {
        let handle = UniformListScrollHandle::new();
        let scroll = RowScroll::UniformWithTail {
            handle: handle.clone(),
            drawn: 90..101,
            last_row: 99,
            tail: 100,
        };
        assert!(scroll.drawn(99));
        assert_eq!(scroll.top(), 90);
        scroll.reveal(98);
        assert_eq!(
            handle
                .0
                .borrow()
                .deferred_scroll_to_item
                .as_ref()
                .unwrap()
                .item_index,
            98
        );
        scroll.reveal(99);
        assert_eq!(
            handle
                .0
                .borrow()
                .deferred_scroll_to_item
                .as_ref()
                .unwrap()
                .item_index,
            100
        );
        let keys: Vec<_> = (0..100).map(|ix| (ix.to_string(), ix)).collect();
        assert_eq!(scroll.page_size(&keys, 90), 9);
    }
}
