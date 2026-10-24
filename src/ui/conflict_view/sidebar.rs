//! One roving source list with top-down sections and one shared scroll viewport.
use super::*;
use crate::ui::{keyboard_nav, list_a11y};
use kagi_ui_core::file_tree::{build_file_tree_opt, TreeRow};
use std::rc::Rc;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: SharedString,
    depth: usize,
    saved: bool,
    file: Option<usize>,
    count: usize,
}
impl Entry {
    fn key(&self) -> String {
        // A file keeps its focus identity when Save moves it between groups.
        if self.file.is_some() {
            format!("file:{:?}", self.path)
        } else {
            format!("folder:{}:{:?}", self.saved, self.path)
        }
    }
}

#[derive(Default)]
pub(super) struct SidebarState {
    entries: Vec<Entry>,
    visible: Rc<Vec<Entry>>,
    keys: keyboard_nav::RowKeys,
    focus: keyboard_nav::RowFocus,
    logical_scroll: UniformListScrollHandle,
    scroll: gpui::ScrollHandle,
    last_focused: Option<String>,
}

impl ConflictView {
    pub(super) fn rebuild_tree(&mut self) {
        self.sidebar.entries.clear();
        if let Some(mode) = &self.mode {
            for saved in [false, true] {
                let paths: Vec<_> = if saved {
                    mode.session
                        .resolved
                        .iter()
                        .map(|f| (f.path.clone(), None))
                        .collect()
                } else {
                    mode.session
                        .files
                        .iter()
                        .map(|f| (f.path.clone(), None))
                        .collect()
                };
                let mut parents: Vec<PathBuf> = Vec::new();
                for row in build_file_tree_opt(&paths) {
                    let entry = match row {
                        TreeRow::Dir { depth, name } => {
                            parents.truncate(depth);
                            let path = parents
                                .last()
                                .cloned()
                                .unwrap_or_default()
                                .join(name.as_ref());
                            parents.push(path.clone());
                            let count = paths.iter().filter(|(p, _)| p.starts_with(&path)).count();
                            Entry {
                                path,
                                name,
                                depth,
                                saved,
                                file: None,
                                count,
                            }
                        }
                        TreeRow::File {
                            depth,
                            name,
                            file_index,
                            ..
                        } => Entry {
                            path: paths[file_index].0.clone(),
                            name,
                            depth,
                            saved,
                            file: Some(file_index),
                            count: 0,
                        },
                    };
                    self.sidebar.entries.push(entry);
                }
            }
        }
        self.rebuild_visible_tree();
    }

    pub(super) fn initialize_tree_expansion(&mut self) {
        for entry in &self.sidebar.entries {
            if entry.file.is_none() {
                self.sidebar_expanded.insert(entry.path.clone());
            }
        }
        self.rebuild_visible_tree();
    }

