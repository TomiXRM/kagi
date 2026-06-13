//! T-CONFLICT-UI/UX: the dedicated 3-pane Conflict Editor (ADR-0064 / 0069 / 0070).
//!
//! This is the **UI half** of the hunk-level conflict feature.  It renders from
//! an immutable [`ConflictMode`] snapshot (held by [`KagiApp`]) plus the
//! [`EditorChrome`] the app passes in (the three CodeEditor `InputState`s, the
//! split ratios, the Result Preview/Edit mode flag, and the measured-bounds
//! geometry cells for the resize handles).  No `git2` calls happen here.
//!
//! # Layout (ADR-0064 / 0069)
//!
//! ```text
//! Top Toolbar: [path] [conflict n/m] [‹ prev] [next ›]   ……   [↗ external] [🗑 reset]
//! ┌──────────────────────────────┬──────────────────────────────┐
//! │ A · Current  ☑accept          │ B · Incoming  ☐accept         │   (resizable A|B)
//! │ <branch/commit label>         │ <branch/commit label>         │
//! │ [CodeEditor InputState · RO]  │ [CodeEditor InputState · RO]  │
//! └──────────────────────────────┴──────────────────────────────┘
//!   [Both: current → incoming] [Both: incoming → current]   (between A·B and Result)
//! ──────────────────────────── resize ────────────────────────────
//! │ Result   [Preview | Edit]   <editing indicator>               │   (resizable A·B/Result)
//! │ [CodeEditor InputState · RO in Preview / editable in Edit]    │
//! └───────────────────────────────────────────────────────────────┘
//! ```
//!
//! The accept controls are **checkbox toggles in each pane header** (UX-010);
//! the both-order buttons sit **between** the A·B row and the Result pane
//! (UX-011).  Both drive the existing per-hunk apply
//! (`conflict_editor_apply_hunk` / `HunkChoice`).  MVP stays hunk-level — the
//! header toggles apply to *all* hunks of the file (the simplest extensible
//! mapping; per-line is v0.2 per UX-012).
//!
//! Terminology (ADR-0058): side labels come from `mode.labels()`; the words
//! "ours" / "theirs" never appear.  All prose is via [`Msg`] (en + ja).  Sizes
//! go through [`theme::scaled_px`] so the editor respects zoom.

use std::sync::Arc;

use gpui::{
    canvas, div, prelude::*, px, relative, rgb, uniform_list, AnyElement, Bounds, Context, Pixels,
    SharedString, UniformListScrollHandle, Window,
};
use gpui_component::input::Input;
use gpui_component::scroll::Scrollbar;

use kagi::git::resolution::{LineOrder, Region, SelectionSide, TriState};

use super::conflict_view::ConflictMode;
use super::conflict_view::EditorChrome;
use super::i18n::Msg;
use super::theme::{self, theme};
use super::{terminal, DividerDrag, DividerGhost, DividerKind, KagiApp};

/// Render the full Conflict Editor, replacing the normal body while editing.
pub fn render_editor(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Embedded as the CENTER main area; the right Conflict Dashboard is always
    // rendered alongside, so flex within the row instead of taking the window.
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .h_full()
        .min_w(px(0.))
        .bg(rgb(theme().bg_base))
        .child(render_toolbar(mode, chrome, path, cx))
        .child(render_panes(mode, chrome, path, cx))
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Top toolbar — file-level path + nav; external-tool + reset icons (POLISH-040/041)
// ────────────────────────────────────────────────────────────

fn render_toolbar(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let path_str = path.to_string_lossy().into_owned();
    let total = mode.buffer.hunk_count(path);
    let resolved = mode
        .buffer
        .hunk_model(path)
        .map(|m| m.resolved_hunk_count())
        .unwrap_or(0);
    let n_of_m = format!("{} {}/{}", Msg::EditorConflictNofM.t(), resolved, total);

    let prev = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_nav_hunk(-1);
        cx.notify();
    });
    let next = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_nav_hunk(1);
        cx.notify();
    });
    let p_ext = path.to_path_buf();
    let open_ext = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_open_external(&p_ext);
        cx.notify();
    });
    let p_reset = path.to_path_buf();
    let reset = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_reset_all_request(&p_reset);
        cx.notify();
    });
    let reset_armed = chrome.reset_all_armed;

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(6.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().color_warning))
        .child(
            // file name + conflict n/m laid out horizontally (was stacked).
            div()
                .flex_grow()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .child(SharedString::from(path_str)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::scaled_px(10.))
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(n_of_m)),
                ),
        )
        .child(tool_button("editor-prev", Msg::EditorPrevHunk.t(), theme().text_sub, prev))
        .child(tool_button("editor-next", Msg::EditorNextHunk.t(), theme().text_sub, next))
        .child(icon_button(
            "editor-open-external",
            "icons/external-link.svg",
            Msg::EditorOpenExternal.t(),
            theme().text_sub,
            open_ext,
        ))
        // Reset all — destructive: trash icon, armed → blocker colour + confirm label.
        .child(
            icon_button(
                "editor-reset",
                "icons/trash-2.svg",
                if reset_armed {
                    Msg::EditorResetAllConfirm.t()
                } else {
                    Msg::EditorReset.t()
                },
                if reset_armed { theme().color_blocker } else { theme().color_warning },
                reset,
            ),
        )
        .into_any_element()
}

