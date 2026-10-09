//! Review threads on the PR diff (#351, ADR-0209).
//!
//! A row a thread anchors to (`kagi_domain::review_thread::anchor_rows`, on
//! the whole-PR head diff) gets a count badge in the gutter — on the thread's
//! side in the split view. Clicking it folds the row's threads open directly
//! under the row; clicking again closes them. Outdated threads are dimmed.
//! Read-only: there is no resolve button (resolving is a write).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, App, Context, Entity, ListState, SharedString};
use kagi_domain::review_thread::{anchor_rows, DiffSide, ReviewThread};

use super::{KagiApp, PrView};
use crate::ui::diff_view::{DiffRow, MainDiffView};
use crate::ui::render_helpers::row_overlay::{self, GutterSide, RowOverlay};
use crate::ui::render_helpers::DiffHeader;
use crate::ui::theme::{self, theme};

type Placement = Rc<BTreeMap<usize, Vec<usize>>>;

struct PlacedThreads {
    path: String,
    source: Weak<Vec<DiffRow>>,
    placement: Placement,
}

/// A PR tab's review threads and which of their rows are open.
#[derive(Default)]
pub struct PrThreads {
    threads: Rc<Vec<ReviewThread>>,
    /// Open anchored rows of the diff of `open_path`; another file's diff
    /// starts closed.
    open: BTreeSet<usize>,
    open_path: String,
    expanded_revision: u64,
    /// Placement and overlay indices share the immutable source's lifetime.
    placed: RefCell<Option<PlacedThreads>>,
    projection: RefCell<row_overlay::ProjectionCache>,
}

impl PrThreads {
    pub(crate) fn set(&mut self, threads: Vec<ReviewThread>) {
        self.threads = Rc::new(threads);
        self.placed.replace(None);
        self.projection.borrow_mut().clear();
    }

    /// Where the threads sit in `rows`, the diff of `path`.
    pub fn placement(&self, path: &str, rows: &Arc<Vec<DiffRow>>) -> Placement {
        if let Some(cached) = self.placed.borrow().as_ref() {
            if cached.path == path
                && cached.source.strong_count() != 0
                && cached.source.as_ptr() == Arc::as_ptr(rows)
            {
                return cached.placement.clone();
            }
        }
        // Also release the old overlay when the new diff has no placed threads.
        self.projection.borrow_mut().clear();
        let lines: Vec<(Option<u32>, Option<u32>)> = rows
            .iter()
            .map(|row| match row {
                DiffRow::Line {
                    old_lineno,
                    new_lineno,
                    ..
                } => (*old_lineno, *new_lineno),
                _ => (None, None),
            })
            .collect();
        let placed = Rc::new(anchor_rows(&self.threads, path, &lines));
        self.placed.replace(Some(PlacedThreads {
            path: path.to_string(),
            source: Arc::downgrade(rows),
            placement: placed.clone(),
        }));
        placed
    }

    fn projection(
        &self,
        path: &str,
        rows: &Arc<Vec<DiffRow>>,
        split: bool,
    ) -> Rc<row_overlay::Projection> {
        let closed = BTreeSet::new();
        let open = if self.open_path == path {
            &self.open
        } else {
            &closed
        };
        self.projection
            .borrow_mut()
            .get(path, rows, split, self.expanded_revision, open)
    }

    fn toggle(
        &mut self,
        path: &str,
        rows: &Arc<Vec<DiffRow>>,
        split: bool,
        list: &ListState,
        row: usize,
    ) -> bool {
        let before = self.projection(path, rows, split);
        if self.open_path != path {
            self.open.clear();
            self.open_path = path.to_string();
        }
        if !self.open.remove(&row) {
            self.open.insert(row);
        }
        row_overlay::splice_expansion(
            list,
            &before.layout,
            row,
            &|row| before.expanded(row),
            &|row| self.open.contains(&row),
        );
        self.expanded_revision = self.expanded_revision.wrapping_add(1);
        self.open.contains(&row)
    }
}

