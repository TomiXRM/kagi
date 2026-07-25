//! Right-pane History tab + center-pane Diff/Snapshot tabs (T-WS-EDITOR-008).
//!
//! Kept in a sibling module rather than growing `lib.rs` further (CLAUDE.md:
//! prefer a focused sibling module over an oversized file already past the
//! 800-LOC target).

use std::ops::Range;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, relative, rgb, uniform_list, AnyElement, Context, SharedString, Window};
use gpui_component::input::Input;

use kagi_domain::file_history::{CommitSummary, FileHistoryEntry, FileHistoryEntryKind};
use kagi_ui_core::avatar::AvatarImages;
use kagi_ui_core::commit_header::render_commit_header;
use kagi_ui_core::i18n::Msg;
use kagi_ui_core::theme::theme;

/// Fixed top:bottom ratio for the History pane's commit-detail header vs.
/// the commit list (user request — "3:7"; not a drag-resizable
/// `InspectorSplit`-style divider, at least not in this v1).
const HISTORY_HEADER_RATIO: f32 = 0.3;

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
    on_click: impl Fn(&mut EditorWorkspaceView, &mut Window, &mut Context<EditorWorkspaceView>)
        + 'static,
    cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        on_click(this, window, cx);
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
            |this, window, cx| this.set_right_tab(RightPaneTab::Diff, window, cx),
            cx,
        ))
        .child(render_tab_chip(
            ("ews-right-tab", 1),
            Msg::EditorRightTabHistory.t().into(),
            view.right_tab == RightPaneTab::History,
            |this, window, cx| this.set_right_tab(RightPaneTab::History, window, cx),
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
            |this, _window, cx| this.set_middle_tab(MiddlePaneTab::Diff, cx),
            cx,
        ))
        .child(render_tab_chip(
            ("ews-middle-tab", 1),
            Msg::EditorMiddleTabSnapshot.t().into(),
            view.middle_tab == MiddlePaneTab::Snapshot,
            |this, _window, cx| this.set_middle_tab(MiddlePaneTab::Snapshot, cx),
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

    let selected_commit = view
        .selected_history_commit
        .as_deref()
        .and_then(|hash| history.entry_by_hash(hash))
        .and_then(|e| e.commit.as_ref());
    let header = render_history_header(selected_commit, &view.avatars);

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
    // Fixed 3:7 ratio (user request), not a drag divider — both boxes use
    // `flex_basis(relative(_)) + flex_shrink(1.)` with no explicit
    // `flex_grow`, the same recipe `inspector.rs`'s `InspectorSplit` uses to
    // avoid the wrapped-text-feeds-back-into-layout jitter that plain
    // `overflow_y_scroll` directly on a flex-sized box causes.
    let list_box = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(1.0 - HISTORY_HEADER_RATIO))
        .flex_shrink(1.)
        .child(list);
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(header)
        .child(list_box)
        .into_any_element()
}

/// Top of the History pane: the selected commit's subject/meta/body via the
/// shared `kagi_ui_core::commit_header::render_commit_header` — also used by
/// File History's detail pane (see that function's doc comment for why Graph
/// mode's Inspector isn't folded in too). This wrapper supplies only the
/// Editor-specific layout: a fixed 30% box with its own inner scroll (same
/// double-div anti-jitter technique `inspector.rs`'s `InspectorSplit` uses —
/// `overflow_hidden` on the sized outer box, `overflow_y_scroll` on an
/// unconstrained inner child, so wrapped text can't feed back into the flex
/// solve).
fn render_history_header(commit: Option<&CommitSummary>, avatars: &AvatarImages) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(HISTORY_HEADER_RATIO))
        .flex_shrink(1.)
        .overflow_hidden()
        .px_2()
        .py_2()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .id("ews-history-header-scroll")
                .size_full()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(render_commit_header(commit, avatars)),
        )
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
    let click = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        this.select_history_commit(hash.clone(), window, cx);
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

/// Center pane's Snapshot tab body: the selected commit's full file text via
/// `view.snapshot_editor` (`sync_snapshot_editor`, `lib.rs`) — the same
/// `code_editor` `InputState` widget the WIP buffer uses, so Snapshot gets
/// the same monospace font, line numbers, and text selection/copy for free
/// (bug report: an earlier hand-rolled `uniform_list` of styled divs had
/// none of the three). Deliberately a SEPARATE `InputState` from `editor`
/// (the live, saveable WIP buffer) — typing here is inert, never read back
/// or persisted (see `snapshot_editor`'s doc comment for why: gpui-component
/// has no read-only-but-selectable input mode in this version).
pub(crate) fn render_snapshot_pane(
    view: &EditorWorkspaceView,
    _cx: &mut Context<EditorWorkspaceView>,
) -> AnyElement {
    if view.snapshot_loading {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    }
    // Bug report: this used to also show "Loading…" here, forever — but a
    // finished load that found nothing (e.g. this commit predates a rename
    // and the path didn't exist yet) is a distinct, resolved state, not a
    // stuck spinner.
    let Some(snapshot) = view.snapshot.as_ref() else {
        return placeholder_text(Msg::EditorSnapshotUnavailable.t()).into_any_element();
    };
    if snapshot.is_binary {
        return placeholder_text(Msg::EditorWorkspaceBinary.t()).into_any_element();
    }
    if snapshot.content.is_none() {
        return placeholder_text(Msg::EditorWorkspaceDeleted.t()).into_any_element();
    }
    let Some(editor) = view.snapshot_editor.clone() else {
        return placeholder_text(Msg::EditorWorkspaceLoading.t()).into_any_element();
    };
    div()
        .id("ews-snapshot")
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .child(
            Input::new(&editor)
                .appearance(false)
                .bordered(false)
                .h_full(),
        )
        .into_any_element()
}