fn tool_button<H>(id: &str, label: &str, accent: u32, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(id.to_string()))
        .px(theme::scaled_px(7.2))
        .py(theme::scaled_px(3.2))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(8.8))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(SharedString::from(label.to_string()))
        .on_click(handler)
}

/// An icon button with a compact text label beside the glyph (POLISH-040/041).
fn icon_button<H>(
    id: &str,
    icon_path: &'static str,
    label: &str,
    accent: u32,
    handler: H,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px(theme::scaled_px(7.2))
        .py(theme::scaled_px(3.2))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(8.8))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(
            gpui::svg()
                .path(icon_path)
                .w(theme::scaled_px(10.4))
                .h(theme::scaled_px(10.4))
                .text_color(rgb(accent)),
        )
        .child(SharedString::from(label.to_string()))
        .on_click(handler)
}

// ────────────────────────────────────────────────────────────
// 3-pane body: synchronized A/B row lists + Result pane
// ────────────────────────────────────────────────────────────

fn render_panes(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // No hunk model (binary / single-sided) → guidance message.
    let Some(_inputs) = chrome.inputs.as_ref().filter(|i| i.path == path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };
    let Some(model) = mode.buffer.hunk_model(path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };

    let labels = mode.labels();
    let current_label =
        format!("{} — {}", Msg::EditorCurrentSide.t(), labels.current.name);
    let incoming_label =
        format!("{} — {}", Msg::EditorIncomingSide.t(), labels.incoming.name);

    // ── A | B row (resizable A|B), measured for the vertical divider drag ──
    let ab_geom = chrome.ab_geom.clone();
    let ab_measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                eprintln!("[kagi] ab_geom left={:.1} right={:.1} width={:.1}", f32::from(bounds.origin.x), f32::from(bounds.origin.x + bounds.size.width), f32::from(bounds.size.width));
            }
            ab_geom.set((f32::from(bounds.origin.x), f32::from(bounds.origin.x + bounds.size.width)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    let scroll = chrome.ab_scroll.clone();
    let a_pane = pane(
        "conflict-pane-a",
        current_label,
        theme().color_branch,
        Some(side_file_checkbox(path, model.file_side_state(SelectionSide::Current), SelectionSide::Current, cx)),
        side_row_list(
            path,
            model,
            SelectionSide::Current,
            scroll.clone(),
            chrome.selected_hunk,
            cx,
        ),
    );

    let b_pane = pane(
        "conflict-pane-b",
        incoming_label,
        theme().color_remote,
        Some(side_file_checkbox(path, model.file_side_state(SelectionSide::Incoming), SelectionSide::Incoming, cx)),
        side_row_list(
            path,
            model,
            SelectionSide::Incoming,
            scroll,
            chrome.selected_hunk,
            cx,
        ),
    );

    let ab_row = div()
        .relative()
        .flex()
        .flex_row()
        .w_full()
        .flex_basis(relative(chrome.result_split))
        .flex_shrink()
        .min_h(theme::scaled_px(80.))
        // Measure the FULL A·B row width (not just the A pane) so the divider
        // drag maps the cursor against the whole span — measuring inside A would
        // shrink the span and feed back on itself, making resize unusable.
        .child(ab_measure)
        .child(div().h_full().min_w(px(0.)).w(relative(chrome.ab_split)).child(a_pane))
        .child(vertical_divider())
        .child(div().h_full().min_w(px(0.)).flex_1().child(b_pane));

    // ── Result pane (resizable A·B / Result) ──
    // flex_basis(relative) split like the inspector (NOT h(relative)) so the
    // child uniform_lists get a definite height and actually render their rows.
    let result_frac = (1.0 - chrome.result_split).max(0.05);
    let result_pane = div()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .flex_basis(relative(result_frac))
        .flex_shrink()
        .child(render_result_pane(mode, chrome, path, cx));

    // The split region is measured for the horizontal divider drag.
    let geom = chrome.geom.clone();
    let measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            geom.set((f32::from(bounds.origin.y), f32::from(bounds.origin.y + bounds.size.height)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    div()
        .relative()
        .flex()
        .flex_col()
        .size_full()
        .child(measure)
        .child(ab_row)
        .child(horizontal_divider())
        .child(result_pane)
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// A/B row lists: file/chunk/line tri-state checkbox hierarchy (ADR-0071)
// ────────────────────────────────────────────────────────────

#[derive(Clone)]
enum SideRow {
    HunkHeader { hunk_index: usize, state: TriState, order: LineOrder },
    Line {
        hunk_index: usize,
        line_index: usize,
        line_no: usize,
        text: String,
        taken: bool,
    },
}

fn side_file_checkbox(
    path: &std::path::Path,
    state: TriState,
    side: SelectionSide,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let p = path.to_path_buf();
    let next = state != TriState::All;
    let handler = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_set_file_side(&p, side, next);
        cx.notify();
    });
    tri_checkbox(
        format!("file-side-{:?}", side),
        state,
        theme().text_sub,
        handler,
    )
}

fn build_side_rows(
    model: &kagi::git::resolution::HunkModel,
    side: SelectionSide,
) -> Vec<SideRow> {
    let mut rows = Vec::new();
    let mut hunk_index = 0usize;
    let mut line_no = 1usize;
    for region in &model.regions {
        if let Region::Hunk(hunk) = region {
            let order = hunk
                .line_select
                .as_ref()
                .map(|selection| selection.order)
                .unwrap_or_else(|| match hunk.choice {
                    kagi::git::resolution::HunkChoice::BothIncomingFirst => {
                        LineOrder::IncomingFirst
                    }
                    _ => LineOrder::CurrentFirst,
                });
            rows.push(SideRow::HunkHeader {
                hunk_index,
                state: hunk.side_state(side),
                order,
            });
            let (lines, taken) = match side {
                SelectionSide::Current => (
                    &hunk.current,
                    hunk.line_select.as_ref().map(|s| s.current_taken.clone()),
                ),
                SelectionSide::Incoming => (
                    &hunk.incoming,
                    hunk.line_select.as_ref().map(|s| s.incoming_taken.clone()),
                ),
            };
            for (line_index, text) in lines.iter().enumerate() {
                let is_taken = taken
                    .as_ref()
                    .and_then(|values| values.get(line_index))
                    .copied()
                    .unwrap_or_else(|| hunk.side_state(side) == TriState::All);
                rows.push(SideRow::Line {
                    hunk_index,
                    line_index,
                    line_no,
                    text: text.clone(),
                    taken: is_taken,
                });
                line_no += 1;
            }
            hunk_index += 1;
        }
    }
    rows
}

fn side_row_list(
    path: &std::path::Path,
    model: &kagi::git::resolution::HunkModel,
    side: SelectionSide,
    scroll: UniformListScrollHandle,
    selected_hunk: usize,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let rows = Arc::new(build_side_rows(model, side));
    let row_count = rows.len();
    let rows_for_list = rows.clone();
    let p = Arc::new(path.to_path_buf());
    let (list_id, outer_id) = match side {
        SelectionSide::Current => ("conflict-current-lines", "conflict-current-lines-scroll"),
        SelectionSide::Incoming => ("conflict-incoming-lines", "conflict-incoming-lines-scroll"),
    };

    div()
        .id(outer_id)
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_x_scroll()
        .child(
            uniform_list(
                list_id,
                row_count,
                cx.processor(move |_this, range, _window, cx| {
                    render_side_rows(&rows_for_list, p.clone(), side, selected_hunk, range, cx)
                }),
            )
            .track_scroll(scroll.clone())
            .flex_1()
            .min_h(px(0.)),
        )
        .child(Scrollbar::vertical(&scroll))
}

fn render_side_rows(
    rows: &[SideRow],
    path: Arc<std::path::PathBuf>,
    side: SelectionSide,
    selected_hunk: usize,
    range: std::ops::Range<usize>,
    cx: &mut Context<KagiApp>,
) -> Vec<AnyElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row.clone())))
        .map(|(i, row)| match row {
            SideRow::HunkHeader { hunk_index, state, order } => {
                render_hunk_header_row(
                    i,
                    path.clone(),
                    hunk_index,
                    state,
                    order,
                    side,
                    selected_hunk,
                    cx,
                )
            }
            SideRow::Line { hunk_index, line_index, line_no, text, taken } => {
                render_code_line_row(
                    i,
                    path.clone(),
                    hunk_index,
                    line_index,
                    line_no,
                    text,
                    taken,
                    side,
                    selected_hunk,
                    cx,
                )
            }
        })
        .collect()
}

