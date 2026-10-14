//! Stable file-row focus for the two virtualized Changes sections.
use super::commit_panel::{CommitPanelState, CommitPanelView};
use super::keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll};
use super::menu_keys::RowAnchor;
use gpui::{
    prelude::*, Context, Div, Role, SharedString, Stateful, UniformListScrollHandle, Window,
};
use std::cell::OnceCell;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct ChangesNavigation {
    revision: Option<(u64, bool, bool, bool, super::i18n::Lang)>,
    keys: RowKeys,
    file_slots: Vec<Option<usize>>,
    focus: RowFocus,
    drawn: std::ops::Range<usize>,
    pub(crate) rows: Option<Rc<RowList>>,
    anchor: RowAnchor,
    names: Vec<OnceCell<SharedString>>,
}

/// A processor-local row list; uniform_list's measurement range must not
/// replace the reconciled list or its remembered Tab stop.
pub(crate) struct ChangesRows {
    rows: Option<Rc<RowList>>,
    focused_row: Option<usize>,
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
            super::i18n::lang(),
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
            self.names.clear();
            self.names.resize_with(files.len(), OnceCell::new);
            let mut keys = Vec::with_capacity(files.len());
            file_positions(state, staged, |fi, display| {
                self.file_slots[fi] = Some(keys.len());
                keys.push((format!("{staged}:{:?}", files[fi].path), display));
            });
            self.keys = keys.into();
            // Keep a surviving focused path drawn when a status refresh
            // renumbers it, instead of handing its focus to a different file.
            let focused = self.focus.focused(window);
            if let Some((_, display)) = self
                .keys
                .iter()
                .find(|(key, _)| focused == Some(key.as_str()))
            {
                scroll.scroll_to_item(*display, gpui::ScrollStrategy::Nearest);
            }
        }
        self.rows = Some(Rc::new(self.focus.rows(
            self.keys.clone(),
            &RowScroll::Uniform(scroll, self.drawn.clone()),
            fallback,
            window,
            cx,
        )));
    }

    pub(crate) fn drawn(
        &mut self,
        range: &std::ops::Range<usize>,
        window: &Window,
        cx: &gpui::App,
    ) -> ChangesRows {
        self.drawn = range.clone();
        ChangesRows {
            rows: self.rows.as_ref().map(|rows| Rc::new(rows.as_drawn(range))),
            focused_row: self
                .rows
                .as_ref()
                .and_then(|rows| rows.focused_stop(window, cx)),
        }
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

pub(crate) fn row_name(view: &CommitPanelView, staged: bool, fi: usize) -> SharedString {
    let nav = if staged {
        &view.staged_navigation
    } else {
        &view.unstaged_navigation
    };
    nav.names[fi]
        .get_or_init(|| {
            let file = if staged {
                &view.state.staged[fi]
            } else {
                &view.state.unstaged[fi]
            };
            let status = kagi_ui_core::tree_a11y::file_status(
                Some(&file.change),
                !staged && view.state.is_conflicted(&file.path),
            );
            let path =
                kagi_domain::text_safety::sanitize_control_bytes(&file.path.to_string_lossy());
            format!("{status} {path}").into()
        })
        .clone()
}

pub(crate) fn action_tab_stop(
    view: &CommitPanelView,
    drawn: &ChangesRows,
    staged: bool,
    fi: usize,
) -> bool {
    let nav = if staged {
        &view.staged_navigation
    } else {
        &view.unstaged_navigation
    };
    drawn
        .rows
        .as_ref()
        .zip(nav.slot(fi))
        .is_some_and(|(rows, at)| rows.is_tab_stop(at))
}

pub(crate) fn file_row(
    view: &CommitPanelView,
    drawn: &ChangesRows,
    staged: bool,
    fi: usize,
    selected: bool,
    el: Stateful<Div>,
    cx: &mut Context<CommitPanelView>,
) -> Stateful<Div> {
    let nav = if staged {
        &view.staged_navigation
    } else {
        &view.unstaged_navigation
    };
    let Some((rows, at)) = drawn.rows.as_ref().zip(nav.slot(fi)) else {
        return el;
    };
    let file = if staged {
        &view.state.staged[fi]
    } else {
        &view.state.unstaged[fi]
    };
    #[cfg(feature = "gui-e2e")]
    let id = format!("cp-{}-row-{fi}", if staged { "st" } else { "us" });
    let role = if view.state.tree_view {
        Role::TreeItem
    } else {
        Role::ListBoxOption
    };
    let opener = nav.anchor.clone();
    let rows_click = rows.clone();
    let rows_menu = rows.clone();
    let panel = cx.entity().downgrade();
    let mut el = rows
        .selecting_row(at, el, move |display, window, cx| {
            let _ = panel.update(cx, |view, cx| {
                let mut destination = None;
                file_positions(&view.state, staged, |index, position| {
                    if position == display {
                        destination = Some(index);
                    }
                });
                if let Some(index) = destination {
                    let file = if staged {
                        super::commit_panel::CommitPanelFileRef::Staged { index }
                    } else {
                        super::commit_panel::CommitPanelFileRef::Unstaged { index }
                    };
                    view.defer_select_file(file, window, cx);
                }
            });
        })
        .role(role)
        .aria_label(row_name(view, staged, fi))
        .aria_selected(selected)
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            rows_click.focus_row(at, window, cx)
        });
    if drawn.focused_row == Some(at) {
        el = el.child(nav.anchor.probe());
    }
    #[cfg(feature = "gui-e2e")]
    {
        super::e2e::commit_panel_labels::role(&id, role);
        super::e2e::commit_panel_labels::name(&id, row_name(view, staged, fi));
        super::e2e::commit_panel_labels::selected(&id, selected);
        super::e2e::commit_panel_labels::focus(&id, rows.focus_handle(at));
        el = el.child(super::e2e::measure_inside(id));
    }
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
