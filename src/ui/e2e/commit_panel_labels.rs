//! Renderer-fed Commit Panel captions; never predicts a caption from a Msg.
use gpui::SharedString;
#[cfg(feature = "gui-e2e")]
use std::{cell::RefCell, collections::HashMap};

#[cfg(feature = "gui-e2e")]
thread_local! {
    static LABELS: RefCell<HashMap<&'static str, SharedString>> = RefCell::new(HashMap::new());
}
#[cfg(feature = "gui-e2e")]
thread_local! {
    static NAMES: RefCell<HashMap<&'static str, SharedString>> = RefCell::new(HashMap::new());
    static ROLES: RefCell<HashMap<&'static str, gpui::Role>> = RefCell::new(HashMap::new());
    static SELECTED: RefCell<HashMap<&'static str, bool>> = RefCell::new(HashMap::new());
}

pub(crate) fn name(id: &'static str, text: &'static str) -> SharedString {
    #[cfg(feature = "gui-e2e")]
    NAMES.with(|names| names.borrow_mut().insert(id, text.into()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    text.into()
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_name(id: &str) -> Option<SharedString> {
    NAMES.with(|names| names.borrow().get(id).cloned())
}

pub(crate) fn role(id: &'static str, role: gpui::Role) -> gpui::Role {
    #[cfg(feature = "gui-e2e")]
    ROLES.with(|roles| roles.borrow_mut().insert(id, role));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    role
}

pub(crate) fn selected(id: &'static str, selected: bool) -> bool {
    #[cfg(feature = "gui-e2e")]
    SELECTED.with(|states| states.borrow_mut().insert(id, selected));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    selected
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_role(id: &str) -> Option<gpui::Role> {
    ROLES.with(|roles| roles.borrow().get(id).copied())
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_selected(id: &str) -> Option<bool> {
    SELECTED.with(|states| states.borrow().get(id).copied())
}

pub(crate) fn caption(id: &'static str, text: impl Into<SharedString>) -> SharedString {
    let text = text.into();
    #[cfg(feature = "gui-e2e")]
    LABELS.with(|labels| labels.borrow_mut().insert(id, text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn rendered(id: &str) -> Option<SharedString> {
    LABELS.with(|labels| labels.borrow().get(id).cloned())
}
