//! Stable-path Inspector row ownership, independent of main-diff visibility.
use super::{
    inspector_model::InspectorFiles,
    keyboard_nav::{RowFocus, RowKeys, RowList, RowScroll},
    KagiApp,
};
use gpui::{
    actions, div, prelude::*, px, relative, rgb, App, Context, KeyBinding, MouseButton,
    SharedString, UniformListScrollHandle, Window,
};
use kagi_ui_core::{
    file_tree::{status_badge, TreeRow},
    i18n::{self, Msg},
    motion::{self, RowSplice},
    theme::{self, theme},
};
use std::{cell::RefCell, rc::Rc};
mod presentation;
use presentation::Motion;

const LIST: &str = "inspector-files-scroll";
const FOLD: &str = "\0generated";
const ROW_HEIGHT: f32 = super::sidebar::SIDEBAR_ROW_H;

impl KagiApp {
    pub(super) fn settle_file_list_motion(&mut self, cx: &mut Context<Self>) {
        self.ui().inspector_model.keyboard.settle_motion();
        if let Some(panel) = self.ui().commit_panel.clone() {
            panel.update(cx, |panel, _| panel.settle_file_motion());
        }
    }
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn inspector_row_height_for_e2e() -> gpui::Pixels {
        theme::scaled_px(ROW_HEIGHT)
    }
    pub fn inspector_scroll_for_e2e(&self) -> UniformListScrollHandle {
        self.ui().inspector_model.keyboard.0.borrow().scroll.clone()
    }
    pub fn inspector_file_index_for_e2e(&self, path: &str) -> Option<usize> {
        self.ui()
            .inspector_model
            .files()?
            .files
            .iter()
            .position(|file| file.path.as_ref() == path)
    }
}
actions!(kagi_inspector_files, [ExpandGenerated, CollapseGenerated]);
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", ExpandGenerated, Some("KagiInspectorGenerated")),
        KeyBinding::new("left", CollapseGenerated, Some("KagiInspectorGenerated")),
    ]);
}
#[derive(Clone)]
struct FileRow {
    index: usize,
    source: Rc<InspectorFiles>,
    name: SharedString,
    depth: usize,
    slot: usize,
    label: SharedString,
}
enum Entry {
    Directory {
        name: SharedString,
        depth: usize,
        key: SharedString,
    },
    File(FileRow),
    Generated {
        slot: usize,
        count: usize,
    },
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
    scroll: UniformListScrollHandle,
    drawn: Rc<RefCell<std::ops::Range<usize>>>,
    motion: Rc<RefCell<Motion>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            layout: None,
            entries: Rc::from([]),
            keys: Rc::from([]),
            focus: RowFocus::default(),
            scroll: UniformListScrollHandle::new(),
            drawn: Rc::new(RefCell::new(0..0)),
            motion: Rc::new(RefCell::new(Motion::default())),
        }
    }
}