fn render_hunk_header_row(
    row_index: usize,
    path: Arc<std::path::PathBuf>,
    hunk_index: usize,
    state: TriState,
    order: LineOrder,
    side: SelectionSide,
    selected_hunk: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let next = state != TriState::All;
    let p = path.clone();
    let toggle = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_set_hunk_side(&p, hunk_index, side, next);
        cx.notify();
    });
    let p_order = path;
    let next_order = match order {
        LineOrder::CurrentFirst => LineOrder::IncomingFirst,
        LineOrder::IncomingFirst => LineOrder::CurrentFirst,
    };
    let order_click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_set_hunk_order(&p_order, hunk_index, next_order);
        cx.notify();
    });
    let focus_click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_select_hunk(hunk_index);
        cx.notify();
    });
    let order_label = match order {
        LineOrder::CurrentFirst => Msg::EditorCurrentFirst.t(),
        LineOrder::IncomingFirst => Msg::EditorIncomingFirst.t(),
    };
    div()
        .id(SharedString::from(format!("side-hunk-{}", row_index)))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .h(theme::scaled_px(20.))
        .px(theme::scaled_px(4.))
        // Blend with the editor bg (no lighter "stripe" band); only the focused
        // hunk gets a subtle tint. A thin top divider separates hunks instead.
        .border_t_1()
        .border_color(rgb(theme().selected))
        .bg(rgb(if selected_hunk == hunk_index { theme().bg_row_alt } else { theme().bg_base }))
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(focus_click)
        .child(tri_checkbox(
            format!("hunk-side-{:?}-{}", side, hunk_index),
            state,
            theme().text_sub,
            toggle,
        ))
        .child(
            div()
                .text_size(theme::scaled_px(10.))
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!(
                    "{} {}",
                    Msg::EditorHunkLabel.t(),
                    hunk_index + 1
                ))),
        )
        .child(
            div()
                .id(SharedString::from(format!("hunk-order-{}", row_index)))
                .ml_auto()
                .px(theme::scaled_px(6.))
                .py(theme::scaled_px(1.))
                .rounded_sm()
                .border_1()
                .border_color(rgb(theme().selected))
                .text_size(theme::scaled_px(9.))
                .text_color(rgb(theme().text_sub))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().bg_row_alt)))
                .child(SharedString::from(order_label))
                .on_click(order_click),
        )
        .into_any_element()
}

