//! File History right detail pane + commit-row context menu (ADR-0089), moved
//! from the bin's `src/ui/file_history_render.rs` (ADR-0121 C3).
//! Behaviour-preserving move — parent back-calls became [`FileHistoryEvent`]s.

use gpui::prelude::*;
use gpui::{div, rgb, ClipboardItem, Context, MouseButton, SharedString};

use kagi_domain::commit::CommitId;
use kagi_domain::file_history::FileHistoryEntry;
use kagi_ui_core::theme::{self, theme};

use crate::render::fh_header_button;
use crate::{change_type_label, FileHistoryEvent, FileHistoryState, FileHistoryView};

/// Right detail pane for the selected File History entry.
pub(crate) fn render_fh_detail_pane(
    state: &FileHistoryState,
    panel_width: f32,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // Clone the entry out so listeners can capture owned data.
    let entry: Option<FileHistoryEntry> = state.selected_entry().cloned();

    let mut pane = div()
        .id("fh-detail-pane")
        .w(theme::scaled_px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .bg(rgb(theme().panel))
        .overflow_y_scroll();

    let Some(entry) = entry else {
        return pane
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("No entry selected.")),
            )
            .into_any_element();
    };

    let line = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(value)),
            )
    };

    let ct = entry.change.change_type;
    let ct_label = change_type_label(ct).to_string();
    let stat = if entry.change.is_binary {
        "binary".to_string()
    } else {
        format!(
            "+{} \u{2212}{}",
            entry.change.insertions.unwrap_or(0),
            entry.change.deletions.unwrap_or(0)
        )
    };
    let path_after = entry.change.path_after.to_string_lossy().into_owned();
    let path_before = entry
        .change
        .path_before
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());

    if let Some(c) = entry.commit.as_ref() {
        let full = c.full_hash.clone();
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(c.subject.clone())),
            )
            .child(line("Full Hash", c.full_hash.clone()))
            .child(line("Short Hash", c.short_hash.clone()));

        if let Some(body) = c.body.as_ref() {
            pane = pane.child(line("Message", body.clone()));
        }
        pane = pane
            .child(line(
                "Author",
                format!("{} <{}>", c.author_name, c.author_email),
            ))
            .child(line("Committer", c.committer_name.clone()))
            .child(line("Author Date", c.author_date.clone()))
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path After", path_after));
        if let Some(before) = path_before {
            pane = pane.child(line("Path Before", before));
        }

        // ── Actions ──
        let id_open = CommitId(full.clone());
        let id_graph = CommitId(full.clone());
        let full_for_copy = full.clone();
        let actions = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .mt_2()
            .child(fh_header_button(
                "fh-detail-open",
                "Open Commit",
                move |_this, _e, _w, cx| {
                    cx.emit(FileHistoryEvent::JumpToCommit(id_open.clone()));
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-graph",
                "Show in Graph",
                move |_this, _e, _w, cx| {
                    cx.emit(FileHistoryEvent::JumpToCommit(id_graph.clone()));
                },
                cx,
            ))
            .child(fh_header_button(
                "fh-detail-copy",
                "Copy Hash",
                move |_this, _e, _w, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(full_for_copy.clone()));
                },
                cx,
            ));
        pane = pane.child(actions);
    } else {
        // WIP entry — minimal detail.
        pane = pane
            .child(
                div()
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from("Uncommitted changes")),
            )
            .child(line("Change Type", ct_label))
            .child(line("Changes", stat))
            .child(line("Path", path_after));
    }

    pane.into_any_element()
}

/// Context menu for a File History commit row (ADR-0089).
pub(crate) fn render_fh_row_menu(
    state: &FileHistoryState,
    ix: usize,
    pos: gpui::Point<gpui::Pixels>,
    cx: &mut Context<FileHistoryView>,
) -> gpui::AnyElement {
    // Resolve the entry's data up front (commit hash + path at this commit).
    let (commit_hash, path_at) = {
        let entry = state.history.as_ref().and_then(|h| h.entries.get(ix));
        let commit_hash = entry
            .and_then(|e| e.commit.as_ref())
            .map(|c| c.full_hash.clone());
        let path_at = entry.map(|e| e.change.path_after.to_string_lossy().into_owned());
        (commit_hash, path_at)
    };

    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _w, cx| {
        this.menu = None;
        cx.notify();
    });

    fn item<F>(id: &'static str, label: &'static str, on_click: F) -> gpui::Stateful<gpui::Div>
    where
        F: Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
    {
        div()
            .id(id)
            .px_3()
            .py(theme::scaled_px(3.))
            .text_sm()
            .text_color(rgb(theme().text_main))
            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
            .on_click(on_click)
            .child(SharedString::from(label))
    }

    let mut menu = div()
        .absolute()
        .left(pos.x)
        .top(pos.y)
        .w(theme::scaled_px(220.))
        .occlude()
        .bg(rgb(theme().panel))
        .border_1()
        .border_color(rgb(theme().surface))
        .rounded_md()
        .shadow_lg()
        .py(theme::scaled_px(2.));

    if let Some(hash) = commit_hash.clone() {
        let h1 = hash.clone();
        menu = menu.child(item(
            "fh-menu-copy-hash",
            "Copy Commit Hash",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(h1.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(p) = path_at.clone() {
        menu = menu.child(item(
            "fh-menu-copy-path",
            "Copy File Path at This Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.write_to_clipboard(ClipboardItem::new_string(p.clone()));
                cx.notify();
            }),
        ));
    }
    if let Some(hash) = commit_hash.clone() {
        let id_open = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-open-commit",
            "Open Commit",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.emit(FileHistoryEvent::JumpToCommit(id_open.clone()));
            }),
        ));
        let id_graph = CommitId(hash.clone());
        menu = menu.child(item(
            "fh-menu-graph",
            "Show Commit in Graph",
            cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                this.menu = None;
                cx.emit(FileHistoryEvent::JumpToCommit(id_graph.clone()));
            }),
        ));
    }

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(menu)
        .into_any_element()
}
