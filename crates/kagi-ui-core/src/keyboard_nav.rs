//! Shared roving tab lists for root and Git-free feature panes.
use crate::theme::theme;
use gpui::{
    actions, prelude::*, rgb, transparent_black, App, ClickEvent, Div, FocusHandle, KeyBinding,
    KeyDownEvent, Role, SharedString, Stateful, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
#[path = "keyboard_rows.rs"]
mod rows;
pub use rows::*;
actions!(
    kagi_tab_list,
    [TabListPrev, TabListNext, TabListFirst, TabListLast]
);
const CONTEXT: &str = "KagiTabList";
pub fn bind_keys(cx: &mut App) {
    rows::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("left", TabListPrev, Some(CONTEXT)),
        KeyBinding::new("right", TabListNext, Some(CONTEXT)),
        KeyBinding::new("home", TabListFirst, Some(CONTEXT)),
        KeyBinding::new("end", TabListLast, Some(CONTEXT)),
    ]);
    cx.intercept_keystrokes(|event, _, _| {
        if event.keystroke.key == "tab" {
            forget_roving();
        }
    })
    .detach();
}
pub fn forget_roving() {
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
/// The ring's width in px (`border_2`). Not scaled with the UI zoom: a fixed
/// 2px ring stays crisp and visible at 0.7×, where a scaled one would be a
/// blurred 1.4px.
pub const RING: f32 = 2.;

/// Padding of `n` (px at zoom 1.0) for an element that carries the ring: the
/// zoomed padding minus the ring's fixed width, so the element's outer size is
/// the one `scaled_px(n)` padding gave before the ring, at every zoom (#960
/// review). Subtracting the ring before scaling made a cell 2 − 2·zoom px off:
/// smaller at 1.5×, larger at 0.7×. The smallest inset in use is
/// `inset(4.)` at 0.7×, 0.8px.
pub fn inset(n: f32) -> gpui::Pixels {
    crate::theme::scaled_px(n) - gpui::px(RING)
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
pub fn forget_roving_without_focus(window: &Window) {
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
pub struct TabFocus {
    cells: Rc<RefCell<Vec<(u64, FocusHandle)>>>,
    roving: Rc<Cell<Option<u64>>>,
    /// Handles of cells gone since the last frame.
    closed: RefCell<Vec<FocusHandle>>,
    #[cfg(feature = "gui-e2e")]
    a11y: Rc<RefCell<Vec<(Role, String, bool, bool)>>>,
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
            #[cfg(feature = "gui-e2e")]
            a11y: Default::default(),
        }
    }
}

impl TabFocus {
    #[cfg(feature = "gui-e2e")]
    pub fn recorded_tabs(&self) -> Vec<(Role, String, bool, bool)> {
        self.a11y.borrow().clone()
    }

    /// The handles of the cells `keys` names, in that order: a known cell
    /// keeps its handle, a new one gets one, and a cell no longer named
    /// leaves (its handle kept for [`Self::release_closed`]).
    pub fn sync(&self, keys: &[u64], cx: &App) -> Vec<FocusHandle> {
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
    pub fn release_closed(
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
    pub fn focused_cell(&self, window: &Window) -> Option<FocusHandle> {
        self.cells
            .borrow()
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(_, handle)| handle.clone())
    }

    /// The list is not drawn for now (Home is not in front): a focus on any
    /// of its cells goes to `fallback`.
    pub fn yield_focus(&self, fallback: Option<&FocusHandle>, window: &mut Window, cx: &mut App) {
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
    pub fn focus(&self, slot: usize, window: &mut Window, cx: &mut App) {
        let handle = self.cells.borrow().get(slot).map(|(_, h)| h.clone());
        if let Some(handle) = handle {
            handle.focus(window, cx);
        }
    }

    /// The slot, as last drawn, whose cell holds the focus, if any.
    pub fn focused(&self, window: &Window) -> Option<usize> {
        self.cells
            .borrow()
            .iter()
            .position(|(_, handle)| handle.is_focused(window))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// Moving to a cell selects it: selecting is cheap and starts nothing.
    Automatic,
    /// Moving only focuses; Enter/Space select.
    Manual,
}

/// The identities of `n` cells that never move: their slot numbers.
pub fn slot_keys(n: usize) -> Vec<u64> {
    (0..n as u64).collect()
}

type Select = Rc<dyn Fn(usize, bool, &mut Window, &mut App)>;

/// One tab list being drawn: the slots on screen in order, the selected one
/// (none while something no cell names is on screen), and what selecting a
/// slot does.
pub struct TabList {
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
    #[cfg(feature = "gui-e2e")]
    a11y: Rc<RefCell<Vec<(Role, String, bool, bool)>>>,
}

impl TabList {
    /// One cell per entry of `keys`, its identity (a slot number when the
    /// cells never move: [`slot_keys`]); their handles are kept in `focus`.
    /// `shown` lists the slots drawn, in order.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        focus: &TabFocus,
        keys: Vec<u64>,
        shown: Vec<usize>,
        selected: Option<usize>,
        activation: Activation,
        pointer_focus: Option<FocusHandle>,
        select: impl Fn(usize, bool, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Self {
        let handles = focus.sync(&keys, cx);
        #[cfg(feature = "gui-e2e")]
        focus.a11y.borrow_mut().clear();
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
            #[cfg(feature = "gui-e2e")]
            a11y: focus.a11y.clone(),
        }
    }

    /// Stable cell handle for a composed control's accessibility/focus probes.
    pub fn focus_handle(&self, slot: usize) -> &FocusHandle {
        &self.handles[slot]
    }

    /// The list's container.
    pub fn list(&self, el: Stateful<Div>) -> Stateful<Div> {
        el.role(Role::TabList).key_context(CONTEXT)
    }

    /// The cell for `slot`, named `label`.
    pub fn cell(&self, slot: usize, label: &str, el: Stateful<Div>) -> Stateful<Div> {
        let selected = self.selected == Some(slot);
        #[cfg(feature = "gui-e2e")]
        self.a11y.borrow_mut().push((
            Role::Tab,
            label.to_string(),
            selected,
            self.stop == Some(slot),
        ));
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
                    select(next, true, window, cx);
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
            select(slot, event.is_keyboard(), window, cx);
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
pub fn with_ring(el: Stateful<Div>) -> Stateful<Div> {
    el.border_2()
        .border_color(transparent_black())
        .focus_visible(|s| s.border_color(rgb(theme().ui_accent())))
}

/// Enter / Space press the focused element (gpui's keyboard click, already
/// armed by the time this runs); nothing above it may act on them too.
pub fn stop_activation_keys(event: &KeyDownEvent, _: &mut Window, cx: &mut App) {
    let stroke = &event.keystroke;
    if (stroke.key == "enter" || stroke.key == "space") && !stroke.modifiers.modified() {
        cx.stop_propagation();
    }
}