/// The overlay the PR diff draws with: one snapshot per frame.
struct ThreadOverlay {
    threads: Rc<Vec<ReviewThread>>,
    placed: Placement,
    projection: Rc<row_overlay::Projection>,
    app: Entity<KagiApp>,
}

impl ThreadOverlay {
    fn threads_at(&self, row: usize) -> impl Iterator<Item = (usize, &ReviewThread)> {
        self.placed
            .get(&row)
            .into_iter()
            .flatten()
            .filter_map(|&ix| self.threads.get(ix).map(|thread| (ix, thread)))
    }
}

fn side_of(thread: &ReviewThread) -> GutterSide {
    match thread.diff_side {
        DiffSide::Left => GutterSide::Left,
        DiffSide::Right => GutterSide::Right,
    }
}

fn outdated(thread: &ReviewThread) -> bool {
    thread.anchor().is_some_and(|(_, outdated)| outdated)
}

impl RowOverlay for ThreadOverlay {
    fn marker(&self, row: usize, side: Option<GutterSide>, _cx: &mut App) -> Option<AnyElement> {
        let here: Vec<&ReviewThread> = self
            .threads_at(row)
            .map(|(_, thread)| thread)
            .filter(|thread| side.is_none_or(|side| side_of(thread) == side))
            .collect();
        if here.is_empty() {
            return None;
        }
        let all_outdated = here.iter().all(|thread| outdated(thread));
        let app = self.app.clone();
        let suffix = match side {
            None => String::new(),
            Some(GutterSide::Left) => "-l".into(),
            Some(GutterSide::Right) => "-r".into(),
        };
        let badge = div()
            .id(SharedString::from(format!("pr-thread-badge-{row}{suffix}")))
            .px_1()
            .rounded_sm()
            .text_xs()
            .cursor_pointer()
            .bg(rgb(theme().color_branch))
            .text_color(rgb(theme().bg_base))
            .when(all_outdated, |el| el.opacity(0.5))
            .on_click(move |_, _, cx| {
                app.update(cx, |app, cx| app.pr_mode_toggle_thread(row, cx));
            })
            .child(SharedString::from(here.len().to_string()));
        Some(crate::ui::e2e::measure_control(
            format!("pr-thread-badge-{row}{suffix}"),
            badge,
        ))
    }

    fn projection(&self) -> Rc<row_overlay::Projection> {
        self.projection.clone()
    }

    fn expansion(&self, rows: &[usize], cx: &mut App) -> AnyElement {
        let style = crate::ui::timeline_row::markdown_style(14., cx);
        let mut column = div().w_full().flex().flex_col().gap_1().py_1();
        for &row in rows {
            for (k, (_, thread)) in self.threads_at(row).enumerate() {
                column = column.child(thread_card(row, k, thread, &style));
            }
        }
        column.into_any_element()
    }
}

fn thread_card(
    row: usize,
    k: usize,
    thread: &ReviewThread,
    style: &gpui_component::text::TextViewStyle,
) -> AnyElement {
    let is_outdated = outdated(thread);
    let line = thread.anchor().map(|(line, _)| line).unwrap_or(0);
    let chip = |text: &'static str| {
        div()
            .px_1()
            .rounded_sm()
            .text_xs()
            .bg(rgb(theme().surface))
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(text))
    };
    let mut card = div()
        .mx_3()
        .p_2()
        .flex()
        .flex_col()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().surface))
        .bg(rgb(theme().bg_base))
        .font_family(crate::ui::UI_FONT)
        .when(is_outdated, |el| el.opacity(0.55))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(crate::ui::render_helpers::safe_text(&format!(
                    "{}:{}",
                    thread.path, line
                )))
                .when(is_outdated, |el| {
                    el.child(chip(crate::ui::i18n::pr_threads::outdated()))
                })
                .when(thread.is_resolved, |el| {
                    el.child(chip(crate::ui::i18n::pr_threads::resolved()))
                }),
        );
    for (c, comment) in thread.comments.iter().enumerate() {
        let body_id = format!("pr-thread-body-{row}-{k}-{c}");
        card = card.child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .child(crate::ui::render_helpers::safe_text(&comment.author)),
                )
                .child(crate::ui::e2e::measure_control(
                    body_id.clone(),
                    crate::ui::timeline_row::body_markdown(
                        SharedString::from(body_id),
                        &comment.body,
                        style.clone(),
                    ),
                )),
        );
    }
    let name = if is_outdated {
        format!("pr-thread-outdated-{row}-{k}")
    } else {
        format!("pr-thread-{row}-{k}")
    };
    crate::ui::e2e::measure_control(name, card.min_w(px(0.)))
}

