//! Render-boundary evidence for inspector metadata (#1237).
use gpui::{Role, SharedString};
use std::cell::RefCell;

#[derive(Clone, Debug)]
pub struct Metadata {
    pub author: SharedString,
    pub tooltip: Option<SharedString>,
    pub identity: Option<SharedString>,
    pub hash_role: Option<Role>,
    pub hash_name: Option<SharedString>,
}
thread_local! {
    static LAST: RefCell<Option<Metadata>> = const { RefCell::new(None) };
}
thread_local! {
    static FOCUS: RefCell<Option<gpui::FocusHandle>> = const { RefCell::new(None) };
}
pub(crate) fn record_focus(focus: gpui::FocusHandle) {
    FOCUS.with(|slot| *slot.borrow_mut() = Some(focus));
}
pub fn focus_handle() -> gpui::FocusHandle {
    FOCUS.with(|slot| slot.borrow().clone().expect("rendered hash focus"))
}
pub fn clear() {
    LAST.with(|slot| *slot.borrow_mut() = None);
    FOCUS.with(|slot| *slot.borrow_mut() = None);
}
pub(crate) fn record_tooltip(text: SharedString) {
    LAST.with(|slot| {
        if let Some(metadata) = slot.borrow_mut().as_mut() {
            metadata.tooltip = Some(text);
        }
    });
}
pub(crate) fn record_hash(name: SharedString) {
    LAST.with(|slot| {
        if let Some(metadata) = slot.borrow_mut().as_mut() {
            metadata.hash_role = Some(Role::Button);
            metadata.hash_name = Some(name);
        }
    });
}
pub(crate) fn record(mut metadata: Metadata) {
    LAST.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(previous) = slot.as_ref().filter(|old| old.author == metadata.author) {
            metadata.tooltip = previous.tooltip.clone();
        }
        *slot = Some(metadata);
    });
}
pub fn last() -> Option<Metadata> {
    LAST.with(|slot| slot.borrow().clone())
}
