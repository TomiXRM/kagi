//! Renderer-fed shortcut captions; no binding or display synthesis in the oracle.
use gpui::SharedString;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static SHORTCUTS: std::cell::RefCell<std::collections::BTreeMap<usize, SharedString>> =
        std::cell::RefCell::new(Default::default());
}

pub(crate) fn shortcut(index: usize, text: String) -> SharedString {
    let text = SharedString::from(text);
    #[cfg(feature = "gui-e2e")]
    SHORTCUTS.with(|values| values.borrow_mut().insert(index, text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = index;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_shortcut(index: usize) -> Option<SharedString> {
    SHORTCUTS.with(|values| values.borrow().get(&index).cloned())
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct RecordedSection {
    pub role: gpui::Role,
    pub label: String,
    pub rows: std::ops::Range<usize>,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static SECTIONS: std::cell::RefCell<std::collections::BTreeMap<&'static str, RecordedSection>> =
        std::cell::RefCell::new(Default::default());
    static DESCRIPTIONS: std::cell::RefCell<std::collections::BTreeMap<usize, String>> =
        std::cell::RefCell::new(Default::default());
}

pub(crate) fn begin_sections() {
    #[cfg(feature = "gui-e2e")]
    {
        SECTIONS.with(|values| values.borrow_mut().clear());
        DESCRIPTIONS.with(|values| values.borrow_mut().clear());
    }
}

pub(crate) fn section(
    id: &'static str,
    label: &'static str,
    rows: std::ops::Range<usize>,
    group: gpui::Stateful<gpui::Div>,
) -> gpui::Stateful<gpui::Div> {
    use gpui::StatefulInteractiveElement as _;
    let role = gpui::Role::Group;
    #[cfg(feature = "gui-e2e")]
    SECTIONS.with(|values| {
        values.borrow_mut().insert(
            id,
            RecordedSection {
                role,
                label: label.into(),
                rows,
            },
        )
    });
    #[cfg(not(feature = "gui-e2e"))]
    let _ = (id, rows);
    group.role(role).aria_label(label)
}

pub(crate) fn description(index: usize, text: String) -> String {
    #[cfg(feature = "gui-e2e")]
    DESCRIPTIONS.with(|values| values.borrow_mut().insert(index, text.clone()));
    #[cfg(not(feature = "gui-e2e"))]
    let _ = index;
    text
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_sections() -> std::collections::BTreeMap<&'static str, RecordedSection> {
    SECTIONS.with(|values| values.borrow().clone())
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_description(index: usize) -> Option<String> {
    DESCRIPTIONS.with(|values| values.borrow().get(&index).cloned())
}
