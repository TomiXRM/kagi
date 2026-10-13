//! Stable-path Inspector row ownership, independent of main-diff visibility.
use super::{
    inspector_model::InspectorFiles,
    keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll},
    KagiApp,
};
use gpui::{
    actions, div, prelude::*, px, rgb, App, Context, KeyBinding, ListState, MouseButton,
    SharedString, Window,
};
use kagi_ui_core::{
    file_tree::{status_badge, TreeRow},
    i18n::{self, Msg},
    theme::{self, theme},
};
use std::{cell::RefCell, rc::Rc};

const LIST: &str = "inspector-files-scroll";
const FOLD: &str = "\0generated";
actions!(kagi_inspector_files, [ExpandGenerated, CollapseGenerated]);
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", ExpandGenerated, Some("KagiInspectorGenerated")),
        KeyBinding::new("left", CollapseGenerated, Some("KagiInspectorGenerated")),
    ]);
}
struct FileRow {
    index: usize,
    source: Rc<InspectorFiles>,
    name: SharedString,
    depth: usize,
    slot: usize,
    label: SharedString,
}
enum Entry {
    Directory { name: SharedString, depth: usize },
    File(FileRow),
    Generated { slot: usize, count: usize },
    More(usize),
    Unavailable,
}
#[derive(Clone, Copy)]
struct Presentation {
    tree: bool,
    expanded: bool,
    selected: Option<usize>,
    active: bool,
}
struct State {
    layout: Option<(bool, bool, i18n::Lang)>,
    entries: Rc<[Entry]>,
    keys: RowKeys,
    focus: RowFocus,
    scroll: ListState,
}
impl Default for State {
    fn default() -> Self {
        Self {
            layout: None,
            entries: Rc::from([]),
            keys: Rc::from([]),
            focus: RowFocus::default(),
            scroll: ListState::new(0, gpui::ListAlignment::Top, px(0.)),
        }
    }
}

