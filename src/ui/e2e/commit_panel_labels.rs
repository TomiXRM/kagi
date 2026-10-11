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
