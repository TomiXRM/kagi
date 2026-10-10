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
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, rgb, AnyElement, App, Context, Entity, SharedString, Window};
use kagi_domain::review_thread::{anchor_rows, DiffSide, ReviewThread};

use super::{KagiApp, PrView};
use crate::ui::diff_view::{DiffRow, MainDiffView};
use crate::ui::render_helpers::row_overlay::{self, GutterSide, RowOverlay};
use crate::ui::render_helpers::DiffHeader;
use crate::ui::theme::{self, theme};

type Placement = Rc<BTreeMap<usize, Vec<usize>>>;

/// A PR tab's review threads and which of their rows are open.
#[derive(Default)]
pub struct PrThreads {
    threads: Rc<Vec<ReviewThread>>,
    /// Open anchored rows of the diff of `open_path`; another file's diff
    /// starts closed.
    open: BTreeSet<usize>,
    open_path: String,
    /// Placement belongs to the immutable row allocation, not its count.
    placed: RefCell<Option<(String, std::sync::Weak<Vec<DiffRow>>, Placement)>>,
}

impl PrThreads {
    pub(crate) fn set(
        &mut self,
        threads: Vec<ReviewThread>,
        list: &gpui::ListState,
        diff: Option<&MainDiffView>,
    ) {
        self.threads = Rc::new(threads);
        self.placed.replace(None);
        if let Some(diff) = diff.filter(|diff| self.open_path == diff.title.as_ref()) {
            if !self.open.is_empty() {
                let layout = row_overlay::layout(&diff.rows, theme::diff_split());
                let mut item = 0;
                for base in &layout {
                    item += 1;
                    if base.iter().flatten().any(|row| self.open.contains(row)) {
                        if item < list.item_count() {
                            list.remeasure_items(item..item + 1);
                        }
                        item += 1;
                    }
                }
            }
        }
    }

    /// Where the threads sit in `rows`, the diff of `path`.
    pub fn placement(&self, path: &str, rows: &Arc<Vec<DiffRow>>) -> Placement {
        if let Some((cached_path, source, placed)) = self.placed.borrow().as_ref() {
            if cached_path == path && std::ptr::eq(source.as_ptr(), Arc::as_ptr(rows)) {
                return placed.clone();
            }
        }
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
        self.placed.replace(Some((
            path.to_string(),
            Arc::downgrade(rows),
            placed.clone(),
        )));
        placed
    }

    fn open_rows(&self, path: &str) -> BTreeSet<usize> {
        if self.open_path == path {
            self.open.clone()
        } else {
            BTreeSet::new()
        }
    }
}

/// The overlay the PR diff draws with: one snapshot per frame.
struct ThreadOverlay {
    threads: Rc<Vec<ReviewThread>>,
    placed: Placement,
    open: BTreeSet<usize>,
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
        #[cfg(feature = "gui-e2e")]
        let badge = badge
            .relative()
            .child(crate::ui::e2e::measure_inside(format!(
                "pr-thread-badge-{row}{suffix}"
            )));
        Some(badge.into_any_element())
    }

    fn expanded(&self, row: usize) -> bool {
        self.open.contains(&row)
    }

    fn expansion(&self, rows: &[usize], window: &mut Window, cx: &mut App) -> AnyElement {
        let style = crate::ui::timeline_row::markdown_style(14., cx);
        let mut column = div().w_full().flex().flex_col().gap_1().py_1();
        for &row in rows {
            for (k, (_, thread)) in self.threads_at(row).enumerate() {
                column = column.child(thread_card(row, k, thread, &style, window, cx));
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
    window: &mut Window,
    cx: &mut App,
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
                        crate::ui::timeline_row::BodyMarkdownFormat::Original,
                        style.clone(),
                        window,
                        cx,
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
            open: tab.threads.open_rows(&path),
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
        let before = tab.threads.open_rows(&path);
        let mut after = before.clone();
        if !after.remove(&row) {
            after.insert(row);
        }
        let layout = row_overlay::layout(&diff.rows, split);
        row_overlay::splice_expansion(
            &tab.diff_scroll,
            &layout,
            row,
            &|r| before.contains(&r),
            &|r| after.contains(&r),
        );
        klog!(
            "pr-threads: #{} {}:{} open={}",
            tab.pr.number,
            Path::new(&path).display(),
            row,
            after.contains(&row)
        );
        tab.threads.open = after;
        tab.threads.open_path = path;
        cx.notify();
    }
}
