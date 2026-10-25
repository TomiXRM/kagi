//! Renderer-fed resolver metadata; not native VoiceOver evidence.
#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONTROLS: std::cell::RefCell<std::collections::HashMap<String, (gpui::Role, String, String)>> = Default::default();
    static FOCUSED: std::cell::RefCell<Option<String>> = Default::default();
    static DESCRIPTIONS: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
    static ROW_COUNTS: std::cell::Cell<[usize; 3]> = const { std::cell::Cell::new([0; 3]) };
    static BUTTON_ROLES: std::cell::RefCell<std::collections::HashMap<String, super::button_style::ButtonRole>> = Default::default();
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record(id: &str, name: &str, state: &str, description: &str) {
    CONTROLS.with(|controls| {
        controls
            .borrow_mut()
            .insert(id.into(), (gpui::Role::Button, name.into(), state.into()));
    });
    DESCRIPTIONS.with(|values| {
        values.borrow_mut().insert(id.into(), description.into());
    });
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_checkbox(id: &str, name: &str, taken: bool) {
    use super::i18n::Msg;
    let (state, description) = if taken {
        ("all", Msg::EditorSelectionAll.t())
    } else {
        ("none", Msg::EditorSelectionNone.t())
    };
    CONTROLS.with(|controls| {
        controls
            .borrow_mut()
            .insert(id.into(), (gpui::Role::CheckBox, name.into(), state.into()));
    });
    DESCRIPTIONS.with(|values| {
        values.borrow_mut().insert(id.into(), description.into());
    });
}
#[cfg(feature = "gui-e2e")]
pub fn control(id: &str) -> Option<(gpui::Role, String, String)> {
    CONTROLS.with(|controls| controls.borrow().get(id).cloned())
}
#[cfg(feature = "gui-e2e")]
pub fn description(id: &str) -> Option<String> {
    DESCRIPTIONS.with(|values| values.borrow().get(id).cloned())
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn focus(id: &str) {
    FOCUSED.with(|focused| *focused.borrow_mut() = Some(id.into()));
}
#[cfg(feature = "gui-e2e")]
pub fn take_focus_probe() -> Option<String> {
    FOCUSED.with(|focused| focused.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn record_button_role(id: &str, role: super::button_style::ButtonRole) {
    BUTTON_ROLES.with(|values| values.borrow_mut().insert(id.into(), role));
}
#[cfg(feature = "gui-e2e")]
pub fn button_roles() -> std::collections::HashMap<String, super::button_style::ButtonRole> {
    BUTTON_ROLES.with(|values| values.borrow().clone())
}

#[cfg(feature = "gui-e2e")]
pub(crate) fn begin_frame() {
    CONTROLS.with(|values| values.borrow_mut().clear());
    DESCRIPTIONS.with(|values| values.borrow_mut().clear());
    BUTTON_ROLES.with(|values| values.borrow_mut().clear());
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_status(id: &str, name: &str) {
    CONTROLS.with(|values| {
        values
            .borrow_mut()
            .insert(id.into(), (gpui::Role::Status, name.into(), String::new()));
    });
}
#[cfg(feature = "gui-e2e")]
pub(crate) fn record_row_count(side: &str, count: usize) {
    ROW_COUNTS.with(|values| {
        let mut counts = values.get();
        counts[match side {
            "current" => 0,
            "incoming" => 1,
            _ => 2,
        }] = count;
        values.set(counts);
    });
}
#[cfg(feature = "gui-e2e")]
pub fn row_counts() -> [usize; 3] {
    ROW_COUNTS.with(std::cell::Cell::get)
}
