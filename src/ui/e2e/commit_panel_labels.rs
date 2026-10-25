//! Renderer-fed Commit Panel captions, AX attributes and production focus handles.
//! Focus assertions query the real Window; these records never synthesize UI state.
use gpui::SharedString;
#[cfg(feature = "gui-e2e")]
use std::{cell::RefCell, collections::HashMap};

#[cfg(feature = "gui-e2e")]
thread_local! {
    static LABELS: RefCell<HashMap<&'static str, SharedString>> = RefCell::new(HashMap::new());
}
#[cfg(feature = "gui-e2e")]
thread_local! {
    static NAMES: RefCell<HashMap<String, SharedString>> = RefCell::new(HashMap::new());
    static DESCRIPTIONS: RefCell<HashMap<String, SharedString>> = RefCell::new(HashMap::new());
    static ROLES: RefCell<HashMap<String, gpui::Role>> = RefCell::new(HashMap::new());
    static SELECTED: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    static DISABLED: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    static PRESSED: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
    static FOCUS: RefCell<HashMap<String, gpui::FocusHandle>> = RefCell::new(HashMap::new());
    static RADII: RefCell<HashMap<&'static str, gpui::Pixels>> = RefCell::new(HashMap::new());
    static BADGE_COLORS: RefCell<HashMap<&'static str, (u32, u32, u32)>> = RefCell::new(HashMap::new());
}

pub(crate) fn name(id: &str, text: impl Into<SharedString>) -> SharedString {
    let text = text.into();
    #[cfg(feature = "gui-e2e")]
    NAMES.with(|names| names.borrow_mut().insert(id.into(), text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_name(id: &str) -> Option<SharedString> {
    NAMES.with(|names| names.borrow().get(id).cloned())
}

pub(crate) fn role(id: &str, role: gpui::Role) -> gpui::Role {
    #[cfg(feature = "gui-e2e")]
    ROLES.with(|roles| roles.borrow_mut().insert(id.into(), role));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    role
}

pub(crate) fn selected(id: &str, selected: bool) -> bool {
    #[cfg(feature = "gui-e2e")]
    SELECTED.with(|states| states.borrow_mut().insert(id.into(), selected));
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

pub(crate) fn disabled(id: &str, value: bool) -> bool {
    #[cfg(feature = "gui-e2e")]
    DISABLED.with(|states| states.borrow_mut().insert(id.into(), value));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    value
}

pub(crate) fn pressed(id: &str, value: bool) -> bool {
    #[cfg(feature = "gui-e2e")]
    PRESSED.with(|states| states.borrow_mut().insert(id.into(), value));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    value
}

pub(crate) fn focus(id: &str, handle: &gpui::FocusHandle) {
    #[cfg(feature = "gui-e2e")]
    FOCUS.with(|handles| handles.borrow_mut().insert(id.into(), handle.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = (id, handle);
}

#[cfg(feature = "gui-e2e")]
pub fn focus_handle(id: &str) -> Option<gpui::FocusHandle> {
    FOCUS.with(|handles| handles.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn focused(id: &str, window: &gpui::Window) -> bool {
    focus_handle(id).is_some_and(|handle| handle.is_focused(window))
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_disabled(id: &str) -> Option<bool> {
    DISABLED.with(|states| states.borrow().get(id).copied())
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_pressed(id: &str) -> Option<bool> {
    PRESSED.with(|states| states.borrow().get(id).copied())
}

pub(crate) fn description(id: &str, text: impl Into<SharedString>) -> SharedString {
    let text = text.into();
    #[cfg(feature = "gui-e2e")]
    DESCRIPTIONS.with(|values| values.borrow_mut().insert(id.into(), text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn accessible_description(id: &str) -> Option<SharedString> {
    DESCRIPTIONS.with(|values| values.borrow().get(id).cloned())
}

/// The radius passed to the shared dense row Button, not a pixel sample.
pub(crate) fn radius(id: &'static str, value: gpui::Pixels) -> gpui::Pixels {
    #[cfg(feature = "gui-e2e")]
    RADII.with(|values| values.borrow_mut().insert(id, value));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    value
}

#[cfg(feature = "gui-e2e")]
pub fn rendered_radius(id: &str) -> Option<gpui::Pixels> {
    RADII.with(|values| values.borrow().get(id).copied())
}

/// The underlying row background, caption ink and outline of the unfilled label.
pub(crate) fn badge_colors(
    id: &'static str,
    row_background: u32,
    ink: u32,
    border: u32,
) -> (u32, u32, u32) {
    #[cfg(feature = "gui-e2e")]
    BADGE_COLORS.with(|values| {
        values
            .borrow_mut()
            .insert(id, (row_background, ink, border))
    });
    #[cfg(not(feature = "gui-e2e"))]
    let _ = id;
    (row_background, ink, border)
}

#[cfg(feature = "gui-e2e")]
pub fn rendered_badge_colors(id: &str) -> Option<(u32, u32, u32)> {
    BADGE_COLORS.with(|values| values.borrow().get(id).copied())
}

/// Clear renderer-fed observations before mounting another window.
#[cfg(feature = "gui-e2e")]
pub fn clear() {
    LABELS.with(|values| values.borrow_mut().clear());
    NAMES.with(|values| values.borrow_mut().clear());
    DESCRIPTIONS.with(|values| values.borrow_mut().clear());
    ROLES.with(|values| values.borrow_mut().clear());
    SELECTED.with(|values| values.borrow_mut().clear());
    DISABLED.with(|values| values.borrow_mut().clear());
    PRESSED.with(|values| values.borrow_mut().clear());
    FOCUS.with(|values| values.borrow_mut().clear());
    RADII.with(|values| values.borrow_mut().clear());
    BADGE_COLORS.with(|values| values.borrow_mut().clear());
}

/// Measure the real absolute focus target, including its out-of-flow ring.
pub(crate) fn measure_hit_box(id: &'static str, outset: f32) -> gpui::AnyElement {
    use gpui::{IntoElement, Styled};
    #[cfg(feature = "gui-e2e")]
    {
        gpui::canvas(
            move |bounds, window, _| {
                super::record_control_bounds(window.window_handle().window_id(), id, bounds);
            },
            move |bounds, _, window, _| super::record_control_paint(id, bounds, window),
        )
        .absolute()
        .top(gpui::px(-outset))
        .bottom(gpui::px(-outset))
        .left(gpui::px(-outset))
        .right(gpui::px(-outset))
        .into_any_element()
    }
    #[cfg(not(feature = "gui-e2e"))]
    {
        let _ = (id, outset);
        gpui::div().absolute().size_0().into_any_element()
    }
}
