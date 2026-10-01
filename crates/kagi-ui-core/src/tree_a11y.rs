//! Accessibility for flattened, virtualized tree rows (#354 slice 3).
//! The renderer keeps the same row elements; levels and sibling positions
//! describe the hierarchy even when off-screen rows have not been drawn.

use gpui::{Div, Role, SharedString, Stateful, StatefulInteractiveElement};
use kagi_domain::status::ChangeKind;

use crate::i18n::Msg;

/// 1-based position among siblings and sibling count for each flattened row.
/// Runs in O(rows) for the sidebar and file trees, including large trees.
pub fn sibling_positions(levels: &[usize]) -> Vec<(usize, usize)> {
    let mut out = vec![(0, 0); levels.len()];
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let close = |run: (usize, Vec<usize>), out: &mut Vec<(usize, usize)>| {
        let size = run.1.len();
        for ix in run.1 {
            out[ix].1 = size;
        }
    };
    for (i, &level) in levels.iter().enumerate() {
        while stack.last().is_some_and(|(l, _)| *l > level) {
            close(stack.pop().expect("checked"), &mut out);
        }
        match stack.last_mut() {
            Some((l, members)) if *l == level => members.push(i),
            _ => stack.push((level, vec![i])),
        }
        out[i].0 = stack.last().expect("pushed").1.len();
    }
    while let Some(run) = stack.pop() {
        close(run, &mut out);
    }
    out
}

/// Name the visible file state, rather than asking a screen reader to infer a
/// status from the single-letter visual badge.
pub fn file_status(change: Option<&ChangeKind>, conflicted: bool) -> &'static str {
    if conflicted {
        return Msg::A11yFileConflicted.t();
    }
    match change {
        Some(ChangeKind::Added) => Msg::A11yFileAdded.t(),
        Some(ChangeKind::Modified) => Msg::A11yFileModified.t(),
        Some(ChangeKind::Deleted) => Msg::A11yFileDeleted.t(),
        Some(ChangeKind::Renamed { .. }) => Msg::A11yFileRenamed.t(),
        Some(ChangeKind::TypeChange) => Msg::A11yFileTypeChanged.t(),
        None => Msg::A11yFileUnchanged.t(),
    }
}

/// Mark the scrolling container as a named tree.
pub fn tree(id: &'static str, el: Stateful<Div>, label: &str) -> Stateful<Div> {
    record_tree(id, label);
    el.role(Role::Tree)
        .aria_label(SharedString::from(label.to_string()))
}

/// Mark one visible row as a tree item. `expanded` is only set for a real
/// fold, and `selected` only for file rows whose pane keeps selection.
#[allow(clippy::too_many_arguments)]
pub fn tree_item(
    id: &'static str,
    el: Stateful<Div>,
    index: usize,
    label: &str,
    level: usize,
    expanded: Option<bool>,
    selected: Option<bool>,
    (position, size): (usize, usize),
) -> Stateful<Div> {
    record_item(id, index, label, level, expanded, selected, position, size);
    let el = el
        .role(Role::TreeItem)
        .aria_label(SharedString::from(label.to_string()))
        .aria_level(level)
        .aria_position_in_set(position)
        .aria_size_of_set(size);
    let el = match expanded {
        Some(value) => el.aria_expanded(value),
        None => el,
    };
    match selected {
        Some(value) => el.aria_selected(value),
        None => el,
    }
}

/// Render-time evidence of the exact attributes set above. AccessKit builds
/// its native tree only when an assistive technology is attached; Tier A uses
/// this oracle, while actual VoiceOver delivery still requires a human check.
#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug, Default)]
pub struct RecordedTree {
    pub label: String,
    pub rows: std::collections::BTreeMap<usize, RecordedTreeItem>,
}

#[cfg(feature = "gui-e2e")]
#[derive(Clone, Debug)]
pub struct RecordedTreeItem {
    pub label: String,
    pub level: usize,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub position: usize,
    pub size: usize,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static TREES: std::cell::RefCell<std::collections::HashMap<&'static str, RecordedTree>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
fn record_tree(id: &'static str, label: &str) {
    TREES.with(|trees| trees.borrow_mut().entry(id).or_default().label = label.to_string());
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_tree(_id: &'static str, _label: &str) {}

#[cfg(feature = "gui-e2e")]
#[allow(clippy::too_many_arguments)]
fn record_item(
    id: &'static str,
    index: usize,
    label: &str,
    level: usize,
    expanded: Option<bool>,
    selected: Option<bool>,
    position: usize,
    size: usize,
) {
    TREES.with(|trees| {
        trees.borrow_mut().entry(id).or_default().rows.insert(
            index,
            RecordedTreeItem {
                label: label.to_string(),
                level,
                expanded,
                selected,
                position,
                size,
            },
        );
    });
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
#[allow(clippy::too_many_arguments)]
fn record_item(
    _id: &'static str,
    _index: usize,
    _label: &str,
    _level: usize,
    _expanded: Option<bool>,
    _selected: Option<bool>,
    _position: usize,
    _size: usize,
) {
}

#[cfg(feature = "gui-e2e")]
pub fn recorded_tree(id: &str) -> Option<RecordedTree> {
    TREES.with(|trees| trees.borrow().get(id).cloned())
}

#[cfg(feature = "gui-e2e")]
pub fn clear_recorded_trees() {
    TREES.with(|trees| trees.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siblings_restart_under_each_parent() {
        // Root A > dir (2) > a, b (3); root b (1) > leaf (2).
        assert_eq!(
            sibling_positions(&[1, 2, 3, 3, 1, 2]),
            vec![(1, 2), (1, 1), (1, 2), (2, 2), (2, 2), (1, 1)]
        );
        assert!(sibling_positions(&[]).is_empty());
    }
}