fn render_code_line_row(
    row_index: usize,
    path: Arc<std::path::PathBuf>,
    hunk_index: usize,
    line_index: usize,
    line_no: usize,
    text_value: String,
    taken: bool,
    side: SelectionSide,
    selected_hunk: usize,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    let p = path.clone();
    let toggle = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_set_hunk_line(&p, hunk_index, side, line_index, !taken);
        cx.notify();
    });
    let focus_click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_select_hunk(hunk_index);
        cx.notify();
    });
    let accent = match side {
        SelectionSide::Current => theme().color_branch,
        SelectionSide::Incoming => theme().color_remote,
    };
    div()
        .id(SharedString::from(format!("side-line-{}", row_index)))
        .flex()
        .flex_row()
        .items_center()
        .min_w(relative(1.0))
        .h(theme::scaled_px(17.))
        .px(theme::scaled_px(4.))
        .gap_1()
        .bg(rgb(if selected_hunk == hunk_index { theme().bg_row_alt } else { theme().bg_base }))
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(focus_click)
        .child(line_checkbox(
            taken,
            accent,
            format!("line-side-{:?}-{}-{}", side, hunk_index, line_index),
            toggle,
        ))
        .child(
            div()
                .w(theme::scaled_px(42.))
                .text_size(theme::scaled_px(11.))
                .line_height(theme::scaled_px(17.))
                .font_family(terminal::pick_font_family())
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{:>4}", line_no))),
        )
        .child(
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_size(theme::scaled_px(12.))
                .line_height(theme::scaled_px(17.))
                .font_family(terminal::pick_font_family())
                .text_color(rgb(if taken { theme().text_main } else { theme().text_muted }))
                .child(SharedString::from(text_value)),
        )
        .into_any_element()
}

