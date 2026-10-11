//! Renderer observations for #1235/#1236; never compiled in ordinary builds.
use std::cell::RefCell;
use std::collections::HashMap;
thread_local! {
    static MODES: RefCell<HashMap<&'static str, (gpui::Role, &'static str, bool)>> = RefCell::default();
    static DESCRIPTION: RefCell<Option<String>> = const { RefCell::new(None) };
}
pub(crate) fn record_mode(id: &'static str, role: gpui::Role, label: &'static str, selected: bool) {
    MODES.with(|values| values.borrow_mut().insert(id, (role, label, selected)));
}
pub fn mode(id: &str) -> Option<(gpui::Role, &'static str, bool)> {
    MODES.with(|values| values.borrow().get(id).copied())
}
pub(crate) fn record_description(text: &str) {
    DESCRIPTION.with(|value| *value.borrow_mut() = Some(text.to_owned()));
}
pub fn description() -> Option<String> {
    DESCRIPTION.with(|value| value.borrow().clone())
}
pub fn clear() {
    MODES.with(|values| values.borrow_mut().clear());
    DESCRIPTION.with(|value| *value.borrow_mut() = None);
}
