use super::commit_panel::CommitPanelView;
use super::commit_panel_rows::*;
use super::render_helpers::*;
use super::*;
use gpui::Pixels;
use kagi_ui_core::motion::{self, Easing, KeyedTransition, RowSplice};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FileKey {
    File(PathBuf),
    Directory(PathBuf),
    Fold(bool),
}

#[derive(Clone)]
struct RowVisual {
    key: FileKey,
    name: SharedString,
    indent: f32,
    change: Option<kagi_git::ChangeKind>,
    conflicted: bool,
    stat: Option<kagi_git::FileDiffStat>,
}

struct RemovedRow {
    visual: RowVisual,
    after: Option<FileKey>,
    between: Vec<FileKey>,
}

/// Presentation-only cache. Rebuilt on a logical list change, never each frame.
#[derive(Default)]
pub(super) struct FileListMotion {
    revision: Option<(u64, bool, bool, bool)>,
    rows: Vec<RowVisual>,
    slots: HashMap<FileKey, usize>,
    order: Vec<FileKey>,
    rank: HashMap<FileKey, usize>,
    removed: Vec<RemovedRow>,
    splice: RowSplice<FileKey>,
    pitch: Pixels,
    selection: HashMap<PathBuf, KeyedTransition<bool>>,
}

impl FileListMotion {
    fn sync(
        &mut self,
        panel: &super::commit_panel::CommitPanelState,
        staged: bool,
        selected: [Option<&PathBuf>; 2],
        viewport_rows: usize,
        now: Instant,
    ) -> bool {
        let revision = (
            panel.tree_revision,
            panel.tree_view,
            panel.generated_expanded,
            panel.agent_expanded,
        );
        let first = self.revision.is_none();
        if self.revision != Some(revision) {
            let rows = motion_rows(panel, staged);
            let slots: HashMap<_, _> = rows
                .iter()
                .enumerate()
                .map(|(i, row)| (row.key.clone(), i))
                .collect();
            let inserted: Vec<_> = rows
                .iter()
                .filter(|row| !self.slots.contains_key(&row.key))
                .map(|row| row.key.clone())
                .collect();
            let removed: Vec<_> = self
                .rows
                .iter()
                .filter(|row| !slots.contains_key(&row.key))
                .cloned()
                .collect();
            let instant = motion::instant()
                || first
                || self
                    .revision
                    .is_some_and(|old| old.0 == 0 || old.1 != revision.1)
                || inserted.len() + removed.len() > viewport_rows.max(1);
            self.splice.advance(now, instant);
            let settled = self.splice.splice(
                inserted.iter().cloned(),
                removed.iter().map(|row| row.key.clone()),
                now,
                instant,
            );
            if settled {
                self.removed.clear();
                self.order = rows.iter().map(|row| row.key.clone()).collect();
            } else {
                self.removed
                    .retain(|row| !slots.contains_key(&row.visual.key));
                for visual in removed {
                    if !self.removed.iter().any(|row| row.visual.key == visual.key) {
                        self.removed.push(RemovedRow {
                            visual,
                            after: None,
                            between: Vec::new(),
                        });
                    }
                }
                let ghosts: HashSet<_> = self
                    .removed
                    .iter()
                    .map(|row| row.visual.key.clone())
                    .collect();
                self.order
                    .retain(|key| slots.contains_key(key) || ghosts.contains(key));
                // Stable keys retain their place across overlapping folds/status
                // batches. New keys precede their nearest known successor.
                let known: HashSet<_> = self.order.iter().cloned().collect();
                for (i, row) in rows.iter().enumerate().rev() {
                    if !known.contains(&row.key) {
                        let at = rows
                            .get(i + 1)
                            .and_then(|next| self.order.iter().position(|key| *key == next.key))
                            .unwrap_or(self.order.len());
                        self.order.insert(at, row.key.clone());
                    }
                }
                let mut after = None;
                let mut between = Vec::new();
                for key in &self.order {
                    if slots.contains_key(key) {
                        after = Some(key.clone());
                        between.clear();
                    } else if let Some(ghost) =
                        self.removed.iter_mut().find(|row| row.visual.key == *key)
                    {
                        ghost.after = after.clone();
                        ghost.between = between.clone();
                        between.push(key.clone());
                    }
                }
            }
            self.rank = self
                .order
                .iter()
                .enumerate()
                .map(|(i, key)| (key.clone(), i))
                .collect();
            self.rows = rows;
            self.slots = slots;
            self.revision = Some(revision);
        }
        for (path, transition) in &mut self.selection {
            let active = selected.contains(&Some(path));
            transition.retarget(
                active,
                if active { 1. } else { 0. },
                now,
                motion::TRANSITION,
                Easing::OutCubic,
                motion::instant(),
            );
        }
        for path in selected.into_iter().flatten() {
            self.selection.entry(path.clone()).or_insert_with(|| {
                let mut transition = KeyedTransition::new(false, 0.);
                transition.retarget(
                    true,
                    1.,
                    now,
                    motion::TRANSITION,
                    Easing::OutCubic,
                    first || motion::instant(),
                );
                transition
            });
        }
        self.selection
            .retain(|_, transition| *transition.key() || !transition.sample(now).done);
        let moving = self.splice.advance(now, motion::instant());
        self.removed.retain(|row| {
            self.splice
                .ghost(
                    &row.visual.key,
                    row.after.as_ref(),
                    row.between.iter(),
                    px(1.),
                    now,
                )
                .is_some()
        });
        moving
            || self
                .selection
                .values()
                .any(|transition| !transition.sample(now).done)
    }

