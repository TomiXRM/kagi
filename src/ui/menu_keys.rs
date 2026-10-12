//! The keyboard inside Kagi's context menus, and the key that opens one
//! (#985).
//!
//! Every context menu (commit, branch, remote branch, tag, stash, worktree)
//! is drawn by `menu_overlay::render_menu_overlay`; this module gives that
//! one renderer its keyboard path. When a menu opens it takes the focus, on
//! its first visible item: ↑/↓ move between all visible items (wrapping,
//! disabled ones focusable but inert), Home/End go to the ends, Enter/Space
//! press the focused item; → opens a submenu and ←/Escape closes one level.
//! When the menu closes the focus goes
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
    [
        MenuPrev,
        MenuNext,
        MenuFirst,
        MenuLast,
        MenuDismiss,
        MenuOpen,
        MenuBack,
        MenuEscape,
        ContextMenuKey
    ]
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
        KeyBinding::new("right", MenuOpen, Some(MENU_CONTEXT)),
        KeyBinding::new("left", MenuBack, Some(MENU_CONTEXT)),
        KeyBinding::new("escape", MenuEscape, Some(MENU_CONTEXT)),
        // Tab / Shift+Tab would walk out of the menu to the controls behind
        // it; they close it instead, and the focus goes back where the menu
        // was opened from (#991 review).
        KeyBinding::new("tab", MenuDismiss, Some(MENU_CONTEXT)),
        KeyBinding::new("shift-tab", MenuDismiss, Some(MENU_CONTEXT)),
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
    /// The menu just opened: its first visible item takes the focus.
    focus_first: Cell<bool>,
    /// Where the focus was when the menu opened.
    return_to: RefCell<Option<FocusHandle>>,
    /// The open menu's visible, focusable items, as slots in menu order.
    visible: RefCell<Vec<usize>>,
    /// Each drawn item's place among the item list's children (separators
    /// and group titles are children too), by slot; `None` for a hidden one.
    children: RefCell<Vec<Option<usize>>>,
    /// The item list's scroll: a menu taller than the window scrolls, and
    /// the item the keys move to is scrolled into view (#991 review).
    scroll: gpui::ScrollHandle,
    /// The currently open child, tied to its launching row's stable slot.
    submenu: RefCell<Option<(usize, MenuKeys)>>,
    bounds: RefCell<Vec<Option<Bounds<Pixels>>>>,
    /// Pointer changes wait briefly so diagonal travel can reach a child panel.
    hover_task: RefCell<Option<gpui::Task<()>>>,
}

impl MenuKeys {
    /// Reconcile a compact overlay's independent focus lifecycle.
    pub(crate) fn sync(
        &self,
        open: bool,
        covered: bool,
        fallback: Option<&FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if open && !self.0.open.replace(true) {
            self.0.focus_first.set(true);
            *self.0.return_to.borrow_mut() = window.focused(cx).or_else(|| fallback.cloned());
        } else if !open && self.0.open.replace(false) {
            let back = self.0.return_to.borrow_mut().take();
            if window.focused(cx).is_none() || self.holds_focus(window) {
                if let Some(target) = if covered {
                    fallback.cloned()
                } else {
                    back.or_else(|| fallback.cloned())
                } {
                    target.focus(window, cx);
                }
            }
        }
    }

    /// Item `slot`'s focus handle.
    pub(crate) fn item(&self, slot: usize, cx: &App) -> FocusHandle {
        self.0.items.get(slot, cx)
    }

    /// The item list's scroll handle.
    pub(crate) fn scroll(&self) -> &gpui::ScrollHandle {
        &self.0.scroll
    }

    /// Update visible focusable slots; hidden rows retain their stable slot.
    /// True once when opening requires focusing the first row.
    pub(crate) fn drawn(&self, visible: impl Iterator<Item = usize>) -> bool {
        let mut slots = self.0.visible.borrow_mut();
        slots.clear();
        slots.extend(visible);
        self.0.children.borrow_mut().clear();
        self.0.focus_first.take()
    }

    /// Item `slot` is drawn as child `child` of the item list.
    pub(crate) fn place(&self, slot: usize, child: usize) {
        let mut children = self.0.children.borrow_mut();
        if children.len() <= slot {
            children.resize(slot + 1, None);
        }
        children[slot] = Some(child);
    }

    /// The drawn item holding the focus, if any.
    fn focused(&self, window: &Window) -> Option<usize> {
        self.0.items.focused(window)
    }

    /// Move among every visible item, including unavailable commands.
    pub(crate) fn step(&self, step: Step, window: &mut Window, cx: &mut App) {
        self.cancel_hover();
        let to = {
            let visible = self.0.visible.borrow();
            let Some((&first, &last)) = visible.first().zip(visible.last()) else {
                return;
            };
            let at = self
                .focused(window)
                .and_then(|slot| visible.iter().position(|&s| s == slot));
            let n = visible.len();
            match (step, at) {
                (Step::First, _) | (Step::Next, None) => first,
                (Step::Last, _) | (Step::Prev, None) => last,
                (Step::Next, Some(at)) => visible[(at + 1) % n],
                (Step::Prev, Some(at)) => visible[(at + n - 1) % n],
            }
        };
        self.0.submenu.borrow_mut().take();
        self.0.items.get(to, cx).focus(window, cx);
        if let Some(&Some(child)) = self.0.children.borrow().get(to) {
            self.0.scroll.scroll_to_item(child);
        }
    }

