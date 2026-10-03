//! The keyboard inside Kagi's context menus, and the key that opens one
//! (#985).
//!
//! Every context menu (commit, branch, remote branch, tag, stash, worktree)
//! is drawn by `menu_overlay::render_menu_overlay`; this module gives that
//! one renderer its keyboard path. When a menu opens it takes the focus, on
//! its first enabled item: ↑/↓ move between the enabled items (wrapping,
//! disabled ones passed by), Home/End go to the ends, Enter/Space press the
//! focused item (gpui's keyboard click on its `on_click`), and Escape closes
//! the menu (the window's cancel chain). When the menu closes the focus goes
//! back to where it was when it opened — the row the key was pressed on, or
//! the window for a right-click — except that an item which opened a modal
//! leaves the focus on the window, where the modal's Enter / Escape run
//! (#817). The items' focus shows the hover highlight only for keyboard
//! focus (`focus_visible`), so a pointer user sees the menu as before.
//!
//! Shift+F10 (and the Menu key where the keyboard has one) opens the menu of
//! what has the focus: from the window, the selected commit's.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    actions, canvas, prelude::*, App, Bounds, Context, FocusHandle, KeyBinding, Pixels, Point,
    Window,
};

use super::keyboard_nav::FocusSlots;
use super::KagiApp;

actions!(
    kagi_menu,
    [MenuPrev, MenuNext, MenuFirst, MenuLast, ContextMenuKey]
);

/// The menus' key context, below which ↑/↓/Home/End are the menu's.
pub(crate) const MENU_CONTEXT: &str = "KagiMenu";

/// Bind the menu's keys and the context-menu key. Registered after the
/// app-wide `!Terminal && !Input` arrows so the scoped ones outrank them.
pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", MenuPrev, Some(MENU_CONTEXT)),
        KeyBinding::new("down", MenuNext, Some(MENU_CONTEXT)),
        KeyBinding::new("home", MenuFirst, Some(MENU_CONTEXT)),
        KeyBinding::new("end", MenuLast, Some(MENU_CONTEXT)),
        // macOS keyboards have no Menu key; gpui names the Windows Apps key
        // `menu`.
        KeyBinding::new("shift-f10", ContextMenuKey, Some("!Terminal && !Input")),
        KeyBinding::new("menu", ContextMenuKey, Some("!Terminal && !Input")),
    ]);
}

/// The open menu's keyboard state. Cheap to clone: the menu renderers are
/// free functions handed a clone.
#[derive(Clone, Default)]
pub struct MenuKeys(Rc<Inner>);

#[derive(Default)]
struct Inner {
    /// One focus handle per drawn item, in drawing order.
    items: FocusSlots,
    /// A menu was open last frame.
    open: Cell<bool>,
    /// The menu just opened: its first enabled item takes the focus.
    focus_first: Cell<bool>,
    /// Where the focus was when the menu opened.
    return_to: RefCell<Option<FocusHandle>>,
    /// The open menu's enabled items, as slots in drawing order.
    enabled: RefCell<Vec<usize>>,
}

impl MenuKeys {
    /// Item `slot`'s focus handle.
    pub(crate) fn item(&self, slot: usize, cx: &App) -> FocusHandle {
        self.0.items.get(slot, cx)
    }

    /// The menu was drawn with `enabled` as its enabled items' slots. True
    /// once when it has just opened: its first enabled item takes the focus.
    pub(crate) fn drawn(&self, enabled: impl Iterator<Item = usize>) -> bool {
        let mut slots = self.0.enabled.borrow_mut();
        slots.clear();
        slots.extend(enabled);
        self.0.focus_first.take()
    }

    /// The drawn item holding the focus, if any.
    fn focused(&self, window: &Window) -> Option<usize> {
        self.0.items.focused(window)
    }

