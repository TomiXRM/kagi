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
    file_tree::TreeRow,
    i18n::{self, Msg},
    motion::{self, RowSplice},
    theme::{self, theme},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
mod presentation;
use presentation::{file_content, Motion};

const LIST: &str = "inspector-files-scroll";
const FOLD: &str = "\0generated";
const ROW_HEIGHT: f32 = super::sidebar::SIDEBAR_ROW_H;

impl KagiApp {
    pub(super) fn settle_file_list_motion(
        &mut self,
        owner: crate::app::SessionId,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.ui.get(&owner) else {
            return;
        };
        ui.inspector_model.keyboard.settle_motion();
        if let Some(panel) = ui.commit_panel.clone() {
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
actions!(
    kagi_inspector_files,
    [
        ExpandGenerated,
        CollapseGenerated,
        ExpandDirectory,
        CollapseDirectory
    ]
);
pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", ExpandGenerated, Some("KagiInspectorGenerated")),
        KeyBinding::new("left", CollapseGenerated, Some("KagiInspectorGenerated")),
        KeyBinding::new("right", ExpandDirectory, Some("KagiInspectorDirectory")),
        KeyBinding::new("left", CollapseDirectory, Some("KagiInspectorDirectory")),
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
#[derive(Clone)]
enum Entry {
    Directory {
        name: SharedString,
        depth: usize,
        key: SharedString,
        slot: usize,
        expanded: bool,
    },
    File(FileRow),
    Generated {
        slot: usize,
        count: usize,
    },
    More(usize),
    Unavailable,
}
impl Entry {
    fn key(&self) -> SharedString {
        match self {
            Self::Directory { key, .. } => key.clone(),
            Self::File(row) => row.source.files[row.index].path.clone(),
            Self::Generated { .. } => FOLD.into(),
            Self::More(_) => "\0more".into(),
            Self::Unavailable => "\0unavailable".into(),
        }
    }
}

struct Projection {
    entries: Vec<Entry>,
    keys: Vec<(String, usize)>,
    order: HashMap<SharedString, usize>,
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
    collapsed: HashSet<SharedString>,
    dirty: bool,
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
            collapsed: HashSet::new(),
            dirty: false,
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
        let Projection {
            entries,
            keys,
            order,
        } = entries(files, tree, expanded, &self.collapsed);
        if tree && files.is_some() {
            self.collapsed.retain(|key| order.contains_key(key));
        }
        let same_layout = self
            .layout
            .is_some_and(|(old_tree, _, old_lang)| old_tree == tree && old_lang == i18n::lang());
        {
            let mut motion = self.motion.borrow_mut();
            if same_layout {
                motion.sync_entries(&self.entries, &entries, &order);
            } else {
                *motion = Motion::default();
            }
            motion.order = order;
        }
        if !same_layout {
            self.scroll
                .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
            *self.drawn.borrow_mut() = 0..0;
        }
        self.entries = entries.into();
        self.keys = keys.into();
        self.layout = Some((tree, expanded, i18n::lang()));
        self.dirty = false;
    }
}
#[derive(Default)]
pub(super) struct InspectorKeyboard(RefCell<State>);
impl InspectorKeyboard {
    pub(super) fn invalidate(&self) {
        self.0.borrow_mut().layout = None;
    }
    pub(super) fn reset_directories(&self) {
        let mut state = self.0.borrow_mut();
        state.collapsed.clear();
        state.layout = None;
    }
    fn set_directory(&self, key: SharedString, expanded: Option<bool>) {
        let mut state = self.0.borrow_mut();
        let open = !state.collapsed.contains(&key);
        let target = expanded.unwrap_or(!open);
        if target != open {
            if target {
                state.collapsed.remove(&key);
            } else {
                state.collapsed.insert(key);
            }
            state.dirty = true;
        }
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
        if state.dirty || state.layout != Some((tree, expanded, i18n::lang())) {
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
            move |this: &mut KagiApp, requested: std::ops::Range<usize>, _, cx| {
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
                for ix in range.clone() {
                    let sample = motion.sample(&entries[ix], pitch, now);
                    let wash = match &entries[ix] {
                        Entry::File(row) => motion
                            .washes
                            .get(&row.source.files[row.index].path)
                            .map_or(0., |wash| wash.sample(now).value),
                        _ => 0.,
                    };
                    let content = render_entry(ix, &entries[ix], &rows, presentation, wash, cx);
                    let click_key = entries[ix].key();
                    let root = this.root_focus.clone();
                    let mut live = Some(
                        div()
                            .id(click_key)
                            .relative()
                            .top(sample.offset)
                            .opacity(sample.opacity)
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                if !window.default_prevented() {
                                    if let Some(root) = &root {
                                        root.focus(window, cx);
                                    }
                                    window.prevent_default();
                                }
                            })
                            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(content)
                            .into_any_element(),
                    );
                    let mut slot = div().relative().h(theme::scaled_px(ROW_HEIGHT)).w_full();
                    for (at, ghost) in motion.ghosts.iter().enumerate() {
                        let host = if range.contains(&ghost.after_index) {
                            ghost.after_index
                        } else {
                            range.start
                        };
                        if ix != host {
                            continue;
                        }
                        if let Some(ghost_sample) = motion.splice.ghost(
                            &ghost.key,
                            ghost.after.as_ref(),
                            motion.ghosts[..at]
                                .iter()
                                .filter(|previous| previous.after == ghost.after)
                                .map(|previous| &previous.key),
                            pitch,
                            now,
                        ) {
                            let wash = motion
                                .washes
                                .get(&ghost.key)
                                .map_or(0., |wash| wash.sample(now).value);
                            let content = match &ghost.entry {
                                Entry::File(row) => file_content(row, presentation, wash, false),
                                Entry::Directory {
                                    name,
                                    depth,
                                    expanded,
                                    ..
                                } => presentation::directory_content(name, *depth, *expanded)
                                    .into_any_element(),
                                _ => continue,
                            };
                            // Rehosting an earlier exit must preserve logical paint order.
                            if ghost.after_index >= ix {
                                slot = slot.children(live.take());
                            }
                            slot = slot.child(
                                div()
                                    .absolute()
                                    .top(
                                        ghost_sample.offset
                                            + pitch * (ghost.after_index as f32 - ix as f32)
                                            + if ghost.after.is_some() {
                                                motion
                                                    .sample(&entries[ghost.after_index], pitch, now)
                                                    .offset
                                            } else {
                                                px(0.)
                                            },
                                    )
                                    .left_0()
                                    .w_full()
                                    .h(theme::scaled_px(ROW_HEIGHT))
                                    .border_2()
                                    .border_color(gpui::transparent_black())
                                    .opacity(ghost_sample.opacity)
                                    .child(content),
                            );
                        }
                    }
                    slot = slot.children(live);
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
    collapsed: &HashSet<SharedString>,
) -> Projection {
    let mut entries = Vec::new();
    let mut keys = Vec::new();
    let mut order = HashMap::new();
    let Some(files) = files else {
        return Projection {
            entries: vec![Entry::Unavailable],
            keys,
            order,
        };
    };
    if tree {
        let mut parents: Vec<std::path::PathBuf> = Vec::new();
        let mut hidden_below = None;
        for row in &files.tree {
            let depth = match row {
                TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. } => *depth,
            };
            if hidden_below.is_some_and(|parent| depth <= parent) {
                hidden_below = None;
            }
            match row {
                TreeRow::Dir { name, .. } => {
                    parents.truncate(depth);
                    let mut path = parents.last().cloned().unwrap_or_default();
                    path.push(name.as_ref());
                    let key: SharedString = format!("\0directory/{}", path.display()).into();
                    parents.push(path);
                    order.insert(key.clone(), order.len());
                    if hidden_below.is_none() {
                        let expanded = !collapsed.contains(&key);
                        let slot = keys.len();
                        keys.push((key.to_string(), entries.len()));
                        entries.push(Entry::Directory {
                            name: super::render_helpers::safe_text(name),
                            depth,
                            key,
                            slot,
                            expanded,
                        });
                        if !expanded {
                            hidden_below = Some(depth);
                        }
                    }
                }
                TreeRow::File {
                    file_index, name, ..
                } => {
                    order.insert(files.files[*file_index].path.clone(), order.len());
                    if hidden_below.is_none() {
                        push_file(&mut entries, &mut keys, files, *file_index, name, depth);
                    }
                }
            }
        }
    } else {
        for (ix, file) in files
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| !file.generated)
        {
            order.insert(file.path.clone(), order.len());
            push_file(&mut entries, &mut keys, files, ix, &file.path, 0);
        }
    }
    if files.generated_count > 0 {
        order.insert(FOLD.into(), order.len());
        let slot = keys.len();
        keys.push((FOLD.into(), entries.len()));
        entries.push(Entry::Generated {
            slot,
            count: files.generated_count,
        });
        for (ix, file) in files
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.generated)
        {
            order.insert(file.path.clone(), order.len());
            if expanded {
                push_file(&mut entries, &mut keys, files, ix, &file.path, 1);
            }
        }
    }
    if files.hidden > 0 {
        order.insert("\0more".into(), order.len());
        entries.push(Entry::More(files.hidden));
    }
    Projection {
        entries,
        keys,
        order,
    }
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
        Entry::Directory {
            name,
            depth,
            key,
            slot,
            expanded,
        } => {
            let (slot, depth, expanded) = (*slot, *depth, *expanded);
            let focus = rows.clone();
            let click = key.clone();
            let open = key.clone();
            let close = key.clone();
            let id = format!(
                "inspector-directory-{}",
                key.trim_start_matches("\0directory/")
            );
            let row = rows
                .row(slot, div().id(SharedString::from(id.clone())))
                .key_context("KagiRowList KagiInspectorDirectory")
                .role(gpui::Role::TreeItem)
                .aria_label(name.clone())
                .aria_level(depth + 1)
                .aria_expanded(expanded)
                .h(theme::scaled_px(ROW_HEIGHT))
                .hover(|style| style.bg(rgb(theme().surface)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    focus.focus_row(slot, window, cx);
                    this.ui()
                        .inspector_model
                        .keyboard
                        .set_directory(click.clone(), None);
                    cx.notify();
                }))
                .on_action(cx.listener(move |this, _: &ExpandDirectory, _, cx| {
                    this.ui()
                        .inspector_model
                        .keyboard
                        .set_directory(open.clone(), Some(true));
                    cx.notify();
                }))
                .on_action(cx.listener(move |this, _: &CollapseDirectory, _, cx| {
                    this.ui()
                        .inspector_model
                        .keyboard
                        .set_directory(close.clone(), Some(false));
                    cx.notify();
                }))
                .child(presentation::directory_content(name, depth, expanded));
            let row = super::e2e::measure_control(format!("inspector-dir-{ix}"), row);
            super::e2e::measure_control(id, row)
        }
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
