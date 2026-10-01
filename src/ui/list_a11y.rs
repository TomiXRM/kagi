//! Accessibility for list UIs (#354 slice 3).
//!
//! gpui's `uniform_list` has no accessibility API of its own, but every row
//! it renders is an ordinary element with an id, so the row carries its own
//! role, name, selected state and absolute position (`aria_position_in_set`
//! / `aria_size_of_set`). Rows scrolled out of view are not drawn; position
//! and size are how assistive technology learns the full length. One
//! wrapping element per list carries the list role and name.
//!
//! Selectable lists are `ListBox` with `ListBoxOption` rows. The AccessKit
//! tree only exists while an assistive technology is connected, so Tier A
//! reads what the renderer *set*, recorded here (#797 / #872 pattern).

use gpui::{Div, Role, SharedString, Stateful, StatefulInteractiveElement};
use kagi_ui_core::i18n::Msg;

/// Mark `el` as the list `id`, named `label`.
pub(crate) fn list_box(id: &'static str, el: Stateful<Div>, label: &str) -> Stateful<Div> {
    record_list(id, Role::ListBox, label);
    el.role(Role::ListBox)
        .aria_label(SharedString::from(label.to_string()))
}

/// A clickable trailing list row is a named button, not a list option.
/// GPUI's `on_click` supplies the accessibility Click action as well.
pub(crate) fn list_action(id: &'static str, el: Stateful<Div>, label: &str) -> Stateful<Div> {
    record_list(id, Role::Button, label);
    el.role(Role::Button)
        .aria_label(SharedString::from(label.to_string()))
}

/// Mark `el` as row `position` (0-based) of `size` in list `list`.
pub(crate) fn list_option(
    list: &'static str,
    el: Stateful<Div>,
    position: usize,
    size: usize,
    label: String,
    selected: bool,
) -> Stateful<Div> {
    record_option(list, position, size, &label, selected);
    el.role(Role::ListBoxOption)
        .aria_label(SharedString::from(label))
        .aria_selected(selected)
        .aria_position_in_set(position + 1)
        .aria_size_of_set(size)
}

/// Mark `el` as the tree `id`, named `label`.
pub(crate) fn tree(id: &'static str, el: Stateful<Div>, label: &str) -> Stateful<Div> {
    record_list(id, Role::Tree, label);
    el.role(Role::Tree)
        .aria_label(SharedString::from(label.to_string()))
}

/// Mark `el` as flattened row `index` of tree `list`: a `TreeItem` with its
/// level, expanded state (headers) and 1-based position among `size` siblings.
pub(crate) fn tree_item(
    list: &'static str,
    el: Stateful<Div>,
    index: usize,
    spec: &super::sidebar_a11y::TreeItemSpec,
    (position, size): (usize, usize),
) -> Stateful<Div> {
    record_tree_item(list, index, spec, position, size);
    let el = el
        .role(Role::TreeItem)
        .aria_label(SharedString::from(spec.label.clone()))
        .aria_level(spec.level)
        .aria_position_in_set(position)
        .aria_size_of_set(size);
    match spec.expanded {
        Some(expanded) => el.aria_expanded(expanded),
        None => el,
    }
}

/// Substitute only placeholders in the original translation. Values can
/// themselves contain `{}` (commit subjects and PR titles are user-provided).
fn fill_template(msg: Msg, args: &[&str]) -> String {
    let template = msg.t();
    debug_assert_eq!(template.matches("{}").count(), args.len());
    let mut parts = template.split("{}");
    let mut label =
        String::with_capacity(template.len() + args.iter().map(|arg| arg.len()).sum::<usize>());
    label.push_str(parts.next().unwrap_or_default());
    for (arg, part) in args.iter().zip(parts) {
        label.push_str(arg);
        label.push_str(part);
    }
    label
}

/// One sentence naming a commit row: subject, author, date, short SHA, and
/// the refs that point at it.
pub fn commit_row_label(
    summary: &str,
    author: &str,
    date: &str,
    short_sha: &str,
    refs: &[&str],
) -> String {
    let mut s = fill_template(Msg::A11yCommitRow, &[summary, author, date, short_sha]);
    if !refs.is_empty() {
        s.push_str(&fill_template(Msg::A11yCommitRefs, &[&refs.join(", ")]));
    }
    s
}

