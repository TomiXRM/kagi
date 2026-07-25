//! Right-pane History tab + center-pane Diff/Snapshot tabs (T-WS-EDITOR-008).
//!
//! Kept in a sibling module rather than growing `lib.rs` further (CLAUDE.md:
//! prefer a focused sibling module over an oversized file already past the
//! 800-LOC target).

use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, rgb, uniform_list, AnyElement, App, Context, SharedString, Window};

use kagi_domain::file_history::{FileHistoryEntry, FileHistoryEntryKind};
use kagi_ui_core::i18n::Msg;
use kagi_ui_core::theme::{theme, MONO_FONT};

use crate::{
    placeholder_text, with_vertical_scrollbar, EditorWorkspaceView, MiddlePaneTab, RightPaneTab,
};

/// One tab chip, shared visual style between the right-pane and center-pane
/// switchers — mirrors `render_source_chip`'s look (see `lib.rs`) with a
/// generic click callback since each switcher dispatches to a different
/// setter (`set_right_tab` / `set_middle_tab`).
fn render_tab_chip(
    id: (&'static str, usize),
    label: SharedString,
    active: bool,
    on_click: impl Fn(&mut EditorWorkspaceView, &mut Context<EditorWorkspaceView>) + 'static,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        on_click(this, cx);
    });
    div()
        .id(id)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .py_px()
        .rounded_sm()
        .cursor_pointer()
        .text_xs()
        .text_color(rgb(if active {
            theme().text_main
        } else {
            theme().text_muted
        }))
        .bg(rgb(if active {
            theme().selected
        } else {
            theme().panel
        }))
        .when(!active, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        .on_click(click)
        .child(label)
        .into_any_element()
}

/// Right pane's Diff/History tab strip.
pub(crate) fn render_right_pane_tabs(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    div()
        .id("ews-right-tabs")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .gap_1()
        .px_2()
        .py_1()
        .bg(rgb(theme().panel))
        .child(render_tab_chip(
            ("ews-right-tab", 0),
            Msg::EditorRightTabDiff.t().into(),
            view.right_tab == RightPaneTab::Diff,
            |this, cx| this.set_right_tab(RightPaneTab::Diff, cx),
            cx,
        ))
        .child(render_tab_chip(
            ("ews-right-tab", 1),
            Msg::EditorRightTabHistory.t().into(),
            view.right_tab == RightPaneTab::History,
            |this, cx| this.set_right_tab(RightPaneTab::History, cx),
            cx,
        ))
        .into_any_element()
}

/// Center pane's Diff/Snapshot tab strip (only rendered once a History row
/// is selected — see the early-return branch in `render_center_pane`).
pub(crate) fn render_middle_pane_tabs(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    div()
        .id("ews-middle-tabs")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .gap_1()
        .px_2()
        .py_1()
        .bg(rgb(theme().panel))
        .child(render_tab_chip(
            ("ews-middle-tab", 0),
            Msg::EditorMiddleTabDiff.t().into(),
            view.middle_tab == MiddlePaneTab::Diff,
            |this, cx| this.set_middle_tab(MiddlePaneTab::Diff, cx),
            cx,
        ))
        .child(render_tab_chip(
            ("ews-middle-tab", 1),
            Msg::EditorMiddleTabSnapshot.t().into(),
            view.middle_tab == MiddlePaneTab::Snapshot,
            |this, cx| this.set_middle_tab(MiddlePaneTab::Snapshot, cx),
            cx,
        ))
        .into_any_element()
}