    fn rebuild_visible_tree(&mut self) {
        let mut visible = Vec::new();
        let mut blocked: Option<(bool, usize)> = None;
        for entry in &self.sidebar.entries {
            if blocked.is_some_and(|(saved, depth)| saved == entry.saved && entry.depth > depth) {
                continue;
            }
            blocked = None;
            if entry.file.is_none() && !self.sidebar_expanded.contains(&entry.path) {
                blocked = Some((entry.saved, entry.depth));
            }
            visible.push(entry.clone());
        }
        self.sidebar.keys = visible
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key(), i))
            .collect::<Vec<_>>()
            .into();
        self.sidebar.visible = Rc::new(visible);
    }

    fn toggle_folder(&mut self, path: &std::path::Path) {
        if !self.sidebar_expanded.remove(path) {
            self.sidebar_expanded.insert(path.to_path_buf());
        }
        self.rebuild_visible_tree();
    }

    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_hidden = !self.sidebar_hidden;
        if self.sidebar_hidden {
            self.yield_sidebar_focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn yield_sidebar_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar
            .focus
            .yield_focus(self.sidebar_toggle_focus.as_ref(), window, cx);
    }

    pub(super) fn render_sidebar(
        &mut self,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let entries = self.sidebar.visible.clone();
        // Dense eager rows share one viewport; headers scroll with their files.
        let scroll =
            keyboard_nav::RowScroll::Uniform(self.sidebar.logical_scroll.clone(), 0..entries.len());
        let rows = Rc::new(self.sidebar.focus.rows(
            self.sidebar.keys.clone(),
            &scroll,
            self.sidebar_toggle_focus.as_ref(),
            window,
            cx,
        ));
        let focused = self.sidebar.focus.focused(window).map(str::to_owned);
        if focused != self.sidebar.last_focused {
            self.sidebar.last_focused = focused.clone();
            if let Some((at, entry)) = entries
                .iter()
                .enumerate()
                .find(|(_, e)| Some(e.key()) == focused)
            {
                let header_count = if entry.saved { 2 } else { 1 };
                self.sidebar.scroll.scroll_to_item(at + header_count);
            }
        }
        let Some(mode) = self.mode.as_ref() else {
            return div().into_any_element();
        };
        let counts = [
            mode.session.files.len(),
            if self.saved_review_needed {
                0
            } else {
                mode.session.resolved.len()
            },
        ];
        let mut groups = Vec::new();
        for saved in [false, true] {
            let count = counts[usize::from(saved)];
            let header_id = if saved {
                "conflict-saved-header"
            } else {
                "conflict-pending-header"
            };
            groups.push(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_shrink_0()
                    .h(theme::scaled_px(28.))
                    .px(theme::scaled_px(16.))
                    .when(saved, |el| el.mt(theme::scaled_px(16.)))
                    .text_size(theme::scaled_px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(theme().text_sub))
                    .child(if saved {
                        Msg::ConflictSaved.t()
                    } else {
                        Msg::ConflictInProgress.t()
                    })
                    .child(count.to_string())
                    .child(e2e::measure_inside(header_id))
                    .into_any_element(),
            );
            for (at, entry) in entries.iter().enumerate().filter(|(_, e)| e.saved == saved) {
                groups.push(self.render_tree_row(
                    entry,
                    at,
                    &rows,
                    active && focused.is_some(),
                    cx,
                ));
            }
        }
        list_a11y::list_box(
            "conflict-tree",
            rows.list(div().id("conflict-tree")),
            Msg::ConflictRemainingFiles.t(),
        )
        .flex()
        .flex_col()
        .h_full()
        .w(theme::scaled_px(232.))
        .flex_shrink_0()
        .min_h(px(0.))
        .bg(rgb(theme().sidebar))
        .border_r_1()
        .border_color(rgb(theme().surface))
        .relative()
        .child(
            div()
                .id("conflict-tree-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&self.sidebar.scroll)
                .children(groups),
        )
        .child(gpui_component::scroll::Scrollbar::vertical(
            &self.sidebar.scroll,
        ))
        .into_any_element()
    }

    fn render_tree_row(
        &self,
        entry: &Entry,
        at: usize,
        rows: &Rc<keyboard_nav::RowList>,
        active: bool,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let mode = self.mode.as_ref().expect("tree has a mode");
        let selected = entry.file.is_some()
            && if entry.saved {
                self.selected_saved.as_ref() == Some(&entry.path)
            } else {
                self.selected_saved.is_none() && mode.selected_file == entry.file
            };
        let expanded = self.sidebar_expanded.contains(&entry.path);
        let path = entry.path.clone();
        let full = SharedString::from(path.to_string_lossy().into_owned());
        let id = SharedString::from(match entry.file {
            Some(idx) if entry.saved => format!("conflict-saved-row-{idx}"),
            Some(idx) => format!("conflict-row-{idx}"),
            None => format!("conflict-folder-{}-{}", entry.saved, path.display()),
        });
        let file = entry.file;
        let saved = entry.saved;
        let click_path = path.clone();
        let pointer_focus = rows.clone();
        let click = cx.listener(
            move |view: &mut ConflictView, _: &gpui::ClickEvent, window, cx| {
                pointer_focus.focus_row(at, window, cx);
                if let Some(idx) = file {
                    if saved {
                        view.conflict_open_saved(idx, cx);
                    } else {
                        view.conflict_open_file(idx, cx);
                    }
                } else {
                    view.toggle_folder(&click_path);
                }
                cx.notify();
            },
        );
        let keys = rows.clone();
        let entries = self.sidebar.visible.clone();
        let row = div()
            .id(id.clone())
            .track_focus(
                &rows
                    .focus_handle(at)
                    .clone()
                    .tab_index(0)
                    .tab_stop(rows.is_tab_stop(at)),
            )
            .flex()
            .items_center()
            .gap(theme::scaled_px(4.))
            .flex_shrink_0()
            .h(theme::scaled_px(24.))
            .mx(theme::scaled_px(8.))
            .pl(theme::scaled_px(8. + entry.depth as f32 * 16.))
            .pr(theme::scaled_px(8.))
            .rounded(theme::scaled_px(6.))
            .text_size(theme::scaled_px(13.))
            .text_color(if selected {
                theme().selection_foreground(active)
            } else {
                rgb(theme().text_main).into()
            })
            .when(selected, |el| el.bg(theme().selected_for_window(active)))
            .when(!selected, |el| el.hover(|s| s.bg(rgb(theme().bg_row_alt))))
            .active(|s| s.bg(rgb(theme().bg_row_alt)))
            .cursor_pointer()
            .on_click(click)
            .on_key_down(
                cx.listener(move |view, event: &gpui::KeyDownEvent, window, cx| {
                    #[cfg(feature = "gui-e2e")]
                    if event.keystroke.key == "f19" {
                        NEXT_CONFLICT_FOCUS.with(|focused| focused.set(true));
                    }
                    let key = event.keystroke.key.as_str();
                    if matches!(key, "left" | "right") {
                        let entry = &entries[at];
                        if entry.file.is_none() {
                            let open = view.sidebar_expanded.contains(&entry.path);
                            if open != (key == "right") {
                                view.toggle_folder(&entry.path);
                            }
                        }
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    let next = match key {
                        "up" => Some(at.saturating_sub(1)),
                        "down" => Some((at + 1).min(entries.len().saturating_sub(1))),
                        "home" => Some(0),
                        "end" => Some(entries.len().saturating_sub(1)),
                        "pageup" => Some(at.saturating_sub(10)),
                        "pagedown" => Some((at + 10).min(entries.len().saturating_sub(1))),
                        _ => None,
                    };
                    if let Some(next) = next {
                        keys.focus_row(next, window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    } else {
                        keyboard_nav::stop_activation_keys(event, window, cx);
                    }
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::scaled_px(4.))
                    .flex_shrink_0()
                    .child(
                        gpui_component::Icon::new(if expanded {
                            gpui_component::IconName::ChevronDown
                        } else {
                            gpui_component::IconName::ChevronRight
                        })
                        .size(theme::scaled_px(10.))
                        .text_color(rgb(theme().text_sub))
                        .when(file.is_some(), |el| el.invisible()),
                    )
                    .child(
                        gpui::svg()
                            .path(if file.is_some() {
                                "icons/file-text.svg"
                            } else {
                                "icons/folder-open.svg"
                            })
                            .size(theme::scaled_px(14.))
                            .text_color(rgb(if file.is_some() {
                                theme().text_sub
                            } else {
                                theme().ui_accent()
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .child(entry.name.clone()),
            );
        let row = if file.is_none() && !expanded {
            row.child(chip(&entry.count.to_string(), theme().text_main))
        } else if saved && file.is_some() && self.saved_review_needed {
            row.child(letter_badge("!", theme().color_warning, false))
        } else if !saved && file.is_some() && flow::draft_ready(mode, &path) {
            row.child(letter_badge("M", theme().change_modified, true))
        } else if !saved && file.is_some() {
            row.child(letter_badge("C", theme().color_blocker, true))
        } else {
            row
        };
        let row = list_a11y::list_option(
            "conflict-tree",
            rows.navigation(at, row),
            at,
            self.sidebar.visible.len(),
            full.clone(),
            selected,
        )
        .when(file.is_none(), |el| el.aria_expanded(expanded))
        .relative()
        .child(e2e::measure_inside(id.clone()));
        row.hover_tooltip(id, full).into_any_element()
    }
}

pub(super) fn chip(text: &str, color: u32) -> gpui::AnyElement {
    div()
        .flex_shrink_0()
        .px(theme::scaled_px(4.))
        .rounded_full()
        .border_1()
        .border_color(rgb(theme().text_sub).opacity(0.25))
        .bg(rgb(theme().surface))
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(color))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

fn letter_badge(label: &'static str, color: u32, filled: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .size(theme::scaled_px(14.))
        .rounded(theme::scaled_px(3.))
        .text_size(theme::scaled_px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(if filled { 0xffffff } else { color }))
        .when(filled, |el| el.bg(rgb(color)))
        .child(label)
}