/// [`commit_row_label`] for a graph row.
pub(crate) fn commit_label(row: &super::commit_list::CommitRow) -> String {
    let refs: Vec<&str> = row.badges.iter().map(|b| b.label.as_ref()).collect();
    commit_row_label(&row.summary, &row.author, &row.date, &row.short_id, &refs)
}

/// The WIP row of a working tree.
pub fn wip_row_label(name: &str, note: &str) -> String {
    fill_template(Msg::A11yWipRow, &[name, note])
}

/// A stash row.
pub fn stash_row_label(message: &str) -> String {
    Msg::A11yStashRow.t().replacen("{}", message, 1)
}

// ── Tier A recorder (gui-e2e only) ─────────────────────────────────────────

/// What the renderer set on one list and its drawn rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordedList {
    pub role: Option<Role>,
    pub label: String,
    pub size: usize,
    /// position (0-based) → (label, selected), for drawn rows only.
    pub rows: std::collections::BTreeMap<usize, (String, bool)>,
    /// Tree lists: flattened index → (level, expanded, position, size).
    pub tree: std::collections::BTreeMap<usize, (usize, Option<bool>, usize, usize)>,
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static LISTS: std::cell::RefCell<std::collections::HashMap<&'static str, RecordedList>> =
        std::cell::RefCell::new(Default::default());
}

#[cfg(feature = "gui-e2e")]
fn record_list(id: &'static str, role: Role, label: &str) {
    LISTS.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(id).or_default();
        entry.role = Some(role);
        entry.label = label.to_string();
    });
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_list(_id: &'static str, _role: Role, _label: &str) {}

#[cfg(feature = "gui-e2e")]
fn record_option(list: &'static str, position: usize, size: usize, label: &str, selected: bool) {
    LISTS.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(list).or_default();
        entry.size = size;
        entry.rows.insert(position, (label.to_string(), selected));
    });
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_option(
    _list: &'static str,
    _position: usize,
    _size: usize,
    _label: &str,
    _selected: bool,
) {
}

#[cfg(feature = "gui-e2e")]
fn record_tree_item(
    list: &'static str,
    index: usize,
    spec: &super::sidebar_a11y::TreeItemSpec,
    position: usize,
    size: usize,
) {
    LISTS.with(|m| {
        let mut m = m.borrow_mut();
        let entry = m.entry(list).or_default();
        entry.rows.insert(index, (spec.label.clone(), false));
        entry
            .tree
            .insert(index, (spec.level, spec.expanded, position, size));
    });
}
#[cfg(not(feature = "gui-e2e"))]
#[inline]
fn record_tree_item(
    _list: &'static str,
    _index: usize,
    _spec: &super::sidebar_a11y::TreeItemSpec,
    _position: usize,
    _size: usize,
) {
}

/// What the last drawn frame set on list `id` and its rows.
#[cfg(feature = "gui-e2e")]
pub fn recorded_list(id: &str) -> Option<RecordedList> {
    LISTS.with(|m| m.borrow().get(id).cloned())
}

/// Forget recorded lists so the next draw proves presence.
#[cfg(feature = "gui-e2e")]
pub fn clear_recorded_lists() {
    LISTS.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_label_names_subject_author_date_sha_and_refs() {
        let with_refs = commit_row_label(
            "Fix bug",
            "Alice",
            "2 days ago",
            "abc1234",
            &["main", "v1.0"],
        );
        for part in ["Fix bug", "Alice", "2 days ago", "abc1234", "main", "v1.0"] {
            assert!(with_refs.contains(part), "{with_refs:?} lacks {part}");
        }
        let bare = commit_row_label("Fix bug", "Alice", "2 days ago", "abc1234", &[]);
        assert!(!bare.contains("main"));
        assert!(with_refs.starts_with(&bare), "refs are a suffix");
    }

    #[test]
    fn commit_and_wip_labels_preserve_literal_braces_in_user_text() {
        let label = commit_row_label("Fix {} in subject", "Alice", "today", "abc1234", &[]);
        assert!(label.contains("Fix {} in subject"), "{label}");
        assert!(label.contains("Alice"), "{label}");
        let wip = wip_row_label("tree {}", "3 changes");
        assert!(wip.contains("tree {}"), "{wip}");
        assert!(wip.contains("3 changes"), "{wip}");
    }

    #[test]
    fn wip_and_stash_labels_carry_their_text() {
        assert!(wip_row_label("main", "3 changes").contains("main"));
        assert!(wip_row_label("main", "3 changes").contains("3 changes"));
        assert!(stash_row_label("WIP on main").contains("WIP on main"));
    }
}