impl State {
    fn rebuild(&mut self, files: Option<&Rc<InspectorFiles>>, tree: bool, expanded: bool) {
        let now = motion::now();
        let instant = motion::instant();
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
        {
            let mut motion = self.motion.borrow_mut();
            if splice_start.is_some() && self.layout.is_some_and(|(_, old, _)| old != expanded) {
                if expanded {
                    motion.ghosts.clear();
                } else {
                    motion.ghosts = self
                        .entries
                        .iter()
                        .filter_map(|entry| match entry {
                            Entry::File(row) if row.source.files[row.index].generated => {
                                Some(row.clone())
                            }
                            _ => None,
                        })
                        .collect();
                }
                let changed = files
                    .into_iter()
                    .flat_map(|files| files.files.iter())
                    .filter(|file| file.generated)
                    .map(|file| file.path.clone());
                let (inserted, removed) = if expanded {
                    (Some(changed), None)
                } else {
                    (None, Some(changed))
                };
                if motion.splice.splice(
                    inserted.into_iter().flatten(),
                    removed.into_iter().flatten(),
                    now,
                    instant,
                ) {
                    motion.ghosts.clear();
                }
            } else {
                *motion = Motion::default();
            }
            // Generated children keep their source ordering across fold reversals.
            motion.order.clear();
            if let Some(files) = files {
                for (rank, file) in files
                    .files
                    .iter()
                    .filter(|file| !file.generated)
                    .chain(files.files.iter().filter(|file| file.generated))
                    .enumerate()
                {
                    motion.order.insert(file.path.clone(), rank);
                }
            }
        }
        if splice_start.is_none() {
            self.scroll
                .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
            *self.drawn.borrow_mut() = 0..0;
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
    /// Activation changes presentation owner, not the retained list or scroll.
    pub(super) fn settle_motion(&self) {
        let state = self.0.borrow();
        let mut motion = state.motion.borrow_mut();
        motion.splice = RowSplice::default();
        motion.washes.clear();
        motion.selected = None;
        motion.initialized = false;
        motion.ghosts.clear();
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
        let now = motion::now();
        let instant = motion::instant();
        let motion = state.motion.clone();
        {
            let mut motion = motion.borrow_mut();
            let selected = selected
                .and_then(|index| files.as_ref()?.files.get(index))
                .map(|file| file.path.clone());
            let mut moving = motion.sync_selection(selected, now, instant);
            let splicing = motion.splice.advance(now, instant);
            moving |= splicing;
            if !splicing {
                motion.ghosts.clear();
            }
            motion::request_frame(window, moving);
        }
        let scroll = state.scroll.clone();
        let keys = state.keys.clone();
        let drawn = state.drawn.clone();
        let row_scroll = if matches!(state.entries.last(), Some(Entry::More(_))) {
            RowScroll::UniformWithTail {
                handle: scroll.clone(),
                drawn: drawn.borrow().clone(),
                last_row: keys.last().unwrap().1,
                tail: state.entries.len() - 1,
            }
        } else {
            RowScroll::Uniform(scroll.clone(), drawn.borrow().clone())
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
        let count = entries.len();
        let render_scroll = scroll.clone();
        let render = cx.processor(
            move |_: &mut KagiApp, requested: std::ops::Range<usize>, _, cx| {
                let pitch = render_scroll
                    .0
                    .borrow()
                    .last_item_size
                    .filter(|_| count > 0)
                    .map_or(theme::scaled_px(ROW_HEIGHT), |size| {
                        size.contents.height / count as f32
                    });
                let motion = motion.borrow();
                let range = motion.presentation_range(&entries, requested.clone(), pitch, now);
                *drawn.borrow_mut() = range.clone();
                let rows = Rc::new(rows.as_drawn(&range));
                let mut slots = Vec::with_capacity(requested.len());
                let mut overflow = Vec::new();
                let mut leading = 0;
                for ix in range {
                    let sample = motion.sample(&entries[ix], pitch, now);
                    let wash = match &entries[ix] {
                        Entry::File(row) => motion
                            .washes
                            .get(&row.source.files[row.index].path)
                            .map_or(0., |wash| wash.sample(now).value),
                        _ => 0.,
                    };
                    let content = render_entry(ix, &entries[ix], &rows, presentation, wash, cx);
                    let click_key = match &entries[ix] {
                        Entry::File(row) => row.source.files[row.index].path.clone(),
                        Entry::Directory { key, .. } => key.clone(),
                        Entry::Generated { .. } => FOLD.into(),
                        Entry::More(_) => "\0more".into(),
                        Entry::Unavailable => "\0unavailable".into(),
                    };
                    let mut slot = div()
                        .relative()
                        .h(theme::scaled_px(ROW_HEIGHT))
                        .w_full()
                        .child(
                            div()
                                .id(click_key)
                                .relative()
                                .top(sample.offset)
                                .opacity(sample.opacity)
                                .on_click(|_, _, cx| cx.stop_propagation())
                                .child(content),
                        );
                    if matches!(&entries[ix], Entry::Generated { .. }) {
                        let fold: SharedString = FOLD.into();
                        for (at, ghost) in motion.ghosts.iter().enumerate() {
                            let path = &ghost.source.files[ghost.index].path;
                            if let Some(sample) = motion.splice.ghost(
                                path,
                                Some(&fold),
                                motion.ghosts[..at]
                                    .iter()
                                    .map(|row| &row.source.files[row.index].path),
                                pitch,
                                now,
                            ) {
                                let wash = motion
                                    .washes
                                    .get(path)
                                    .map_or(0., |wash| wash.sample(now).value);
                                slot = slot.child(
                                    div()
                                        .absolute()
                                        .top(sample.offset)
                                        .left_0()
                                        .w_full()
                                        .h(theme::scaled_px(ROW_HEIGHT))
                                        .border_2()
                                        .border_color(gpui::transparent_black())
                                        .opacity(sample.opacity)
                                        .child(file_content(ghost, presentation, wash, false)),
                                );
                            }
                        }
                    }
                    if requested.contains(&ix) {
                        slots.push(slot.into_any_element());
                    } else {
                        let anchor = if ix < requested.start {
                            leading += 1;
                            requested.start
                        } else {
                            requested.end - 1
                        };
                        overflow.push(
                            div()
                                .absolute()
                                .w_full()
                                .top(pitch * (ix as f32 - anchor as f32))
                                .child(slot),
                        );
                    }
                }
                for (at, count, before) in [
                    (0, leading, true),
                    (
                        requested.len().saturating_sub(1),
                        overflow.len() - leading,
                        false,
                    ),
                ] {
                    if count == 0 {
                        continue;
                    }
                    if let Some(slot) = slots.get_mut(at) {
                        let content = std::mem::replace(slot, div().into_any_element());
                        let edge = div().relative().w_full().h(theme::scaled_px(ROW_HEIGHT));
                        let fringe = overflow.drain(..count);
                        *slot = if before {
                            edge.children(fringe).child(content)
                        } else {
                            edge.child(content).children(fringe)
                        }
                        .into_any_element();
                    }
                }
                slots
            },
        );
        let body = gpui::uniform_list("inspector-file-rows", count, move |range, window, cx| {
            render(range, window, cx)
        })
        .track_scroll(&scroll)
        .map(|mut list| {
            list.style().restrict_scroll_to_axis = Some(true);
            list
        })
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
        let mut directory = std::path::PathBuf::new();
        let mut directory_depth = 0;
        for row in &files.tree {
            match row {
                TreeRow::Dir { name, depth } => {
                    while directory_depth > *depth {
                        directory.pop();
                        directory_depth -= 1;
                    }
                    directory.push(name.as_ref());
                    directory_depth += 1;
                    entries.push(Entry::Directory {
                        name: super::render_helpers::safe_text(name),
                        depth: *depth,
                        key: format!("\0directory/{}", directory.display()).into(),
                    });
                }
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
    wash: f32,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    match entry {
        Entry::Directory { name, depth, .. } => div()
            .id(("inspector-dir", ix))
            .role(gpui::Role::TreeItem)
            .aria_label(name.clone())
            .aria_level(depth + 1)
            .aria_expanded(true)
            .h(theme::scaled_px(ROW_HEIGHT))
            .flex()
            .items_center()
            .pl(theme::scaled_px(*depth as f32 * 12.))
            .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
            .text_color(rgb(theme().change_dir))
            .child(name.clone())
            .when(cfg!(feature = "gui-e2e"), |row| {
                row.relative()
                    .child(super::e2e::measure_inside(format!("inspector-dir-{ix}")))
            })
            .into_any_element(),
        Entry::More(count) => super::e2e::measure_control(
            "inspector-files-more",
            div()
                .h(theme::scaled_px(ROW_HEIGHT))
                .flex()
                .items_center()
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .text_color(rgb(theme().text_muted))
                .child(i18n::and_n_more(*count)),
        ),
        Entry::Unavailable => div()
            .h(theme::scaled_px(ROW_HEIGHT))
            .flex()
            .items_center()
            .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
            .text_color(rgb(theme().text_muted))
            .child(Msg::DiffUnavailable.t())
            .into_any_element(),
        Entry::Generated { slot, count } => generated_row(*slot, *count, rows, presentation, cx),
        Entry::File(file) => file_row(file, rows, presentation, wash, cx),
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
        .h(theme::scaled_px(ROW_HEIGHT))
        .line_height(relative(1.))
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
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
    wash: f32,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let FileRow {
        index, slot, depth, ..
    } = row;
    let (index, slot) = (*index, *slot);
    let focus = rows.clone();
    let selected = presentation.selected == Some(index);
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
    .relative()
    .w_full()
    .h(theme::scaled_px(ROW_HEIGHT))
    .when(!selected, |el| {
        el.hover(|style| style.bg(rgb(theme().surface)))
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
    .child(file_content(row, presentation, wash, true));
    super::e2e::measure_leaf(format!("inspector-file-{index}"), row)
}

// Shared painted content; ghosts deliberately do not receive RowList, AX roles,
// focus handles, pointer handlers, tooltips, or semantic-list recorder calls.
fn file_content(
    row: &FileRow,
    presentation: Presentation,
    wash: f32,
    probes: bool,
) -> gpui::AnyElement {
    let index = row.index;
    let file = &row.source.files[index];
    let (badge, color, _) = status_badge(Some(&file.change), false);
    // Intent/AX changes immediately. In-flight labels share readable opaque
    // ink; settled primary/secondary styles remain exactly as before.
    let mut fill = theme().selected_for_window(presentation.active);
    fill.a *= wash;
    let background = gpui::Hsla::from(rgb(theme().panel)).blend(fill);
    let ink = super::diffstat_bar::file_selection_ink(wash, presentation.active, background);
    let primary = ink.map(|(primary, _)| primary);
    let foreground = ink.map(|(_, secondary)| secondary);
    let label = gpui::Hsla::from(rgb(theme().text_main));
    let badge_color = gpui::Hsla::from(rgb(color));
    let stat = if probes {
        super::diffstat_bar::diffstat_unit(index, file.stat.as_ref(), foreground)
    } else {
        super::diffstat_bar::diffstat_unit_inert(file.stat.as_ref(), foreground)
    };
    #[cfg(feature = "gui-e2e")]
    let stat = if probes {
        super::e2e::measure_leaf(format!("inspector-file-stat-{index}"), stat)
    } else {
        stat.into_any_element()
    };
    div()
        .relative()
        .flex()
        .w_full()
        .items_center()
        .gap_1()
        .pl(theme::scaled_px(row.depth as f32 * 12.))
        .h_full()
        .line_height(relative(1.2))
        .rounded_sm()
        .bg(fill)
        .child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .text_color(primary.unwrap_or(badge_color))
                .child(SharedString::from(badge))
                .when(cfg!(feature = "gui-e2e") && probes, |badge| {
                    badge.relative().child(super::e2e::measure_inside(format!(
                        "inspector-file-badge-{index}"
                    )))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
                .text_color(primary.unwrap_or(label))
                .truncate()
                .child(row.name.clone())
                .when(cfg!(feature = "gui-e2e") && probes, |name| {
                    name.relative().child(super::e2e::measure_inside(format!(
                        "inspector-file-name-{index}"
                    )))
                }),
        )
        .child(stat)
        .when(cfg!(feature = "gui-e2e"), |content| {
            content.child(super::e2e::measure_inside(format!(
                "inspector-file-paint-{index}"
            )))
        })
        .when(cfg!(feature = "gui-e2e") && !probes, |content| {
            content.child(super::e2e::measure_inside(format!(
                "inspector-file-ghost-{index}"
            )))
        })
        .into_any_element()
}
