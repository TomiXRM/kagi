//! File History view rendering (ADR-0089), moved from the bin's
//! `src/ui/file_history_render.rs` (ADR-0121 C3). The diff body is the
//! host-provided `diff_pane` `AnyView` (the bin's shared `MainDiffView`
//! pipeline); every parent back-call is a [`FileHistoryEvent`] emission.
//! Behaviour-preserving move — no DOM/style/handler/[kagi]/i18n change.

use gpui::prelude::*;
use gpui::{div, px, rgb, AnyView, ClipboardItem, Context, MouseButton, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;

use kagi_domain::commit::CommitId;
use kagi_ui_core::divider::{DividerDrag, DividerGhost, DividerKind};
use kagi_ui_core::theme::{self, theme};
use kagi_ui_core::time::{now_unix_secs, relative_time};

use crate::detail::{render_fh_detail_pane, render_fh_row_menu};
use crate::{
    entry_badge, fh_row_height, iso_to_epoch, FileHistoryEvent, FileHistoryState, FileHistoryView,
};
use kagi_domain::file_history::{FileChangeType, FileHistoryEntry, FileHistoryEntryKind};

impl Render for FileHistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_file_history_view(self, cx)
    }
}

/// A small text "chip" button used in the File History header.
pub(crate) fn fh_header_button(
    id: &'static str,
    label: impl Into<SharedString>,
    on_click: impl Fn(
            &mut FileHistoryView,
            &gpui::ClickEvent,
            &mut gpui::Window,
            &mut Context<FileHistoryView>,
        ) + 'static,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    Button::new(id)
        .label(label.into())
        .ghost()
        .small()
        .on_click(cx.listener(on_click))
}

/// Render the entire File History view (center + right detail pane), ADR-0089.
///
/// The diff body below the commit list is the host-provided `diff_pane`
/// (`AnyView`).  Returns the body fragment that the bin's `render_body` drops
/// in place of the normal center+right area.
pub fn render_file_history_view(
    view: &FileHistoryView,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // ADR-0117: the entity renders itself. Read only `self`'s data here; every
    // parent interaction is an event emission (ADR-0121 C3). The owned bindings
    // below come straight from `view.data`.
    let state = &view.data;
    let file_history_menu = view.menu;
    let fh_branch = view.data.branch.clone();
    let panel_width = view.panel_width;
    let geom = view.geom.clone();
    let diff_pane = view.diff_pane.clone();

    // Extract the scalar/owned view data from the `state` borrow.
    let (rel_path, follow, split, count, is_loading, error, is_empty, is_untracked) = (
        state.rel_path.clone(),
        state.follow_renames,
        state.split,
        state.commit_count(),
        state.is_loading(),
        state.error.clone(),
        state.is_empty(),
        state.is_untracked(),
    );
    let rel_path_str = SharedString::from(rel_path.to_string_lossy().into_owned());

    // ── Header ──────────────────────────────────────────────────────
    let back = fh_header_button(
        "fh-back",
        "\u{2190} Back",
        |_this, _e, _w, cx| {
            cx.emit(FileHistoryEvent::CloseRequested);
        },
        cx,
    );

    let path_for_copy = rel_path.clone();
    let copy_path = fh_header_button(
        "fh-copy-path",
        "Copy Path",
        move |_this, _e, _w, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(
                path_for_copy.to_string_lossy().into_owned(),
            ));
        },
        cx,
    );

    let refresh = fh_header_button(
        "fh-refresh",
        "Refresh",
        |this, _e, _w, cx| {
            this.reload(true, cx);
        },
        cx,
    );

    let path_for_open = rel_path.clone();
    let open_file = fh_header_button(
        "fh-open-file",
        "Open File",
        move |_this, _e, _w, cx| {
            // v1: return to the normal body; the file's diff is reachable via
            // the commit panel / inspector.  Keep it simple per the spec.
            let _ = &path_for_open;
            cx.emit(FileHistoryEvent::CloseRequested);
        },
        cx,
    );

    let follow_label = if follow {
        "Follow Renames: On"
    } else {
        "Follow Renames: Off"
    };
    let follow_btn = fh_header_button(
        "fh-follow",
        follow_label,
        |this, _e, _w, cx| {
            this.data.follow_renames = !this.data.follow_renames;
            this.reload(false, cx);
        },
        cx,
    );

    let header = div()
        .id("fh-header")
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .w_full()
        .px_3()
        .py_1()
        .gap_2()
        .bg(rgb(theme().surface))
        .child(back)
        .child(
            div()
                .id("fh-title")
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(SharedString::from(format!(
                    "File History: {}",
                    rel_path_str
                )))
                .tooltip(move |window, cx| Tooltip::new(rel_path_str.clone()).build(window, cx)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_sub))
                .child(fh_branch.clone()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{} commits", count))),
        )
        .child(refresh)
        .child(copy_path)
        .child(open_file)
        .child(follow_btn);

    // ── Center column (list + diff) selection of the body content ──
    let center_body: gpui::AnyElement = if is_loading {
        render_fh_message("Loading file history...", false, cx).into_any_element()
    } else if let Some(err) = error {
        render_fh_error(err, cx).into_any_element()
    } else if is_empty {
        render_fh_message("No history found for this file.", false, cx).into_any_element()
    } else if is_untracked {
        // Untracked: show the message but still allow the WIP diff below.
        render_fh_list_and_diff(
            state,
            split,
            Some("This file is untracked. No commit history yet."),
            geom,
            diff_pane,
            cx,
        )
        .into_any_element()
    } else {
        render_fh_list_and_diff(state, split, None, geom, diff_pane, cx).into_any_element()
    };

    let center = div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_w(px(0.))
        .bg(rgb(theme().panel))
        .child(header)
        .child(center_body);

    // ── Right detail pane ──────────────────────────────────────────
    let detail_divider = div()
        .id("fh-detail-divider")
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::Panel,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let detail_pane = render_fh_detail_pane(state, panel_width, cx);

    // ── Optional row context menu overlay ──────────────────────────
    let menu_overlay = file_history_menu.map(|(ix, pos)| render_fh_row_menu(state, ix, pos, cx));

    div()
        .id("file-history-view")
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .child(center)
        .child(detail_divider)
        .child(detail_pane)
        .children(menu_overlay)
        .into_any_element()
}