fn tri_checkbox<H>(
    id: impl Into<String>,
    state: TriState,
    accent: u32,
    handler: H,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let glyph = match state {
        TriState::All => "\u{2611}",
        TriState::Partial => "\u{2014}",
        TriState::None => "\u{2610}",
    };
    // No outer box/border (user request): the ☑/☐/— glyph itself is the
    // checkbox; just give it a click target + colour.
    div()
        .id(SharedString::from(id.into()))
        .flex()
        .items_center()
        .justify_center()
        .w(theme::scaled_px(15.))
        .text_size(theme::scaled_px(15.))
        .line_height(theme::scaled_px(17.))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.text_color(rgb(theme().text_main)))
        .child(SharedString::from(glyph))
        .on_click(handler)
}

fn line_checkbox<H>(
    taken: bool,
    accent: u32,
    id: impl Into<String>,
    handler: H,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    tri_checkbox(
        id,
        if taken { TriState::All } else { TriState::None },
        accent,
        handler,
    )
}

fn guidance_pane(msg: &str) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .size_full()
        .child(
            div()
                .max_w(theme::scaled_px(420.))
                .text_size(theme::scaled_px(13.))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(msg.to_string())),
        )
        .into_any_element()
}

/// One pane: a header (title + branch/commit label + optional accept toggle) and
/// a CodeEditor body, with its own border + a slightly darker editor background
/// (T-CONFLICT-UI-002).
fn pane(
    id: &'static str,
    label: String,
    accent: u32,
    accept: Option<gpui::Stateful<gpui::Div>>,
    editor: impl IntoElement,
) -> gpui::Stateful<gpui::Div> {
    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .px(theme::scaled_px(8.))
        .py(theme::scaled_px(4.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .flex_grow()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(accent))
                .child(SharedString::from(label)),
        );
    if let Some(toggle) = accept {
        header = header.child(toggle);
    }

    div()
        .id(id)
        .flex()
        .flex_col()
        .size_full()
        .min_w(px(0.))
        .border_1()
        // UI-002: clearer pane border — `selected` reads against the darker
        // editor bg, unlike `surface` which nearly matches it.
        .border_color(rgb(theme().selected))
        // Editor background a touch darker than the surrounding chrome (UI-002).
        .bg(rgb(theme().bg_base))
        .child(header)
        .child(
            // Must be a flex container so the editor's `flex_1` resolves to a
            // definite height — otherwise the inner uniform_list measures 0 and
            // renders no rows (the A/B line lists came up blank).
            div()
                .flex()
                .flex_col()
                .flex_grow()
                .w_full()
                .min_h(px(0.))
                .child(editor),
        )
}


// ────────────────────────────────────────────────────────────
// Result pane: Preview (read-only) / Edit (editable) — UX-015
// ────────────────────────────────────────────────────────────