/// Right pane's History tab body: a virtualized, clickable commit list for
/// the open file (`view.history`, ADR-0089's data layer via `seed_history`).
pub(crate) fn render_history_pane(
    view: &EditorWorkspaceView,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    if view.history_loading {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    }
    let Some(history) = view.history.as_ref() else {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    };
    if history.entries.is_empty() {
        return placeholder_text(Msg::EditorHistoryEmpty.t()).into_any_element();
    }

    let entries = Arc::new(history.entries.clone());
    let row_count = entries.len();
    let scroll_handle = view.history_scroll.clone();
    let scrollbar_handle = scroll_handle.clone();
    let selected = view.selected_history_commit.clone();
    let list = with_vertical_scrollbar(
        "ews-history-scroll",
        &scrollbar_handle,
        uniform_list(
            "ews-history-list",
            row_count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                range
                    .filter_map(|i| render_history_row(this, i, entries.get(i)?, &selected, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h(px(0.)),
        false,
    );
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(list)
        .into_any_element()
}

/// One row in the History list: subject, then short hash / author / date.
/// `include_wip: false` on the backing request (see `start_editor_history_load`
/// in the bin) means every entry here is a real, selectable commit — the WIP
/// row is never duplicated into this list (the Diff tab already covers it).
fn render_history_row(
    _view: &EditorWorkspaceView,
    ix: usize,
    entry: &FileHistoryEntry,
    selected: &Option<String>,
    cx: &mut Context<EditorWorkspaceView>,
) -> Option<AnyElement> {
    debug_assert_ne!(entry.kind, FileHistoryEntryKind::Wip);
    let commit = entry.commit.as_ref()?;
    let is_selected = selected.as_deref() == Some(commit.full_hash.as_str());
    let hash = commit.full_hash.clone();
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.select_history_commit(hash.clone(), cx);
    });
    let row_bg = if is_selected {
        theme().selected
    } else if ix % 2 == 1 {
        theme().bg_row_alt
    } else {
        theme().panel
    };
    // `author_date` is `git log`'s strict-ISO8601 (`%aI`) — the leading 10
    // bytes are always `YYYY-MM-DD`; no date parser needed for this compact
    // sidebar row (the full-width File History panel has the relative-time
    // treatment already; this row just isn't wide enough for it).
    let date = commit.author_date.get(0..10).unwrap_or("").to_string();

    Some(
        div()
            .id(("ews-history-row", ix))
            .flex()
            .flex_col()
            .w_full()
            .px_2()
            .py_1()
            .gap_px()
            .bg(rgb(row_bg))
            .cursor_pointer()
            .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
            .on_click(click)
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .truncate()
                    .child(SharedString::from(commit.subject.clone())),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(commit.short_hash.clone()))
                    .child(SharedString::from(commit.author_name.clone()))
                    .child(SharedString::from(date)),
            )
            .into_any_element(),
    )
}

/// Center pane's Snapshot tab body: the selected commit's full read-only
/// file text, virtualized line-by-line (same `uniform_list` technique the
/// left file tree uses — a single un-virtualized div would choke on a large
/// file). Deliberately never touches `editor`/`content` (the live, saveable
/// WIP buffer) — this is a separate, read-only display.
pub(crate) fn render_snapshot_pane(
    view: &EditorWorkspaceView,
    _cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    if view.snapshot_loading {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    }
    let Some(snapshot) = view.snapshot.as_ref() else {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    };
    if snapshot.is_binary {
        return placeholder_text(Msg::EditorWorkspaceBinary.t()).into_any_element();
    }
    let Some(content) = snapshot.content.as_ref() else {
        return placeholder_text(Msg::EditorWorkspaceDeleted.t()).into_any_element();
    };

    let lines = Arc::new(content.lines().map(SharedString::from).collect::<Vec<_>>());
    let row_count = lines.len();
    let scroll_handle = view.snapshot_scroll.clone();
    let scrollbar_handle = scroll_handle.clone();
    // No entity access needed per row (plain text, no click handler) — a
    // raw closure is enough, unlike the History list's `cx.processor` (whose
    // rows need `cx.listener` for the select-commit click).
    let list = with_vertical_scrollbar(
        "ews-snapshot-scroll",
        &scrollbar_handle,
        uniform_list(
            "ews-snapshot-list",
            row_count,
            move |range: Range<usize>, _window: &mut Window, _cx: &mut App| {
                range
                    .filter_map(|i| lines.get(i).cloned())
                    .map(|line| {
                        div()
                            .w_full()
                            .px_2()
                            .font_family(MONO_FONT)
                            .text_sm()
                            .text_color(rgb(theme().text_main))
                            .child(line)
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h(px(0.)),
        false,
    );
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(list)
        .into_any_element()
}
