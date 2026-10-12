//! Renderer-fed resolver metadata; not native VoiceOver evidence.
#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONTROLS: std::cell::RefCell<std::collections::HashMap<String, (gpui::Role, String, String)>> = Default::default();
    static FOCUSED: std::cell::RefCell<Option<String>> = Default::default();
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record(id: &str, name: &str, state: &str) {
    CONTROLS.with(|controls| {
        controls
            .borrow_mut()
            .insert(id.into(), (gpui::Role::Button, name.into(), state.into()));
    });
}
#[cfg(feature = "gui-e2e")]
pub fn control(id: &str) -> Option<(gpui::Role, String, String)> {
    CONTROLS.with(|controls| controls.borrow().get(id).cloned())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn focus(id: &str) {
    FOCUSED.with(|focused| *focused.borrow_mut() = Some(id.into()));
}
#[cfg(feature = "gui-e2e")]
pub fn take_focus_probe() -> Option<String> {
    FOCUSED.with(|focused| focused.borrow_mut().take())
}