/// The PR diff's header: review-thread overlay for the whole-PR diff of the
/// active tab (not one commit's, not the conflicts list).
pub(super) fn diff_header(
    app: &KagiApp,
    ix: usize,
    dv: &MainDiffView,
    cx: &mut Context<KagiApp>,
) -> DiffHeader {
    let Some(tab) = app.pr_mode().and_then(|mode| mode.tabs.get(ix)) else {
        return DiffHeader::default();
    };
    if tab.selected_commit.is_some() {
        return DiffHeader::default();
    }
    let path = dv.title.to_string();
    let placed = tab.threads.placement(&path, &dv.rows);
    if placed.is_empty() {
        return DiffHeader::default();
    }
    DiffHeader {
        overlay: Some(Rc::new(ThreadOverlay {
            threads: tab.threads.threads.clone(),
            placed,
            projection: tab.threads.projection(&path, &dv.rows, theme::diff_split()),
            app: cx.entity(),
        })),
        ..DiffHeader::default()
    }
}

impl KagiApp {
    /// Open or close the review threads under `row` of the active PR tab's
    /// diff, splicing the list so the scroll position stays.
    pub fn pr_mode_toggle_thread(&mut self, row: usize, cx: &mut Context<Self>) {
        let split = theme::diff_split();
        let Some(mode) = self.pr_mode_mut() else {
            return;
        };
        if mode.view != PrView::Diff {
            return;
        }
        let Some(tab) = mode.active.and_then(|active| mode.tabs.get_mut(active)) else {
            return;
        };
        let Some(diff) = tab.diff.as_ref() else {
            return;
        };
        let path = diff.title.to_string();
        let open = tab
            .threads
            .toggle(&path, &diff.rows, split, &tab.diff_scroll, row);
        klog!(
            "pr-threads: #{} {}:{} open={}",
            tab.pr.number,
            Path::new(&path).display(),
            row,
            open
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_git::DiffLineKind;

    fn line(kind: DiffLineKind, old: Option<u32>, new: Option<u32>) -> DiffRow {
        DiffRow::Line {
            kind,
            text: "line".into(),
            old_lineno: old,
            new_lineno: new,
            highlights: Vec::new(),
        }
    }

    fn anchored_threads() -> PrThreads {
        let mut threads = PrThreads::default();
        threads.set(vec![
            ReviewThread {
                path: "file.rs".into(),
                line: Some(2),
                diff_side: DiffSide::Left,
                ..ReviewThread::default()
            },
            ReviewThread {
                path: "file.rs".into(),
                line: Some(2),
                diff_side: DiffSide::Right,
                ..ReviewThread::default()
            },
        ]);
        threads
    }

    fn rows() -> Arc<Vec<DiffRow>> {
        Arc::new(vec![
            line(DiffLineKind::Context, Some(1), Some(1)),
            line(DiffLineKind::Removed, Some(2), None),
            line(DiffLineKind::Added, None, Some(2)),
            line(DiffLineKind::Context, Some(3), Some(3)),
        ])
    }

    #[test]
    fn same_path_same_count_replacement_and_in_place_mutation_reanchor_threads() {
        let threads = anchored_threads();
        let first = rows();
        assert_eq!(
            *threads.placement("file.rs", &first),
            BTreeMap::from([(1, vec![0]), (2, vec![1])])
        );
        let mut replacement = Arc::new(vec![
            line(DiffLineKind::Context, Some(2), Some(2)),
            line(DiffLineKind::Context, Some(1), Some(1)),
            line(DiffLineKind::Removed, Some(3), None),
            line(DiffLineKind::Added, None, Some(3)),
        ]);
        assert_eq!(
            *threads.placement("file.rs", &replacement),
            BTreeMap::from([(0, vec![0, 1])])
        );
        Arc::make_mut(&mut replacement).swap(0, 1);
        assert_eq!(
            *threads.placement("file.rs", &replacement),
            BTreeMap::from([(1, vec![0, 1])])
        );
        assert_eq!(
            *threads.placement("file.rs", &first),
            BTreeMap::from([(1, vec![0]), (2, vec![1])])
        );
        assert!(threads.placement("other.rs", &first).is_empty());
    }

    #[test]
    fn paired_expansions_and_path_changes_remain_owned_by_the_pr_tab() {
        let mut first = anchored_threads();
        let second = anchored_threads();
        let rows = rows();
        let closed = first.projection("file.rs", &rows, true);
        let list = ListState::new(closed.items.len(), gpui::ListAlignment::Top, px(100.));
        list.scroll_to(gpui::ListOffset {
            item_ix: 2,
            offset_in_item: px(9.),
        });
        assert!(first.toggle("file.rs", &rows, true, &list, 1));
        assert!(first.toggle("file.rs", &rows, true, &list, 2));
        let open = first.projection("file.rs", &rows, true);
        assert_eq!(open.expansion_rows(1), [Some(1), Some(2)]);
        assert_eq!(list.item_count(), open.items.len());
        assert_eq!(
            open.items[list.logical_scroll_top().item_ix],
            row_overlay::Item::Row(2)
        );
        assert_eq!(list.logical_scroll_top().offset_in_item, px(9.));
        assert_eq!(
            second.projection("file.rs", &rows, true).items,
            [
                row_overlay::Item::Row(0),
                row_overlay::Item::Row(1),
                row_overlay::Item::Row(2)
            ]
        );
        assert_eq!(
            first.projection("other.rs", &rows, true).items,
            [
                row_overlay::Item::Row(0),
                row_overlay::Item::Row(1),
                row_overlay::Item::Row(2)
            ]
        );
        assert!(!first.toggle("file.rs", &rows, true, &list, 1));
        assert!(!first.toggle("file.rs", &rows, true, &list, 2));
        let closed = first.projection("file.rs", &rows, true);
        assert_eq!(list.item_count(), closed.items.len());
        assert_eq!(
            closed.items[list.logical_scroll_top().item_ix],
            row_overlay::Item::Row(2)
        );
        assert_eq!(list.logical_scroll_top().offset_in_item, px(9.));
    }

    #[test]
    fn switching_to_an_unanchored_source_releases_the_last_overlay_snapshot() {
        let threads = anchored_threads();
        let rows = rows();
        threads.placement("file.rs", &rows);
        let projection = threads.projection("file.rs", &rows, true);
        let snapshot = Rc::downgrade(&projection);
        let source = Arc::downgrade(&rows);
        drop(projection);
        drop(rows);
        assert!(
            source.upgrade().is_none(),
            "placement must not retain source rows"
        );
        let replacement = Arc::new(vec![line(DiffLineKind::Context, Some(5), Some(5))]);
        assert!(threads.placement("file.rs", &replacement).is_empty());
        assert!(
            snapshot.upgrade().is_none(),
            "an unanchored replacement must release the previous projection"
        );
    }
}