    fn sample(&self, key: &FileKey, now: Instant, height: Pixels) -> motion::RowSample {
        self.splice.row(key, height, now, |other| {
            self.rank.get(other) < self.rank.get(key)
        })
    }

    fn wrap(&self, i: usize, row: gpui::AnyElement, height: Pixels) -> gpui::AnyElement {
        let sample = self.sample(&self.rows[i].key, motion::now(), height);
        if sample.offset == px(0.) && sample.opacity == 1. {
            return row;
        }
        div()
            .w_full()
            .h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)))
            .child(
                div()
                    .relative()
                    .top(sample.offset)
                    .opacity(sample.opacity)
                    .child(row),
            )
            .into_any_element()
    }

    pub(super) fn selection(&self, path: &Path) -> f32 {
        self.selection
            .get(path)
            .map_or(0., |transition| transition.sample(motion::now()).value)
    }

    /// Mount the displaced edge rows that uniform_list's final-slot viewport
    /// would otherwise omit. Expansion stays bounded by the active splice cap.
    fn presentation_range(
        &self,
        requested: std::ops::Range<usize>,
        height: Pixels,
    ) -> std::ops::Range<usize> {
        let mut visible = requested.clone();
        if visible.is_empty() {
            return visible;
        }
        let now = motion::now();
        let mut remaining = motion::MAX_ANIMATED_ROWS;
        while visible.start > 0 && remaining > 0 {
            let i = visible.start - 1;
            if height * (i + 1) as f32 + self.sample(&self.rows[i].key, now, height).offset
                <= height * requested.start as f32
            {
                break;
            }
            visible.start = i;
            remaining -= 1;
        }
        while visible.end < self.rows.len() && remaining > 0 {
            let i = visible.end;
            if height * i as f32 + self.sample(&self.rows[i].key, now, height).offset
                >= height * requested.end as f32
            {
                break;
            }
            visible.end += 1;
            remaining -= 1;
        }
        visible
    }

    fn mount_rows(
        &self,
        requested: std::ops::Range<usize>,
        range: std::ops::Range<usize>,
        height: Pixels,
        mut render: impl FnMut(usize) -> Option<gpui::AnyElement>,
    ) -> Vec<gpui::AnyElement> {
        let mut slots = Vec::with_capacity(requested.len());
        let mut overflow = Vec::new();
        for i in range {
            let Some(row) = render(i) else { continue };
            let row = self.wrap(i, row, height);
            if requested.contains(&i) {
                slots.push(row);
            } else {
                overflow.push(
                    div()
                        .absolute()
                        .w_full()
                        .top(height * (i as f32 - requested.start as f32))
                        .child(row),
                );
            }
        }
        if !overflow.is_empty() {
            if let Some(first) = slots.first_mut() {
                *first = div()
                    .relative()
                    .w_full()
                    .h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)))
                    .child(std::mem::replace(first, div().into_any_element()))
                    .children(overflow)
                    .into_any_element();
            }
        }
        slots
    }

    fn ghosts(&self, staged: bool, scroll: &UniformListScrollHandle) -> gpui::Div {
        let state = scroll.0.borrow();
        let offset = state.base_handle.offset().y;
        let viewport = state.base_handle.bounds().size.height;
        let height = self.pitch;
        let now = motion::now();
        let mut layer = div().absolute().inset_0().overflow_hidden();
        for row in &self.removed {
            let Some(sample) = self.splice.ghost(
                &row.visual.key,
                row.after.as_ref(),
                row.between.iter(),
                height,
                now,
            ) else {
                continue;
            };
            let anchor = row.after.as_ref().map_or(px(0.), |key| {
                height * self.slots[key] as f32 + self.sample(key, now, height).offset
            });
            let top = offset + anchor + sample.offset;
            if top + height < px(0.) || top > viewport {
                continue;
            }
            layer = layer.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(top)
                    .h(height)
                    .opacity(sample.opacity)
                    .child(ghost_content(&row.visual, staged)),
            );
        }
        layer
    }
}