impl State {
    fn rebuild(&mut self, files: Option<&Rc<InspectorFiles>>, tree: bool, expanded: bool) {
        let (entries, keys) = entries(files, tree, expanded);
        let splice_start = self
            .layout
            .filter(|&(old_tree, _, old_lang)| old_tree == tree && old_lang == i18n::lang())
            .and_then(|_| {
                self.entries
                    .iter()
                    .position(|entry| matches!(entry, Entry::Generated { .. }))
            })
            .map(|at| at + 1);
        if let Some(start) = splice_start {
            let old_end = self.entries.len()
                - usize::from(matches!(self.entries.last(), Some(Entry::More(_))));
            let new_end =
                entries.len() - usize::from(matches!(entries.last(), Some(Entry::More(_))));
            self.scroll.splice(start..old_end, new_end - start);
        } else {
            self.scroll.reset(entries.len());
        }
        self.entries = entries.into();
        self.keys = keys.into();
        self.layout = Some((tree, expanded, i18n::lang()));
    }
}
#[derive(Default)]
pub(super) struct InspectorKeyboard(RefCell<State>);
impl InspectorKeyboard {
    pub(super) fn invalidate(&self) {
        self.0.borrow_mut().layout = None;
    }
    pub(super) fn yield_focus(
        &self,
        fallback: Option<&gpui::FocusHandle>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.borrow_mut().focus.yield_focus(fallback, window, cx);
    }
    #[cfg(feature = "gui-e2e")]
    pub(super) fn focused(&self, window: &Window) -> Option<String> {
        self.0.borrow().focus.focused(window).map(str::to_owned)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render(
        &self,
        files: Option<Rc<InspectorFiles>>,
        tree: bool,
        expanded: bool,
        selected: Option<usize>,
        active: bool,
        fallback: Option<&gpui::FocusHandle>,
        window: &mut Window,
        cx: &mut Context<KagiApp>,
    ) -> gpui::AnyElement {
        let mut state = self.0.borrow_mut();
        if state.layout != Some((tree, expanded, i18n::lang())) {
            state.rebuild(files.as_ref(), tree, expanded);
        }
        // Unmeasured rows need an extent so wheel/scrollbar input can reach the tail.
        let scroll = state
            .scroll
            .clone()
            .with_uniform_item_height(theme::scaled_px(24.));
        let keys = state.keys.clone();
        let row_scroll = if matches!(state.entries.last(), Some(Entry::More(_))) {
            RowScroll::ListWithTail(scroll.clone(), keys.last().unwrap().1)
        } else {
            RowScroll::List(scroll.clone())
        };
        let rows = Rc::new(state.focus.rows(keys, &row_scroll, fallback, window, cx));
        let entries = state.entries.clone();
        drop(state);
        let presentation = Presentation {
            tree,
            expanded,
            selected,
            active,
        };
        let render = cx.processor(move |_: &mut KagiApp, ix: usize, _, cx| {
            render_entry(ix, &entries[ix], &rows, presentation, cx)
        });
        let body = gpui::list(scroll.clone(), move |ix, window, cx| render(ix, window, cx))
            .flex_1()
            .min_h(px(0.));
        let frame = div()
            .id(LIST)
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .key_context("KagiRowList");
        let frame = if tree {
            super::list_a11y::tree(LIST, frame, Msg::A11yInspectorFiles.t())
        } else {
            super::list_a11y::list_box(LIST, frame, Msg::A11yInspectorFiles.t())
        }
        .child(body)
        .child(super::e2e::measure_inside(LIST));
        super::render_helpers::with_system_vertical_scrollbar(
            "inspector-files-scrollbar",
            &scroll,
            frame,
            cx,
        )
        .into_any_element()
    }
}
fn entries(
    files: Option<&Rc<InspectorFiles>>,
    tree: bool,
    expanded: bool,
) -> (Vec<Entry>, Vec<(String, usize)>) {
    let mut entries = Vec::new();
    let mut keys = Vec::new();
    let Some(files) = files else {
        return (vec![Entry::Unavailable], keys);
    };
    if tree {
        for row in &files.tree {
            match row {
                TreeRow::Dir { name, depth } => entries.push(Entry::Directory {
                    name: super::render_helpers::safe_text(name),
                    depth: *depth,
                }),
                TreeRow::File {
                    file_index,
                    name,
                    depth,
                    ..
                } => push_file(&mut entries, &mut keys, files, *file_index, name, *depth),
            }
        }
    } else {
        for (ix, file) in files
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| !file.generated)
        {
            push_file(&mut entries, &mut keys, files, ix, &file.path, 0);
        }
    }
    if files.generated_count > 0 {
        let slot = keys.len();
        keys.push((FOLD.into(), entries.len()));
        entries.push(Entry::Generated {
            slot,
            count: files.generated_count,
        });
        if expanded {
            for (ix, file) in files
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| file.generated)
            {
                push_file(&mut entries, &mut keys, files, ix, &file.path, 1);
            }
        }
    }
    if files.hidden > 0 {
        entries.push(Entry::More(files.hidden));
    }
    (entries, keys)
}
fn push_file(
    entries: &mut Vec<Entry>,
    keys: &mut Vec<(String, usize)>,
    files: &Rc<InspectorFiles>,
    index: usize,
    name: &str,
    depth: usize,
) {
    let file = &files.files[index];
    let slot = keys.len();
    keys.push((file.path.to_string(), entries.len()));
    let path = kagi_domain::text_safety::sanitize_control_bytes(&file.path);
    let label = format!(
        "{} {path}",
        kagi_ui_core::tree_a11y::file_status(Some(&file.change), false)
    )
    .into();
    entries.push(Entry::File(FileRow {
        index,
        source: files.clone(),
        name: super::render_helpers::safe_text(name),
        depth,
        slot,
        label,
    }));
}
fn render_entry(
    ix: usize,
    entry: &Entry,
    rows: &Rc<RowList>,
    presentation: Presentation,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    match entry {
        Entry::Directory { name, depth } => div()
            .id(("inspector-dir", ix))
            .role(gpui::Role::TreeItem)
            .aria_label(name.clone())
            .aria_level(depth + 1)
            .aria_expanded(true)
            .pl(theme::scaled_px(*depth as f32 * 12.))
            .mb_px()
            .text_sm()
            .text_color(rgb(theme().change_dir))
            .child(name.clone())
            .into_any_element(),
        Entry::More(count) => super::e2e::measure_control(
            "inspector-files-more",
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(i18n::and_n_more(*count)),
        ),
        Entry::Unavailable => div()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(Msg::DiffUnavailable.t())
            .into_any_element(),
        Entry::Generated { slot, count } => generated_row(*slot, *count, rows, presentation, cx),
        Entry::File(file) => file_row(file, rows, presentation, cx),
    }
}
fn generated_row(
    slot: usize,
    count: usize,
    rows: &Rc<RowList>,
    presentation: Presentation,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let focus = rows.clone();
    let label: SharedString = format!("{} ({count})", Msg::GeneratedFilesSection.t()).into();
    let row = rows
        .row(slot, div().id("generated-fold-header"))
        .key_context("KagiRowList KagiInspectorGenerated")
        .role(gpui::Role::Button)
        .aria_label(label.clone())
        .aria_expanded(presentation.expanded)
        .when(presentation.tree, |el| {
            el.role(gpui::Role::TreeItem).aria_level(1)
        })
        .flex()
        .items_center()
        .mt_1()
        .mb_px()
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .on_click(cx.listener(move |this, _, window, cx| {
            focus.focus_row(slot, window, cx);
            this.inspector_generated_expanded = !this.inspector_generated_expanded;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ExpandGenerated, _, cx| {
            this.inspector_generated_expanded = true;
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &CollapseGenerated, _, cx| {
            this.inspector_generated_expanded = false;
            cx.notify();
        }))
        .child(format!(
            "{} {label}",
            if presentation.expanded { "▾" } else { "▸" }
        ));
    super::e2e::measure_control("generated-fold-header", row)
}
fn file_row(
    row: &FileRow,
    rows: &Rc<RowList>,
    presentation: Presentation,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let FileRow {
        index, slot, depth, ..
    } = row;
    let (index, slot) = (*index, *slot);
    let file = &row.source.files[index];
    let (badge, color, _) = status_badge(Some(&file.change), false);
    let focus = rows.clone();
    let selected = presentation.selected == Some(index);
    let selection_on_accent = theme().selection_uses_on_accent(selected, presentation.active);
    let row = super::list_a11y::list_option(
        LIST,
        rows.row(slot, div().id(("inspector-file", index))),
        slot,
        rows.len(),
        row.label.clone(),
        selected,
    )
    .when(presentation.tree, |el| {
        el.role(gpui::Role::TreeItem).aria_level(depth + 1)
    })
    .flex()
    .items_center()
    .gap_1()
    .pl(theme::scaled_px(*depth as f32 * 12.))
    .mb_px()
    .when(!selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
    .when(selected, |el| {
        el.bg(theme().selected_for_window(presentation.active))
            .rounded_sm()
    })
    .on_click(cx.listener(move |this, _, window, cx| {
        focus.focus_row(slot, window, cx);
        this.open_main_diff_inspector_file(index, cx);
        cx.notify();
    }))
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
            this.inspector_file_menu = Some((index, e.position));
            cx.stop_propagation();
            cx.notify();
        }),
    )
    .child(
        div()
            .w(theme::scaled_px(14.))
            .flex_shrink_0()
            .text_sm()
            .text_color(if selection_on_accent {
                theme().selection_foreground(presentation.active)
            } else {
                rgb(color).into()
            })
            .child(SharedString::from(badge)),
    )
    .child(
        div()
            .flex_1()
            .min_w(px(0.))
            .text_sm()
            .text_color(if selection_on_accent {
                theme().selection_foreground(presentation.active)
            } else {
                rgb(theme().text_main).into()
            })
            .truncate()
            .child(row.name.clone()),
    )
    .child(super::diffstat_bar::diffstat_unit(
        index,
        file.stat.as_ref(),
        selection_on_accent.then(|| theme().selection_secondary_foreground(presentation.active)),
    ));
    super::e2e::measure_control(format!("inspector-file-{index}"), row)
}
