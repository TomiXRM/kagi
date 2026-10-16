//! Renderer probes for the keyboard diff regressions (not native AX delivery).
use gpui::{prelude::*, Div, Fill, Role, Stateful};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static ROWS: RefCell<HashMap<(u64, usize), Option<Fill>>> = RefCell::new(HashMap::new());
    static BUTTONS: RefCell<HashMap<usize, (Role, String)>> = RefCell::new(HashMap::new());
    static FOCUS: RefCell<Option<usize>> = const { RefCell::new(None) };
    static BODY_FOCUS: RefCell<HashMap<u64, (bool, bool)>> = RefCell::new(HashMap::new());
}

pub(crate) fn row(key: u64, index: usize, mut element: Stateful<Div>) -> Stateful<Div> {
    ROWS.with(|rows| {
        rows.borrow_mut()
            .insert((key, index), element.style().background.clone())
    });
    element
}

pub fn painted_rows(key: u64) -> Vec<(usize, Option<Fill>)> {
    ROWS.with(|rows| {
        rows.borrow()
            .iter()
            .filter(|((k, _), _)| *k == key)
            .map(|((_, i), fill)| (*i, fill.clone()))
            .collect()
    })
}

pub fn clear_frame() {
    ROWS.with(|rows| rows.borrow_mut().clear());
    BUTTONS.with(|buttons| buttons.borrow_mut().clear());
    BODY_FOCUS.with(|focus| focus.borrow_mut().clear());
}

pub(crate) fn button(row: usize, role: Role, label: &str) {
    BUTTONS.with(|buttons| buttons.borrow_mut().insert(row, (role, label.to_owned())));
}

pub fn button_attributes(row: usize) -> Option<(Role, String)> {
    BUTTONS.with(|buttons| buttons.borrow().get(&row).cloned())
}

pub(crate) fn note_focus(row: usize) {
    FOCUS.with(|focus| *focus.borrow_mut() = Some(row));
}
pub fn take_focus() -> Option<usize> {
    FOCUS.with(|focus| focus.borrow_mut().take())
}

pub(crate) fn note_body_focus(key: u64, focused: bool, ring_visible: bool) {
    BODY_FOCUS.with(|focus| focus.borrow_mut().insert(key, (focused, ring_visible)));
}

pub fn body_focused(key: u64) -> bool {
    BODY_FOCUS.with(|focus| {
        focus
            .borrow()
            .get(&key)
            .is_some_and(|(focused, _)| *focused)
    })
}

pub fn body_ring_visible(key: u64) -> bool {
    BODY_FOCUS.with(|focus| {
        focus
            .borrow()
            .get(&key)
            .is_some_and(|(_, visible)| *visible)
    })
}
