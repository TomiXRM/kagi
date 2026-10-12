//! Renderer-fed push subject accessibility metadata (#1345).
use gpui::{Role, SharedString};
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Clone, Debug)]
pub struct Subject {
    pub role: Role,
    pub label: SharedString,
}
thread_local! {
    static SUBJECTS: RefCell<BTreeMap<usize, Subject>> = const { RefCell::new(BTreeMap::new()) };
}
pub(crate) fn record(index: usize, label: SharedString) {
    SUBJECTS.with(|rows| {
        rows.borrow_mut().insert(
            index,
            Subject {
                role: Role::Label,
                label,
            },
        )
    });
}
pub fn rows() -> Vec<Subject> {
    SUBJECTS.with(|rows| rows.borrow().values().cloned().collect())
}
pub fn clear() {
    SUBJECTS.with(|rows| rows.borrow_mut().clear());
}