fn list_pitch(scroll: &UniformListScrollHandle, count: usize) -> Pixels {
    scroll
        .0
        .borrow()
        .last_item_size
        .filter(|_| count > 0)
        .map_or(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)), |size| {
            size.contents.height / count as f32
        })
}

fn motion_rows(panel: &super::commit_panel::CommitPanelState, staged: bool) -> Vec<RowVisual> {
    let (files, tree, normal, generated, artifacts) = if staged {
        (
            &panel.staged,
            &panel.staged_tree,
            &panel.staged_normal_files,
            &panel.staged_gen_files,
            &panel.staged_artifact_files,
        )
    } else {
        (
            &panel.unstaged,
            &panel.unstaged_tree,
            &panel.unstaged_normal_files,
            &panel.unstaged_gen_files,
            &panel.unstaged_artifact_files,
        )
    };
    let file = |fi: usize, indent: f32| {
        let f = &files[fi];
        RowVisual {
            key: FileKey::File(f.path.clone()),
            name: f
                .path
                .file_name()
                .unwrap_or(f.path.as_os_str())
                .to_string_lossy()
                .into_owned()
                .into(),
            indent,
            change: Some(f.change.clone()),
            conflicted: !staged && panel.is_conflicted(&f.path),
            stat: if staged {
                panel.staged_stat(&f.path)
            } else {
                panel.unstaged_stat(&f.path)
            }
            .cloned(),
        }
    };
    let mut rows = Vec::new();
    let mut directories = Vec::<PathBuf>::new();
    if panel.tree_view {
        for row in tree {
            match row {
                file_tree::TreeRow::Dir { name, depth } => {
                    directories.truncate(*depth);
                    let path = directories
                        .last()
                        .cloned()
                        .unwrap_or_default()
                        .join(name.as_ref());
                    directories.push(path.clone());
                    rows.push(RowVisual {
                        key: FileKey::Directory(path),
                        name: name.clone(),
                        indent: *depth as f32 * 12.,
                        change: None,
                        conflicted: false,
                        stat: None,
                    });
                }
                file_tree::TreeRow::File {
                    file_index, depth, ..
                } => rows.push(file(*file_index, *depth as f32 * 12.)),
            }
        }
    } else {
        rows.extend(normal.iter().map(|&fi| file(fi, 0.)));
    }
    for (agent, group, expanded) in [
        (false, generated, panel.generated_expanded),
        (true, artifacts, panel.agent_expanded),
    ] {
        if group.is_empty() {
            continue;
        }
        let label = if agent {
            Msg::AgentArtifactsSection.t()
        } else {
            Msg::GeneratedFilesSection.t()
        };
        rows.push(RowVisual {
            key: FileKey::Fold(agent),
            name: format!(
                "{} {label} ({})",
                if expanded { "▾" } else { "▸" },
                group.len()
            )
            .into(),
            indent: 0.,
            change: None,
            conflicted: false,
            stat: None,
        });
        if expanded {
            rows.extend(group.iter().map(|&fi| file(fi, 0.)));
        }
    }
    rows
}

