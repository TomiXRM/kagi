//! Renderer-fed tab action names (not native VoiceOver evidence).
use gpui::Role;
use std::{cell::RefCell, collections::HashMap};
thread_local! {
    static CONTROLS: RefCell<HashMap<String, (Role, String)>> = RefCell::default();
    static DESCRIPTIONS: RefCell<HashMap<String, String>> = RefCell::default();
    static SCROLL: RefCell<Option<gpui::ScrollHandle>> = const { RefCell::new(None) };
}
pub(crate) fn record(id: impl Into<String>, role: Role, label: &str) {
    CONTROLS.with(|values| {
        values
            .borrow_mut()
            .insert(id.into(), (role, label.to_owned()))
    });
}
pub fn control(id: &str) -> Option<(Role, String)> {
    CONTROLS.with(|values| values.borrow().get(id).cloned())
}
pub(crate) fn record_description(id: impl Into<String>, description: &str) {
    DESCRIPTIONS.with(|values| {
        values
            .borrow_mut()
            .insert(id.into(), description.to_owned())
    });
}
pub fn description(id: &str) -> Option<String> {
    DESCRIPTIONS.with(|values| values.borrow().get(id).cloned())
}
pub(crate) fn record_scroll(handle: gpui::ScrollHandle) {
    SCROLL.with(|value| *value.borrow_mut() = Some(handle));
}
pub fn scroll() -> Option<gpui::ScrollHandle> {
    SCROLL.with(|value| value.borrow().clone())
}