    pub(crate) fn record_bounds(&self, slot: usize, bounds: Bounds<Pixels>) {
        let mut rows = self.0.bounds.borrow_mut();
        if rows.len() <= slot {
            rows.resize(slot + 1, None);
        }
        rows[slot] = Some(bounds);
    }

    pub(crate) fn bounds(&self, slot: usize) -> Option<Bounds<Pixels>> {
        self.0.bounds.borrow().get(slot).copied().flatten()
    }

    pub(crate) fn open_submenu(&self, slot: usize, keyboard: bool) {
        let mut submenu = self.0.submenu.borrow_mut();
        if submenu.as_ref().is_none_or(|(current, _)| *current != slot) {
            *submenu = Some((slot, Self::default()));
        }
        if keyboard {
            if let Some((_, keys)) = submenu.as_ref() {
                keys.0.focus_first.set(true);
            }
        }
    }

    pub(crate) fn submenu(&self) -> Option<(usize, Self)> {
        self.0.submenu.borrow().clone()
    }

    pub(crate) fn close_submenu(&self, window: &mut Window, cx: &mut App) {
        self.cancel_hover();
        self.restore_submenu_focus(window, cx);
    }

    fn restore_submenu_focus(&self, window: &mut Window, cx: &mut App) {
        if let Some((slot, _)) = self.0.submenu.borrow_mut().take() {
            self.item(slot, cx).focus(window, cx);
        }
    }

    pub(crate) fn cancel_hover(&self) {
        self.0.hover_task.borrow_mut().take();
    }

    pub(crate) fn hover_submenu(
        &self,
        slot: Option<usize>,
        window: &mut Window,
        cx: &mut Context<KagiApp>,
    ) {
        self.cancel_hover();
        if self.submenu().map(|(at, _)| at) == slot {
            return;
        }
        let keys = Rc::downgrade(&self.0);
        *self.0.hover_task.borrow_mut() = Some(cx.spawn_in(window, async move |app, acx| {
            acx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let _ = app.update_in(acx, |_, window, cx| {
                let Some(keys) = keys.upgrade().map(Self) else {
                    return;
                };
                if slot.is_some_and(|slot| !keys.0.visible.borrow().contains(&slot)) {
                    return;
                }
                if keys
                    .submenu()
                    .is_some_and(|(_, child)| child.holds_focus(window))
                {
                    keys.restore_submenu_focus(window, cx);
                } else {
                    keys.0.submenu.borrow_mut().take();
                }
                if let Some(slot) = slot {
                    keys.open_submenu(slot, false);
                }
                cx.notify();
            });
        }));
    }

    fn holds_focus(&self, window: &Window) -> bool {
        self.focused(window).is_some()
            || self
                .submenu()
                .is_some_and(|(_, child)| child.holds_focus(window))
    }

    #[cfg(feature = "gui-e2e")]
    fn submenu_depth(&self) -> usize {
        self.submenu()
            .map_or(0, |(_, child)| 1 + child.submenu_depth())
    }

    /// The active level's focused item and all visible focusable slots.
    #[cfg(feature = "gui-e2e")]
    pub(crate) fn state_for_e2e(&self, window: &Window) -> (Option<usize>, Vec<usize>) {
        if let Some((_, child)) = self.submenu() {
            return child.state_for_e2e(window);
        }
        (self.focused(window), self.0.visible.borrow().clone())
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
    pub(crate) fn point(&self, window: &Window) -> Point<Pixels> {
        menu_point(self, window)
    }

    /// A probe filling its parent (painting nothing), recording the parent's
    /// bounds: give it to the selected row only.
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

    /// The repository on screen is being left (Home in front, another tab):
    /// close every workspace context menu and forget the keyboard's hold on
    /// it. Whoever leaves places the focus; a menu reset here returns it
    /// nowhere (#991 review).
    pub(crate) fn reset_context_menus(&mut self) {
        self.close_context_menus();
        self.menu_keys.cancel_hover();
        let keys = &self.menu_keys.0;
        keys.open.set(false);
        keys.focus_first.set(false);
        keys.return_to.borrow_mut().take();
        keys.submenu.borrow_mut().take();
    }

    /// Close every workspace context menu on the tab on screen (a reload
    /// replaced the state its items were planned on). The next frame gives
    /// the focus back as any close does: where the menu was opened from, or
    /// to the window when a modal is up (#991 review).
    pub(crate) fn close_context_menus(&mut self) {
        self.menu_keys.cancel_hover();
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = None;
        self.tag_menu = None;
        self.worktree_menu = None;
    }

    /// Each frame before the menus are drawn: a menu that just opened takes
    /// the focus (on its first item, in its renderer); one that just closed
    /// gives it back. A layer drawn above the menus that leaves the focus
    /// where it was (a modal, a notice, a plan, an Info panel) closes them
    /// first: the focused item would otherwise take Enter unseen; the focus
    /// then goes to the window, never back to the row behind that layer
    /// (#991 review).
    pub(super) fn sync_menu_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let covered = self.workspace_menus_covered(cx);
        if self.any_context_menu_open() && covered {
            self.close_context_menus();
        }
        let open = self.any_context_menu_open();
        let keys = &self.menu_keys.0;
        if open && !keys.open.get() {
            keys.hover_task.borrow_mut().take();
            keys.open.set(true);
            keys.focus_first.set(true);
            keys.submenu.borrow_mut().take();
            *keys.return_to.borrow_mut() = window.focused(cx).or_else(|| self.root_focus.clone());
        } else if !open && keys.open.get() {
            keys.hover_task.borrow_mut().take();
            keys.open.set(false);
            let back = keys.return_to.borrow_mut().take();
            // Only a focus the menu held (or lost with it) moves: an item may
            // have moved it on itself.
            let held = window.focused(cx).is_none() || self.menu_keys.holds_focus(window);
            if held {
                let target = if self.active_modal.is_some() || covered {
                    self.root_focus.clone()
                } else {
                    back.or_else(|| self.root_focus.clone())
                };
                if let Some(target) = target {
                    target.focus(window, cx);
                }
            }
            keys.submenu.borrow_mut().take();
        }
    }

