//! Observations from actual diff marker renderers, not native VoiceOver evidence.
use std::{cell::RefCell, collections::HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub label: String,
    pub help: String,
}

thread_local! {
    static WARNINGS: RefCell<HashMap<(String, usize), Marker>> = RefCell::new(HashMap::new());
    static ENDINGS: RefCell<HashMap<(String, usize), Marker>> = RefCell::new(HashMap::new());
    static TOOLTIP: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn warning(side: &str, row: usize, label: &str, help: &str) {
    WARNINGS.with(|markers| {
        markers.borrow_mut().insert(
            (side.to_owned(), row),
            Marker {
                label: label.to_owned(),
                help: help.to_owned(),
            },
        )
    });
}

pub(crate) fn ending(side: &str, row: usize, label: &str) {
    ENDINGS.with(|markers| {
        markers.borrow_mut().insert(
            (side.to_owned(), row),
            Marker {
                label: label.to_owned(),
                help: label.to_owned(),
            },
        )
    });
}

pub(crate) fn tooltip(help: &str) {
    TOOLTIP.with(|value| *value.borrow_mut() = Some(help.to_owned()));
}

pub fn warnings() -> HashMap<(String, usize), Marker> {
    WARNINGS.with(|markers| markers.borrow().clone())
}

pub fn endings() -> HashMap<(String, usize), Marker> {
    ENDINGS.with(|markers| markers.borrow().clone())
}

pub fn clear() {
    WARNINGS.with(|markers| markers.borrow_mut().clear());
    ENDINGS.with(|markers| markers.borrow_mut().clear());
    TOOLTIP.with(|value| *value.borrow_mut() = None);
}

pub fn take_tooltip() -> Option<String> {
    TOOLTIP.with(|value| value.borrow_mut().take())
}

pub fn approvals(view: &crate::ui::MainDiffView) -> Vec<kagi_domain::diff::HunkApproval> {
    view.hunk_approvals.as_ref().clone()
}