/// A centered single-line message (Loading / Empty), optionally an error tint.
pub(crate) fn render_fh_message(
    msg: &'static str,
    is_error: bool,
    _cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let color = if is_error {
        theme().color_blocker
    } else {
        theme().text_muted
    };
    div()
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(color))
        .child(SharedString::from(msg))
}

/// Error state: message + detail + Retry button.
pub(crate) fn render_fh_error(
    detail: String,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let retry = div()
        .id("fh-retry")
        .px_3()
        .py_1()
        .rounded_sm()
        .bg(rgb(theme().bg_base))
        .text_sm()
        .text_color(rgb(theme().text_sub))
        .on_click(cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.reload(true, cx);
        }))
        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
        .child(SharedString::from("Retry"));

    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .child(SharedString::from("Failed to load file history.")),
        )
        .child(
            div()
                .max_w(px(520.))
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(detail)),
        )
        .child(retry)
}

/// The vertically-split commit list (top) + diff viewer (bottom).
pub(crate) fn render_fh_list_and_diff(
    state: &FileHistoryState,
    split: f32,
    banner: Option<&'static str>,
    geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    diff_pane: AnyView,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let list = render_fh_commit_list(state, cx);
    // Per-entry banner (Added / Deleted / Renamed) above the diff.
    let sel_banner = state.selected_entry().map(|e| match e.change.change_type {
        FileChangeType::Added => "This file was added in this commit.".to_string(),
        FileChangeType::Deleted => "This file was deleted in this commit.".to_string(),
        FileChangeType::Renamed => {
            let before = e
                .change
                .path_before
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let after = e.change.path_after.to_string_lossy().into_owned();
            format!("{} \u{2192} {}", before, after)
        }
        _ if e.change.is_binary => "Binary file changed. Preview is not available.".to_string(),
        _ => String::new(),
    });

    // Divider between list and diff (horizontal drag).
    let h_divider = div()
        .id("fh-rows-divider")
        .w_full()
        .h(theme::scaled_px(4.))
        .flex_shrink_0()
        .bg(rgb(theme().surface))
        .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
        .cursor_row_resize()
        .on_drag(
            DividerDrag {
                kind: DividerKind::FileHistoryRows,
            },
            |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
        );

    let list_frac = split.clamp(0.15, 0.85);
    let diff_frac = 1.0 - list_frac;

    let diff_section = div()
        .w_full()
        .flex()
        .flex_col()
        .flex_grow(1.)
        .flex_basis(gpui::relative(diff_frac))
        .min_h(px(0.))
        // Optional view-level banner (untracked note).
        .when_some(banner, |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().color_warning))
                    .child(SharedString::from(b)),
            )
        })
        // Per-entry banner (added/deleted/renamed/binary).
        .when_some(sel_banner.filter(|s| !s.is_empty()), |el, b| {
            el.child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .bg(rgb(theme().bg_row_alt))
                    .child(SharedString::from(b)),
            )
        })
        // ADR-0121 C3: the diff body is the host-provided pane entity (the
        // bin's shared `MainDiffView` pipeline, or its "no diff" placeholder).
        .child(diff_pane);

    // Paint-time canvas records the real (top, bottom) screen bounds of this
    // list+diff region so the divider drag maps the cursor exactly (a constant
    // top offset misses the variable-height header → the drag would jump).
    let measure = {
        let geom = geom.clone();
        gpui::canvas(
            move |_b: gpui::Bounds<gpui::Pixels>, _w: &mut Window, _cx: &mut gpui::App| {},
            move |b: gpui::Bounds<gpui::Pixels>, _p: (), _w: &mut Window, _cx: &mut gpui::App| {
                let top = f32::from(b.origin.y);
                geom.set((top, top + f32::from(b.size.height)));
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    };

    div()
        .relative()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(measure)
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .flex_grow(1.)
                .flex_basis(gpui::relative(list_frac))
                .min_h(px(0.))
                .child(list),
        )
        .child(h_divider)
        .child(diff_section)
}

