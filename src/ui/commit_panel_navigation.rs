//! Stable file-row focus for the two virtualized Changes sections.
use super::commit_panel::{CommitPanelState, CommitPanelView};
use super::keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll};
use super::menu_keys::RowAnchor;
use gpui::{
    prelude::*, Context, Div, Role, SharedString, Stateful, UniformListScrollHandle, Window,
};
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct ChangesNavigation {
    revision: Option<(u64, bool, bool, bool)>,
    keys: RowKeys,
    file_slots: Vec<Option<usize>>,
    focus: RowFocus,
    drawn: std::ops::Range<usize>,
    pub(crate) rows: Option<Rc<RowList>>,
}

impl ChangesNavigation {
    pub(crate) fn invalidate(&mut self) {
        self.revision = None;
    }

    pub(crate) fn reconcile(
        &mut self,
        state: &CommitPanelState,
        staged: bool,
        scroll: UniformListScrollHandle,
        fallback: Option<&gpui::FocusHandle>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        let revision = (
            state.tree_revision,
            state.tree_view,
            state.generated_expanded,
            state.agent_expanded,
        );
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            let files = if staged {
                &state.staged
            } else {
                &state.unstaged
            };
            self.file_slots.clear();
            self.file_slots.resize(files.len(), None);
            let mut keys = Vec::with_capacity(files.len());
            file_positions(state, staged, |fi, display| {
                self.file_slots[fi] = Some(keys.len());
                keys.push((format!("{staged}:{:?}", files[fi].path), display));
            });
            self.keys = keys.into();
        }
        self.rows = Some(Rc::new(self.focus.rows(
            self.keys.clone(),
            &RowScroll::Uniform(scroll, self.drawn.clone()),
            fallback,
            window,
            cx,
        )));
    }

    pub(crate) fn drawn(&mut self, range: &std::ops::Range<usize>) {
        self.drawn = range.clone();
        self.rows = self.rows.as_ref().map(|rows| Rc::new(rows.as_drawn(range)));
    }

    fn slot(&self, fi: usize) -> Option<usize> {
        self.file_slots.get(fi).copied().flatten()
    }
}

fn file_positions(state: &CommitPanelState, staged: bool, mut push: impl FnMut(usize, usize)) {
    let (tree, normal, generated, artifacts) = if staged {
        (
            &state.staged_tree,
            &state.staged_normal_files,
            &state.staged_gen_files,
            &state.staged_artifact_files,
        )
    } else {
        (
            &state.unstaged_tree,
            &state.unstaged_normal_files,
            &state.unstaged_gen_files,
            &state.unstaged_artifact_files,
        )
    };
    let mut display = if state.tree_view {
        for (i, row) in tree.iter().enumerate() {
            if let super::file_tree::TreeRow::File { file_index, .. } = row {
                push(*file_index, i);
            }
        }
        tree.len()
    } else {
        for (i, &fi) in normal.iter().enumerate() {
            push(fi, i);
        }
        normal.len()
    };
    for (files, expanded) in [
        (generated, state.generated_expanded),
        (artifacts, state.agent_expanded),
    ] {
        if files.is_empty() {
            continue;
        }
        display += 1;
        if expanded {
            for &fi in files {
                push(fi, display);
                display += 1;
            }
        }
    }
}

pub(crate) fn file_name(
    path: &std::path::Path,
    change: Option<&kagi_git::ChangeKind>,
    conflicted: bool,
) -> SharedString {
    let status = kagi_ui_core::tree_a11y::file_status(change, conflicted);
    let path = kagi_domain::text_safety::sanitize_control_bytes(&path.to_string_lossy());
    format!("{status} {path}").into()
}

pub(crate) fn file_row(
    view: &CommitPanelView,
    staged: bool,
    fi: usize,
    tree: bool,
    (name, selected): (SharedString, bool),
    el: Stateful<Div>,
    cx: &mut Context<CommitPanelView>,
) -> Stateful<Div> {
    let nav = if staged {
        &view.staged_navigation
    } else {
        &view.unstaged_navigation
    };
    let Some((rows, at)) = nav.rows.as_ref().zip(nav.slot(fi)) else {
        return el;
    };
    let file = if staged {
        &view.state.staged[fi]
    } else {
        &view.state.unstaged[fi]
    };
    let id = format!("cp-{}-row-{fi}", if staged { "st" } else { "us" });
    let role = if tree {
        Role::TreeItem
    } else {
        Role::ListBoxOption
    };
    let anchor = RowAnchor::default();
    let opener = anchor.clone();
    let rows_click = rows.clone();
    let rows_menu = rows.clone();
    let mut el = rows
        .row(at, el)
        .role(super::e2e::commit_panel_labels::role(&id, role))
        .aria_label(super::e2e::commit_panel_labels::name(&id, name))
        .aria_selected(super::e2e::commit_panel_labels::selected(&id, selected))
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            rows_click.focus_row(at, window, cx)
        })
        .child(super::e2e::measure_inside(id.clone()))
        .child(anchor.probe());
    super::e2e::commit_panel_labels::focus(&id, rows.focus_handle(at));
    if !staged && !view.state.is_conflicted(&file.path) {
        el = el.on_action(cx.listener(
            move |view, _: &super::menu_keys::ContextMenuKey, window, cx| {
                rows_menu.focus_row(at, window, cx);
                view.defer_open_file_menu(fi, opener.point(window), window, cx);
                cx.stop_propagation();
            },
        ));
    }
    el
}