fn render_result_pane(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let Some(inputs) = chrome.inputs.as_ref().filter(|i| i.path == path) else {
        return div().flex_1().into_any_element();
    };
    let editing = chrome.result_editing;
    let all_resolved = mode.buffer.hunks_all_resolved(path);
    let model = mode.buffer.hunk_model(path);
    let unresolved = model
        .map(|m| m.hunk_count() - m.resolved_hunk_count())
        .unwrap_or(0);
    let status_text = if all_resolved {
        Msg::EditorAllResolved.t().to_string()
    } else {
        format!("{} {}", unresolved, Msg::EditorUnresolvedHunks.t())
    };
    let status_color = if all_resolved { theme().color_success } else { theme().color_warning };

    let toggle = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    // File-level "Save resolution" lives near the Result (deliverable #4).
    let p_save = path.to_path_buf();
    let save = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_save(&p_save);
        cx.notify();
    });

    // Header: "Result", a Preview|Edit segmented toggle, status, editing badge,
    // and the file-level Save resolution button.
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .px(theme::scaled_px(8.))
        .py(theme::scaled_px(4.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(Msg::EditorResultOutput.t())),
        )
        .child(mode_toggle(editing, toggle))
        .child(
            div()
                .flex_grow()
                .text_size(theme::scaled_px(10.))
                .text_color(rgb(status_color))
                .child(SharedString::from(status_text)),
        )
        .when(editing, |el| {
            el.child(
                div()
                    .text_size(theme::scaled_px(10.))
                    .text_color(rgb(theme().color_warning))
                    .child(SharedString::from(Msg::EditorEditingIndicator.t())),
            )
        })
        .child(tool_button(
            "editor-save",
            Msg::EditorSave.t(),
            theme().color_success,
            save,
        ));

    // Body: Preview renders custom monospace rows that exactly match the A/B
    // line lists (12px + terminal font); Edit uses the InputState so the text is
    // editable. (The InputState reads window.text_style at prepaint, which the
    // parent div's text-style cascade does NOT reach, so its font/size can't be
    // matched to the A/B rows — hence the custom Preview rendering.)
    let preview_body: gpui::AnyElement = if editing {
        div()
            .flex_grow()
            .w_full()
            .min_h(px(0.))
            .child(
                Input::new(&inputs.result)
                    .disabled(false)
                    .appearance(true)
                    .bordered(false)
                    .h_full(),
            )
            .into_any_element()
    } else {
        let text = mode
            .buffer
            .hunk_model(path)
            .map(|m| m.assembled_text())
            .unwrap_or_default();
        let mut col = div()
            .id("conflict-result-preview")
            .flex()
            .flex_col()
            .flex_grow()
            .min_h(px(0.))
            .w_full()
            .overflow_scroll();
        for (i, line) in text.lines().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .min_w(relative(1.0))
                    .h(theme::scaled_px(17.))
                    .px(theme::scaled_px(4.))
                    .gap_1()
                    .child(
                        div()
                            .w(theme::scaled_px(42.))
                            .flex_shrink_0()
                            .text_size(theme::scaled_px(11.))
                            .line_height(theme::scaled_px(17.))
                            .font_family(terminal::pick_font_family())
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(format!("{:>4}", i + 1))),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_size(theme::scaled_px(12.))
                            .line_height(theme::scaled_px(17.))
                            .font_family(terminal::pick_font_family())
                            .text_color(rgb(theme().text_main))
                            .child(SharedString::from(line.to_string())),
                    ),
            );
        }
        col.into_any_element()
    };

    div()
        .id("conflict-pane-result")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .border_1()
        .border_color(rgb(theme().surface))
        .bg(rgb(theme().bg_base))
        .child(header)
        .child(preview_body)
        .into_any_element()
}

/// A two-segment Preview | Edit toggle (UX-015).
fn mode_toggle<H>(editing: bool, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let seg = |label: &str, active: bool| {
        let accent = if active { theme().color_branch } else { theme().text_sub };
        div()
            .px(theme::scaled_px(7.))
            .py(theme::scaled_px(2.))
            .text_size(theme::scaled_px(10.))
            .text_color(rgb(accent))
            .when(active, |s| s.bg(rgb(theme().selected)))
            .child(SharedString::from(label.to_string()))
    };
    div()
        .id("result-mode-toggle")
        .flex()
        .flex_row()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().surface))
        .overflow_hidden()
        .cursor_pointer()
        .child(seg(Msg::EditorPreviewMode.t(), !editing))
        .child(seg(Msg::EditorEditMode.t(), editing))
        .on_click(handler)
}

// ────────────────────────────────────────────────────────────
// Resize handles (W7 measured-bounds + Rc<Cell> drag pattern) — UI-003
// ────────────────────────────────────────────────────────────

/// Vertical divider between the A and B panes (drives the A|B width ratio).
fn vertical_divider() -> gpui::Stateful<gpui::Div> {
    div()
        .id("conflict-divider-ab")
        .w(theme::scaled_px(4.))
        .h_full()
        .bg(rgb(theme().selected))
        .cursor_col_resize()
        .hover(|s| s.bg(rgb(theme().color_branch)))
        .on_drag(DividerDrag { kind: DividerKind::ConflictAB }, |_, _, _, cx| {
            cx.new(|_| DividerGhost)
        })
}

/// Horizontal divider between the A·B row and the Result pane (drives the
/// A·B / Result height ratio).
fn horizontal_divider() -> gpui::Stateful<gpui::Div> {
    div()
        .id("conflict-divider-result")
        .w_full()
        .h(theme::scaled_px(4.))
        .bg(rgb(theme().selected))
        .cursor_row_resize()
        .hover(|s| s.bg(rgb(theme().color_branch)))
        .on_drag(DividerDrag { kind: DividerKind::ConflictResult }, |_, _, _, cx| {
            cx.new(|_| DividerGhost)
        })
}