/// No listeners, focus, IDs, accessibility roles or tooltips on exit-only content.
fn ghost_content(row: &RowVisual, staged: bool) -> gpui::Div {
    let mut content = div()
        .h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)))
        .w_full()
        // Reserve the same transparent focus-ring inset as the live RowList
        // row, without retaining a focus handle or a keyboard listener.
        .border_2()
        .border_color(gpui::transparent_black())
        .flex()
        .items_center()
        .pl(theme::scaled_px(8. + row.indent))
        .pr_2()
        .bg(rgb(theme().panel))
        .text_size(theme::scaled_px(theme::FILE_TREE_FONT_PX))
        .line_height(theme::scaled_px(16.))
        .text_color(rgb(theme().text_main));
    if matches!(row.key, FileKey::File(_)) {
        let (badge, color, _) = status_badge(row.change.as_ref(), row.conflicted);
        content = content.child(
            div()
                .w(theme::scaled_px(16.))
                .flex_shrink_0()
                .text_color(rgb(color))
                .child(SharedString::from(badge)),
        );
    }
    content = content
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .child(row.name.clone()),
        )
        .child(diffstat_bar::diffstat_unit_inert(row.stat.as_ref(), None));
    if matches!(row.key, FileKey::File(_)) {
        content = content.child(div().ml_2().text_color(rgb(theme().text_muted)).child(
            if staged {
                Msg::CommitPanelUnstage.t()
            } else {
                Msg::CommitPanelStage.t()
            },
        ));
    }
    #[cfg(feature = "gui-e2e")]
    if let FileKey::File(path) = &row.key {
        content = content.relative().child(super::e2e::measure_inside(format!(
            "cp-{}-ghost:{}",
            if staged { "st" } else { "us" },
            path.display()
        )));
    }
    content
}

