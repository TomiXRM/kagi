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
//! The focus ring is the one `Input` draws (`sync_gpui_component_theme` maps
//! gpui-component's `ring` to `color_branch`), shown only for keyboard focus
//! (`focus_visible`). It is a border kept transparent at rest, so taking focus
//! does not move anything; callers pad with [`inset`], which takes its width
//! out of their zoomed padding.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

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

/// The ring's width in px (`border_2`). Not scaled with the UI zoom: a fixed
/// 2px ring stays crisp and visible at 0.7×, where a scaled one would be a
/// blurred 1.4px.
const RING: f32 = 2.;

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
    roving: Weak<Cell<Option<usize>>>,
    handles: Weak<RefCell<Vec<FocusHandle>>>,
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
            let Some(at) = roving.get() else {
                continue;
            };
            let focused = list.handles.upgrade().is_some_and(|handles| {
                handles
                    .borrow()
                    .get(at)
                    .is_some_and(|handle| handle.is_focused(window))
            });
            if !focused {
                roving.set(None);
            }
        }
    });
}

/// The focus handles of one tab list's cells, one per slot, made as slots
/// appear and kept across frames; and the cell the arrows moved to, while
/// it holds the focus.
pub(crate) struct TabFocus {
    handles: Rc<RefCell<Vec<FocusHandle>>>,
    roving: Rc<Cell<Option<usize>>>,
}

impl Default for TabFocus {
    fn default() -> Self {
        let roving = Rc::new(Cell::new(None));
        let handles = Rc::new(RefCell::new(Vec::new()));
        ROVING.with(|lists| {
            lists.borrow_mut().push(Watched {
                roving: Rc::downgrade(&roving),
                handles: Rc::downgrade(&handles),
            })
        });
        Self { handles, roving }
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
