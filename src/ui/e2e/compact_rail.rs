//! Native compact navigator probes; handles are recorded only by their renderer.
use gpui::{ScrollHandle, WindowId};
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static PR_SCROLL: RefCell<HashMap<WindowId, ScrollHandle>> = RefCell::new(HashMap::new());
}

pub fn record_pr_scroll(window: WindowId, handle: &ScrollHandle) {
    PR_SCROLL.with(|owners| owners.borrow_mut().insert(window, handle.clone()));
}

pub fn pr_scroll(window: WindowId) -> Option<ScrollHandle> {
    PR_SCROLL.with(|owners| owners.borrow().get(&window).cloned())
}

pub fn clear(window: WindowId) {
    PR_SCROLL.with(|owners| owners.borrow_mut().remove(&window));
}