    /// The context-menu key on the window: the selected commit's menu, below
    /// its row. Only where the arrows move the commit selection (#986's
    /// guard): the Graph's commit list in front, no menu or modal over it.
    pub(super) fn open_context_menu_from_key(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.commit_list_has_focus(window, cx) {
            return;
        }
        let Some(row) = self.ui().selected else {
            return;
        };
        let at = menu_point(&self.context_anchor, window);
        self.open_commit_menu(row, at);
        cx.notify();
    }

    /// The context key opens the focused row's menu below it. Dynamic group
    /// headers reveal their complete identity in the existing keyboard-card
    /// layer instead. Neither path acts through a modal or another front layer.
    pub(super) fn open_sidebar_row_menu(
        &mut self,
        row: &super::sidebar::SidebarRow,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        use super::sidebar::SidebarRow;
        if self.active_modal.is_some()
            || self.any_context_menu_open()
            || self.front_layer(cx) != super::front_layer::FrontLayer::None
        {
            return;
        }
        let at = menu_point(&self.sidebar.focus.row_anchor, window);
        match row {
            SidebarRow::LocalBranchLeaf { name, .. } => {
                self.open_local_branch_menu(name.clone(), at)
            }
            SidebarRow::RemoteLeaf {
                display, target, ..
            } => self.open_remote_branch_menu(display.clone(), target.clone(), at),
            SidebarRow::Tag { name, .. } => self.open_tag_menu(name.clone(), at),
            SidebarRow::Stash { index, message, .. } => {
                self.open_stash_menu(*index, message.clone(), at)
            }
            // Sidebar worktree rows are linked worktrees (#733), as their
            // right-click says.
            SidebarRow::Worktree {
                name, locked, path, ..
            } => self.open_worktree_menu(name.clone(), *locked, false, Some(path.clone()), at),
            SidebarRow::LocalGroupHeader { key, .. }
            | SidebarRow::RemoteHeader { key, .. }
            | SidebarRow::RemoteSubGroup { key, .. } => {
                let reveal = &mut self.sidebar.focus.group_reveal;
                *reveal = if reveal.as_ref() == Some(key) {
                    None
                } else {
                    Some(key.clone())
                };
            }
            SidebarRow::SectionHeader { .. } => return,
        }
        cx.notify();
    }
}

/// Where the key opens a menu: below the row `anchor` recorded, or — the row
/// not drawn last frame — at a fixed point of the window.
fn menu_point(anchor: &RowAnchor, window: &Window) -> Point<Pixels> {
    anchor.bottom_left().unwrap_or_else(|| {
        let size = window.viewport_size();
        Point::new(size.width / 2., size.height / 3.)
    })
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// The active level's focused item and visible focusable slots.
    pub fn menu_keys_for_e2e(&self, window: &Window) -> (Option<usize>, Vec<usize>) {
        self.menu_keys.state_for_e2e(window)
    }

    pub fn submenu_depth_for_e2e(&self) -> usize {
        self.menu_keys.submenu_depth()
    }

    /// Where the selected commit row was last drawn, if it was.
    pub fn context_anchor_for_e2e(&self) -> Option<Point<Pixels>> {
        self.context_anchor.bottom_left()
    }

    /// Where the focused sidebar row was last drawn, if it was.
    pub fn sidebar_row_anchor_for_e2e(&self) -> Option<Point<Pixels>> {
        self.sidebar.focus.row_anchor.bottom_left()
    }
}
