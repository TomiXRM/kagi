//! Renderer observations for #1235/#1236; never compiled in ordinary builds.
use std::cell::RefCell;
use std::collections::HashMap;
thread_local! {
    static MODES: RefCell<HashMap<&'static str, (gpui::Role, &'static str, bool)>> = RefCell::default();
    static DESCRIPTION: RefCell<Option<String>> = const { RefCell::new(None) };
    static CONFLICT_SCROLL: RefCell<Option<gpui::ScrollHandle>> = const { RefCell::new(None) };
    static IMAGE_CAPTIONS: RefCell<HashMap<&'static str, String>> = RefCell::default();
    static HEADER: RefCell<Option<HeaderIdentity>> = const { RefCell::new(None) };
    static HEADER_TOOLTIP: RefCell<Option<String>> = const { RefCell::new(None) };
}
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderIdentity {
    pub displayed: String,
    pub role: Option<gpui::Role>,
    pub description: Option<String>,
}
pub(crate) fn record_header(text: &str, role: Option<gpui::Role>, description: Option<&str>) {
    HEADER.with(|slot| {
        *slot.borrow_mut() = Some(HeaderIdentity {
            displayed: text.to_owned(),
            role,
            description: description.map(str::to_owned),
        })
    });
}
pub fn header() -> Option<HeaderIdentity> {
    HEADER.with(|slot| slot.borrow().clone())
}
pub(crate) fn record_header_tooltip(text: &str) {
    HEADER_TOOLTIP.with(|slot| *slot.borrow_mut() = Some(text.to_owned()));
}
pub fn take_header_tooltip() -> Option<String> {
    HEADER_TOOLTIP.with(|slot| slot.borrow_mut().take())
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
pub(crate) fn record_conflict_scroll(handle: gpui::ScrollHandle) {
    CONFLICT_SCROLL.with(|value| *value.borrow_mut() = Some(handle));
}
pub fn conflict_scroll() -> Option<gpui::ScrollHandle> {
    CONFLICT_SCROLL.with(|value| value.borrow().clone())
}
pub(crate) fn record_image_caption(id: &'static str, text: &str) {
    IMAGE_CAPTIONS.with(|values| values.borrow_mut().insert(id, text.to_owned()));
}
pub fn image_caption(id: &str) -> Option<String> {
    IMAGE_CAPTIONS.with(|values| values.borrow().get(id).cloned())
}
pub fn clear() {
    MODES.with(|values| values.borrow_mut().clear());
    DESCRIPTION.with(|value| *value.borrow_mut() = None);
    CONFLICT_SCROLL.with(|value| *value.borrow_mut() = None);
    IMAGE_CAPTIONS.with(|values| values.borrow_mut().clear());
    HEADER.with(|slot| *slot.borrow_mut() = None);
    HEADER_TOOLTIP.with(|slot| *slot.borrow_mut() = None);
}