    /// Move the focus among the enabled items: one step back or on
    /// (wrapping, the disabled ones passed by), or to an end.
    pub(crate) fn step(&self, step: Step, window: &mut Window, cx: &mut App) {
        let to = {
            let enabled = self.0.enabled.borrow();
            let Some((&first, &last)) = enabled.first().zip(enabled.last()) else {
                return;
            };
            let at = self
                .focused(window)
                .and_then(|slot| enabled.iter().position(|&s| s == slot));
            let n = enabled.len();
            match (step, at) {
                (Step::First, _) | (Step::Next, None) => first,
                (Step::Last, _) | (Step::Prev, None) => last,
                (Step::Next, Some(at)) => enabled[(at + 1) % n],
                (Step::Prev, Some(at)) => enabled[(at + n - 1) % n],
            }
        };
        self.0.items.get(to, cx).focus(window, cx);
    }

    /// The item holding the focus and the enabled items, as slots.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn state_for_e2e(&self, window: &Window) -> (Option<usize>, Vec<usize>) {
        (self.focused(window), self.0.enabled.borrow().clone())
    }
}

/// A move inside the menu.
#[derive(Clone, Copy)]
pub(crate) enum Step {
    Prev,
    Next,
    First,
    Last,
}

/// The selected commit row's bounds as last drawn, for the menu the key
/// opens. Written by the selected row only.
#[derive(Clone, Default)]
pub struct RowAnchor(Rc<Cell<Option<Bounds<Pixels>>>>);

impl RowAnchor {
    /// A zero-size probe recording its parent's bounds: give it to the
    /// selected row only.
    pub(crate) fn probe(&self) -> impl IntoElement {
        let anchor = self.0.clone();
        canvas(
            |_, _, _| {},
            move |bounds, _, _, _| anchor.set(Some(bounds)),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Forget what was recorded, so a frame that does not draw the selected
    /// row leaves no stale anchor.
    pub(crate) fn clear(&self) {
        self.0.set(None);
    }

    fn bottom_left(&self) -> Option<Point<Pixels>> {
        self.0.get().map(|bounds| bounds.bottom_left())
    }
}

impl KagiApp {
    fn any_context_menu_open(&self) -> bool {
        self.commit_menu.is_some()
            || self.branch_menu.is_some()
            || self.stash_menu.is_some()
            || self.tag_menu.is_some()
            || self.worktree_menu.is_some()
    }

    /// Each frame before the menus are drawn: a menu that just opened takes
    /// the focus (on its first item, in its renderer); one that just closed
    /// gives it back.
    pub(super) fn sync_menu_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.any_context_menu_open();
        let keys = &self.menu_keys.0;
        if open && !keys.open.get() {
            keys.open.set(true);
            keys.focus_first.set(true);
            *keys.return_to.borrow_mut() = window.focused(cx).or_else(|| self.root_focus.clone());
        } else if !open && keys.open.get() {
            keys.open.set(false);
            let back = keys.return_to.borrow_mut().take();
            // Only a focus the menu held (or lost with it) moves: an item may
            // have moved it on itself.
            let held = window.focused(cx).is_none() || keys.items.focused(window).is_some();
            if held {
                let target = if self.active_modal.is_some() {
                    self.root_focus.clone()
                } else {
                    back.or_else(|| self.root_focus.clone())
                };
                if let Some(target) = target {
                    target.focus(window, cx);
                }
            }
        }
    }

    /// The context-menu key on the window: the selected commit's menu, below
    /// its row. Nothing while a modal or a menu is open.
    pub(super) fn open_context_menu_from_key(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.active_modal.is_some() || self.any_context_menu_open() {
            return;
        }
        let Some(row) = self.ui().selected else {
            return;
        };
        let at = self.context_anchor.bottom_left().unwrap_or_else(|| {
            let size = window.viewport_size();
            Point::new(size.width / 2., size.height / 3.)
        });
        self.open_commit_menu(row, at);
        cx.notify();
    }
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// The open menu's focused item and enabled items, as slots.
    pub fn menu_keys_for_e2e(&self, window: &Window) -> (Option<usize>, Vec<usize>) {
        self.menu_keys.state_for_e2e(window)
    }

    /// Where the selected commit row was last drawn, if it was.
    pub fn context_anchor_for_e2e(&self) -> Option<Point<Pixels>> {
        self.context_anchor.bottom_left()
    }
}
