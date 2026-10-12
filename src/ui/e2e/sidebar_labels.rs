//! Renderer-fed sidebar metadata; no retained data in ordinary builds.
use gpui::SharedString;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DESCRIPTIONS: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
    static TOOLTIPS: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
}

pub(crate) fn description(tag_name: &str, text: SharedString) -> SharedString {
    #[cfg(feature = "gui-e2e")]
    DESCRIPTIONS.with(|map| {
        map.borrow_mut()
            .insert(format!("sidebar-tag-{tag_name}"), text.to_string())
    });
    #[cfg(not(feature = "gui-e2e"))]
    let _ = tag_name;
    text
}

pub(crate) fn record_tooltip(id: &str, text: &SharedString) {
    #[cfg(feature = "gui-e2e")]
    TOOLTIPS.with(|map| map.borrow_mut().insert(id.to_string(), text.to_string()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = (id, text);
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_description(id: &str) -> Option<String> {
    DESCRIPTIONS.with(|map| map.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_tooltip(id: &str) -> Option<String> {
    TOOLTIPS.with(|map| map.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn clear() {
    DESCRIPTIONS.with(|map| map.borrow_mut().clear());
    TOOLTIPS.with(|map| map.borrow_mut().clear());
}