/// The commit list (upper pane) of the File History view.
pub(crate) fn render_fh_commit_list(
    state: &FileHistoryState,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    let Some(history) = state.history.as_ref() else {
        return div().into_any_element();
    };
    let entries = &history.entries;
    let selected = state.selected;
    let now = now_unix_secs();

    let mut list = div()
        .id("fh-commit-list")
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .min_h(px(0.));

    for (ix, entry) in entries.iter().enumerate() {
        list = list.child(render_fh_row(ix, entry, ix == selected, now, cx));
    }

    list.into_any_element()
}

/// One row in the File History commit list.
pub(crate) fn render_fh_row(
    ix: usize,
    entry: &FileHistoryEntry,
    is_selected: bool,
    now: i64,
    cx: &mut Context<FileHistoryView>,
) -> impl IntoElement {
    let (badge, badge_color) = entry_badge(entry);
    let is_wip = entry.kind == FileHistoryEntryKind::Wip;

    let (subject, author, date, short_hash) = if is_wip {
        (
            SharedString::from("WIP \u{2014} Uncommitted changes"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    } else if let Some(c) = entry.commit.as_ref() {
        let date = iso_to_epoch(&c.author_date)
            .map(|e| relative_time(e, now))
            .unwrap_or_default();
        (
            SharedString::from(c.subject.clone()),
            SharedString::from(c.author_name.clone()),
            SharedString::from(date),
            SharedString::from(c.short_hash.clone()),
        )
    } else {
        (
            SharedString::from("(unknown)"),
            SharedString::from(""),
            SharedString::from(""),
            SharedString::from(""),
        )
    };

    let ins = entry.change.insertions;
    let del = entry.change.deletions;
    let stat = if entry.change.is_binary {
        SharedString::from("bin")
    } else {
        SharedString::from(format!(
            "+{} \u{2212}{}",
            ins.unwrap_or(0),
            del.unwrap_or(0)
        ))
    };

    let row_bg = if is_selected {
        theme().selected
    } else if ix % 2 == 1 {
        theme().bg_row_alt
    } else {
        theme().panel
    };

    let click = cx.listener(move |this, e: &gpui::ClickEvent, _w, cx| {
        this.menu = None;
        if e.click_count() >= 2 {
            // Double-click: jump to the commit in the graph (commits only).
            if let Some(id) = this
                .data
                .history
                .as_ref()
                .and_then(|h| h.entries.get(ix))
                .and_then(|e| e.commit.as_ref())
                .map(|c| CommitId(c.full_hash.clone()))
            {
                cx.emit(FileHistoryEvent::JumpToCommit(id));
                return;
            }
        }
        this.select(ix, cx);
    });
    let ctx = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
        this.menu = Some((ix, e.position));
        cx.stop_propagation();
        cx.notify();
    });

    div()
        .id(("fh-row", ix))
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .px_3()
        .py_px()
        .h(px(fh_row_height()))
        .flex_shrink_0()
        .bg(rgb(row_bg))
        .on_click(click)
        .on_mouse_down(MouseButton::Right, ctx)
        .cursor_pointer()
        // Hover uses the subtle `surface` tint (like the commit panel / branch
        // list), NOT `selected` — using the selection colour made a hovered row
        // indistinguishable from the selected one, so the row the mouse was left
        // on after a click looked "still selected" while the arrows moved the
        // real selection elsewhere. The selected row keeps its colour on hover.
        .when(!is_selected, |el| el.hover(|s| s.bg(rgb(theme().surface))))
        // change-type letter
        .child(
            div()
                .w(theme::scaled_px(18.))
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        // subject
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .truncate()
                .child(subject),
        )
        // author
        .child(
            div()
                .w(theme::scaled_px(90.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(author),
        )
        // relative date
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(date),
        )
        // +ins / -del
        .child(
            div()
                .w(theme::scaled_px(72.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .truncate()
                .child(stat),
        )
        // short hash
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .truncate()
                .child(short_hash),
        )
}
