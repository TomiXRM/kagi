//! Renderer-fed PR/Issues control observations; not native AX readback.
use crate::ui::button_style::ButtonRole;
use gpui::{Hsla, Role};
use std::{cell::RefCell, collections::HashMap};

#[derive(Clone, Debug)]
pub struct Control {
    pub role: Option<Role>,
    pub name: Option<String>,
    pub toggled: Option<bool>,
    pub fill: Option<Hsla>,
    pub tab_stop: bool,
    pub disabled: bool,
    pub semantic: Option<ButtonRole>,
}

thread_local! {
    static CONTROLS: RefCell<HashMap<String, Control>> = RefCell::new(HashMap::new());
    static ACTIVATIONS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    static OPENED_URL: RefCell<Option<String>> = const { RefCell::new(None) };
    static KEY_FOCUS: RefCell<Option<gpui::ElementId>> = const { RefCell::new(None) };
}

pub fn record_control(
    id: impl Into<String>,
    role: Option<Role>,
    name: Option<&str>,
    toggled: Option<bool>,
    fill: Option<Hsla>,
) {
    CONTROLS.with(|controls| {
        controls.borrow_mut().insert(
            id.into(),
            Control {
                role,
                name: name.map(str::to_owned),
                toggled,
                fill,
                tab_stop: false,
                disabled: false,
                semantic: None,
            },
        );
    });
}

pub fn control(id: &str) -> Option<Control> {
    CONTROLS.with(|controls| controls.borrow().get(id).cloned())
}

pub fn clear_controls() {
    CONTROLS.with(|controls| controls.borrow_mut().clear());
}

pub fn record_activation(id: &str) {
    ACTIVATIONS.with(|counts| *counts.borrow_mut().entry(id.to_owned()).or_default() += 1);
}

pub fn activation_count(id: &str) -> usize {
    ACTIVATIONS.with(|counts| counts.borrow().get(id).copied().unwrap_or(0))
}

pub fn record_button_state(id: &str, tab_stop: bool, disabled: bool, semantic: Option<ButtonRole>) {
    CONTROLS.with(|controls| {
        let mut controls = controls.borrow_mut();
        let control = controls
            .get_mut(id)
            .expect("record_control precedes button state");
        control.tab_stop = tab_stop;
        control.disabled = disabled;
        control.semantic = semantic;
    });
}

pub fn record_opened_url(url: &str) {
    OPENED_URL.with(|value| *value.borrow_mut() = Some(url.to_owned()));
}

pub fn take_opened_url() -> Option<String> {
    OPENED_URL.with(|value| value.borrow_mut().take())
}

pub fn has_text_selection(window: &mut gpui::Window, cx: &mut gpui::App) -> bool {
    use gpui_component::WindowExt;
    window.has_text_selection(cx)
}

pub fn record_key_focus(id: gpui::ElementId) {
    KEY_FOCUS.with(|value| *value.borrow_mut() = Some(id));
}

pub fn key_focus() -> Option<gpui::ElementId> {
    KEY_FOCUS.with(|value| value.borrow().clone())
}

pub fn clear_key_focus() {
    KEY_FOCUS.with(|value| *value.borrow_mut() = None);
}
