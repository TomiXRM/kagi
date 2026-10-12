//! Stable-keyed disclosure navigation and renderer-fed accessibility evidence.
use super::super::keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll};
use gpui::{App, Div, Role, SharedString, Stateful, StatefulInteractiveElement, Window};
use kagi_git::oplog::OpLogEntry;

#[derive(Default)]
pub(in crate::ui) struct OpLogKeyboard {
    keys: RowKeys,
    focus: RowFocus,
    changed: bool,
}

/// Include the owning worktree: persisted IDs are not globally unique.
pub(in crate::ui) fn entry_key(entry: &OpLogEntry) -> String {
    format!(
        "{:?}",
        (
            &entry.repo,
            &entry.worktree,
            entry.id,
            entry.timestamp,
            &entry.op
        )
    )
}

impl OpLogKeyboard {
    pub(super) fn rows(
        &mut self,
        entries: &std::collections::VecDeque<OpLogEntry>,
        scroll: gpui::ListState,
        window: &mut Window,
        cx: &mut App,
    ) -> RowList {
        if self.changed || self.keys.len() != entries.len() {
            self.changed = false;
            self.keys = entries
                .iter()
                .enumerate()
                .map(|(i, entry)| (entry_key(entry), i))
                .collect();
        }
        self.focus.rows(
            self.keys.clone(),
            &RowScroll::List(scroll),
            None,
            window,
            cx,
        )
    }

    pub(super) fn invalidate(&mut self) {
        self.changed = true;
    }
    #[cfg(feature = "gui-e2e")]
    pub(super) fn focused(&self, window: &Window) -> Option<&str> {
        self.focus.focused(window)
    }
}

/// Record exactly the role/name/state installed on the summary.
pub(in crate::ui) fn disclosure(
    el: Stateful<Div>,
    i: usize,
    size: usize,
    name: SharedString,
    expanded: bool,
) -> Stateful<Div> {
    #[cfg(feature = "gui-e2e")]
    CONTROLS.with(|map| {
        map.borrow_mut().insert(
            i,
            RecordedDisclosure {
                role: Role::Button,
                name: name.to_string(),
                expanded,
                position: i + 1,
                size,
            },
        );
    });
    el.role(Role::Button)
        .aria_label(name)
        .aria_expanded(expanded)
        .aria_position_in_set(i + 1)
        .aria_size_of_set(size)
}

/// Public observations are renderer attributes, not a native AX tree.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct RecordedDisclosure {
    pub role: Role,
    pub name: String,
    pub expanded: bool,
    pub position: usize,
    pub size: usize,
}
#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONTROLS: std::cell::RefCell<std::collections::BTreeMap<usize, RecordedDisclosure>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    static COPIES: std::cell::RefCell<std::collections::BTreeMap<usize, (Role, String)>> = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}
#[cfg(feature = "gui-e2e")]
pub fn recorded_disclosure(i: usize) -> Option<RecordedDisclosure> {
    CONTROLS.with(|map| map.borrow().get(&i).cloned())
}
#[cfg(feature = "gui-e2e")]
pub fn recorded_copy(i: usize) -> Option<(Role, String)> {
    COPIES.with(|map| map.borrow().get(&i).cloned())
}
pub(in crate::ui) fn copy_button(el: Stateful<Div>, i: usize, name: String) -> Stateful<Div> {
    #[cfg(feature = "gui-e2e")]
    COPIES.with(|map| {
        map.borrow_mut().insert(i, (Role::Button, name.clone()));
    });
    #[cfg(not(feature = "gui-e2e"))]
    let _ = i;
    el.role(Role::Button).aria_label(SharedString::from(name))
}

#[cfg(feature = "gui-e2e")]
pub(in crate::ui) fn clear() {
    CONTROLS.with(|map| map.borrow_mut().clear());
    COPIES.with(|map| map.borrow_mut().clear());
}