impl CommitPanelView {
    pub(super) fn settle_file_motion(&mut self) {
        self.file_motion = Default::default();
    }
    pub(super) fn sync_file_motion(&mut self, window: &mut Window) {
        let now = motion::now();
        for staged in [false, true] {
            let files = if staged {
                &self.state.staged
            } else {
                &self.state.unstaged
            };
            let selected = match self.state.selected_file.as_ref() {
                Some(CommitPanelFileRef::Staged { index }) if staged => {
                    files.get(*index).map(|f| &f.path)
                }
                Some(CommitPanelFileRef::Unstaged { index }) if !staged => {
                    files.get(*index).map(|f| &f.path)
                }
                _ => None,
            };
            let wip = self
                .active_wip
                .as_ref()
                .filter(|(side, _)| *side == staged)
                .map(|(_, path)| path);
            let scroll = if staged {
                &self.staged_scroll_handle
            } else {
                &self.unstaged_scroll_handle
            };
            let height = scroll.0.borrow().base_handle.bounds().size.height;
            // ItemSize.item is the viewport, not one row. Recover the actual
            // pixel-snapped pitch from the last completed list layout before
            // replacing its logical rows with the next status/fold generation.
            let previous_count = self.file_motion[staged as usize].rows.len();
            self.file_motion[staged as usize].pitch = scroll
                .0
                .borrow()
                .last_item_size
                .filter(|_| previous_count > 0)
                .map_or(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)), |size| {
                    size.contents.height / previous_count as f32
                });
            let viewport_rows = (f32::from(height)
                / f32::from(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.))))
            .ceil() as usize;
            let moving = self.file_motion[staged as usize].sync(
                &self.state,
                staged,
                [selected, wip],
                viewport_rows,
                now,
            );
            motion::request_frame(window, moving);
        }
    }
    pub(super) fn render_unstaged_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let panel = &self.state;
        let tree_view = panel.tree_view;
        let unstaged_scroll_handle = self.unstaged_scroll_handle.clone();
        let unstaged_row_count = if tree_view {
            panel.unstaged_tree.len()
        } else {
            panel.unstaged_normal_files.len()
        } + panel.generated_extra_rows(false)
            + panel.agent_extra_rows(false);
        div()
            .id("cp-unstaged-scroll")
            .relative()
            .when_some(self.unstaged_navigation.rows.as_ref(), |el, rows| {
                rows.list(el)
            })
            .when(!tree_view, |el| {
                el.role(gpui::Role::ListBox)
                    .aria_label(Msg::A11yUnstagedTree.t())
            })
            .child(super::e2e::measure_inside("cp-us-viewport"))
            .flex_1()
            .min_h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)) + px(4.))
            .mx_1()
            .mb_px()
            .border_1()
            .border_color(rgb(theme().surface))
            .rounded_sm()
            .flex()
            .flex_col()
            .child({
                let handle = unstaged_scroll_handle.clone();
                let list = with_vertical_scrollbar(
                    "cp-unstaged-list-scroll",
                    &handle,
                    uniform_list(
                        "cp-unstaged-list",
                        unstaged_row_count,
                        cx.processor(move |view, range: std::ops::Range<usize>, _window, cx| {
                            let height =
                                list_pitch(&view.unstaged_scroll_handle, unstaged_row_count);
                            let requested = range;
                            let range =
                                view.file_motion[0].presentation_range(requested.clone(), height);
                            let rows = view.unstaged_navigation.drawn(&range, _window, cx);
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.unstaged_tree_layout.as_slice());
                            view.file_motion[0].mount_rows(requested, range, height, |i| {
                                match &layout {
                                    Some(layout) => render_unstaged_tree_row(
                                        view,
                                        &rows,
                                        i,
                                        cp_tree_slot(layout, "cp-unstaged-tree", i),
                                        cx,
                                    ),
                                    None => render_unstaged_flat_row(view, &rows, i, cx),
                                }
                            })
                        }),
                    )
                    .track_scroll(&unstaged_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    false,
                )
                .when(!self.file_motion[0].removed.is_empty(), |list| {
                    list.child(self.file_motion[0].ghosts(false, &self.unstaged_scroll_handle))
                });
                if tree_view {
                    kagi_ui_core::tree_a11y::tree(
                        "cp-unstaged-tree",
                        list,
                        Msg::A11yUnstagedTree.t(),
                    )
                } else {
                    list
                }
            })
    }
    pub(super) fn render_staged_list(&self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let panel = &self.state;
        let tree_view = panel.tree_view;
        let staged_scroll_handle = self.staged_scroll_handle.clone();
        let staged_row_count = if tree_view {
            panel.staged_tree.len()
        } else {
            panel.staged_normal_files.len()
        } + panel.generated_extra_rows(true)
            + panel.agent_extra_rows(true);
        div()
            .id("cp-staged-scroll")
            .relative()
            .when_some(self.staged_navigation.rows.as_ref(), |el, rows| {
                rows.list(el)
            })
            .when(!tree_view, |el| {
                el.role(gpui::Role::ListBox)
                    .aria_label(Msg::A11yStagedTree.t())
            })
            .child(super::e2e::measure_inside("cp-st-viewport"))
            .flex_1()
            .min_h(theme::scaled_px(FILE_ROW_HEIGHT).max(px(20.)) + px(4.))
            .mx_1()
            .mb_px()
            .border_1()
            .border_color(rgb(theme().surface))
            .rounded_sm()
            .flex()
            .flex_col()
            .child({
                let handle = staged_scroll_handle.clone();
                let list = with_vertical_scrollbar(
                    "cp-staged-list-scroll",
                    &handle,
                    uniform_list(
                        "cp-staged-list",
                        staged_row_count,
                        cx.processor(move |view, range: std::ops::Range<usize>, _window, cx| {
                            let height = list_pitch(&view.staged_scroll_handle, staged_row_count);
                            let requested = range;
                            let range =
                                view.file_motion[1].presentation_range(requested.clone(), height);
                            let rows = view.staged_navigation.drawn(&range, _window, cx);
                            let layout = view
                                .state
                                .tree_view
                                .then_some(view.staged_tree_layout.as_slice());
                            view.file_motion[1].mount_rows(requested, range, height, |i| {
                                match &layout {
                                    Some(layout) => render_staged_tree_row(
                                        view,
                                        &rows,
                                        i,
                                        cp_tree_slot(layout, "cp-staged-tree", i),
                                        cx,
                                    ),
                                    None => render_staged_flat_row(view, &rows, i, cx),
                                }
                            })
                        }),
                    )
                    .track_scroll(&staged_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    false,
                )
                .when(!self.file_motion[1].removed.is_empty(), |list| {
                    list.child(self.file_motion[1].ghosts(true, &self.staged_scroll_handle))
                });
                if tree_view {
                    kagi_ui_core::tree_a11y::tree("cp-staged-tree", list, Msg::A11yStagedTree.t())
                } else {
                    list
                }
            })
    }
}
