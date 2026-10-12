//! Feature-only report transport and observation for native worktree inspection.

use std::{cell::RefCell, path::Path};

use kagi_git::worktree_inspection::WorktreeInspection;

use super::super::{sidebar_worktree_row, KagiApp};

thread_local! {
    static REPORT: RefCell<Option<gpui::Task<WorktreeInspection>>> = const { RefCell::new(None) };
}

pub fn queue(task: gpui::Task<WorktreeInspection>) {
    REPORT.with(|slot| {
        assert!(
            slot.borrow_mut().replace(task).is_none(),
            "unconsumed worktree inspection"
        );
    });
}

pub(crate) fn take() -> Option<gpui::Task<WorktreeInspection>> {
    REPORT.with(|slot| slot.borrow_mut().take())
}

pub fn status(
    app: &KagiApp,
    path: &Path,
) -> (
    bool,
    Option<u64>,
    Option<kagi_domain::remove::WorktreeRemovalVerdict>,
) {
    sidebar_worktree_row::inspection_status(app, path)
}

#[derive(Clone)]
pub struct ActionProbe {
    pub role: gpui::Role,
    pub name: gpui::SharedString,
    pub focus: Option<gpui::FocusHandle>,
}

thread_local! {
    static ACTIONS: RefCell<std::collections::HashMap<&'static str, ActionProbe>> =
        RefCell::new(Default::default());
    static REFRESH_ACTIVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Values passed to the actual renderer, including the absence of old controls' focus.
pub(crate) fn record_action(
    id: &'static str,
    role: gpui::Role,
    name: gpui::SharedString,
    focus: Option<gpui::FocusHandle>,
) {
    ACTIONS.with(|values| {
        values
            .borrow_mut()
            .insert(id, ActionProbe { role, name, focus })
    });
}

pub fn action(id: &str) -> Option<ActionProbe> {
    ACTIONS.with(|values| values.borrow().get(id).cloned())
}

pub fn clear_actions() {
    ACTIONS.with(|values| values.borrow_mut().clear());
}

pub(crate) fn record_refresh_activation() {
    REFRESH_ACTIVATIONS.with(|count| count.set(count.get() + 1));
}

pub fn take_refresh_activations() -> usize {
    REFRESH_ACTIVATIONS.with(|count| count.replace(0))
}
