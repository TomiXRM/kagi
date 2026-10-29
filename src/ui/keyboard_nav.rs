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
//! gpui-component's `ring` to `ui_accent()`), shown only for keyboard focus
//! (`focus_visible`). It is a border kept transparent at rest, so taking focus
//! does not move anything; callers pad with [`inset`], which takes its width
//! out of their zoomed padding.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{prelude::*, App, Div, FocusHandle, KeyBinding, Stateful, Window};

// Root callers retain their existing API while Git-free panes share the same implementation.
#[allow(unused_imports)]
pub(crate) use kagi_ui_core::keyboard_nav::{
    forget_roving_without_focus, inset, reveal_list_row, slot_keys, stop_activation_keys,
    with_ring, Activation, RowFocus, RowKeys, RowList, RowScroll, TabFocus, TabList, TabListFirst,
    TabListLast, TabListNext, TabListPrev, RING,
};

/// ←/→/Home/End inside a tab list. The list's context is deeper than the
/// app-wide `!Terminal && !Input` arrows (PR mode's pane cycling), so these
/// win only while a cell holds the focus.
///
/// Tab and Shift+Tab leave a tab list from wherever the arrows put the focus
/// (that cell is the Tab stop of the frame on screen), so the next frame's
/// stop goes back to the selected cell: they are watched before any binding
/// runs, and every list forgets its arrowed-to cell.
pub(crate) fn bind_keys(cx: &mut App) {
    super::diff_selection::keyboard::bind_keys(cx);
    super::sidebar_focus::bind_keys(cx);
    super::inspector_files::bind_keys(cx);
    super::operations::oplog_restore_inspection::bind_keys(cx);
    kagi_ui_core::keyboard_nav::bind_keys(cx);
    kagi_ui_file_history::bind_keys(cx);
    // The resolver's inline Up returns to its own hunk, overriding RowList Up.
    super::conflict_editor_keys::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("up", TabListPrev, Some("KagiCommitPanelView")),
        KeyBinding::new("down", TabListNext, Some("KagiCommitPanelView")),
        KeyBinding::new("up", TabListPrev, Some("KagiActivityBuckets")),
        KeyBinding::new("down", TabListNext, Some("KagiActivityBuckets")),
    ]);
    cx.intercept_keystrokes(|event, window, cx| {
        if event.keystroke.key == "tab" {
            // Root's Tab binding runs before a div's capture_key_down.
            // Stop it at the window boundary while the dropdown owns keys;
            // otherwise it moves into the Settings trap before render repairs
            // the focus. Read the current window owner, not a retained tab.
            let app = window
                .root::<gpui_component::Root>()
                .flatten()
                .and_then(|root| {
                    root.read(cx)
                        .view()
                        .clone()
                        .downcast::<super::KagiApp>()
                        .ok()
                });
            if let Some(app) = app {
                app.update(cx, |app, cx| {
                    if app.visible_platform_menu_section().is_some() {
                        if let Some(root) = &app.root_focus {
                            window.focus(root, cx);
                        }
                        cx.stop_propagation();
                    }
                });
            }
        }
    })
    .detach();
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
    pub(crate) fn focused(&self, window: &Window) -> Option<usize> {
        self.0
            .borrow()
            .iter()
            .position(|handle| handle.is_focused(window))
    }
}
