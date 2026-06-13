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

use gpui::{canvas, div, prelude::*, px, relative, rgb, Bounds, Context, Pixels, SharedString, Window};
use gpui_component::input::Input;

use kagi::git::resolution::{HunkChoice, Region};

use super::conflict_view::ConflictMode;
use super::conflict_view::EditorChrome;
use super::i18n::Msg;
use super::theme::{self, theme};
use super::{DividerDrag, DividerGhost, DividerKind, KagiApp};

/// The aggregate accept state of a file's hunks, used to drive the header
/// checkbox toggles (UX-010).  Computed from the per-hunk `HunkChoice`s.
#[derive(Clone, Copy, PartialEq)]
struct AcceptState {
    /// Every hunk currently accepts the current side.
    current: bool,
    /// Every hunk currently accepts the incoming side.
    incoming: bool,
}

/// Compute the file-level accept state from the hunk model.
fn accept_state(mode: &ConflictMode, path: &std::path::Path) -> AcceptState {
    let Some(model) = mode.buffer.hunk_model(path) else {
        return AcceptState { current: false, incoming: false };
    };
    let hunks: Vec<&kagi::git::resolution::ConflictHunk> = model
        .regions
        .iter()
        .filter_map(|r| match r {
            Region::Hunk(h) => Some(h),
            Region::Passthrough(_) => None,
        })
        .collect();
    if hunks.is_empty() {
        return AcceptState { current: false, incoming: false };
    }
    let all_current = hunks.iter().all(|h| h.choice == HunkChoice::AcceptCurrent);
    let all_incoming = hunks.iter().all(|h| h.choice == HunkChoice::AcceptIncoming);
    let all_both = hunks.iter().all(|h| {
        matches!(h.choice, HunkChoice::BothCurrentFirst | HunkChoice::BothIncomingFirst)
    });
    AcceptState {
        current: all_current || all_both,
        incoming: all_incoming || all_both,
    }
}

/// Apply a `HunkChoice` to **every** hunk of the file (MVP file-level mapping of
/// the header accept toggles / both-order buttons — UX-010/011/012).
fn apply_all_hunks(
    this: &mut KagiApp,
    path: &std::path::Path,
    choice: HunkChoice,
) {
    let n = this
        .conflict
        .as_ref()
        .and_then(|c| c.buffer.hunk_model(path).map(|m| m.hunk_count()))
        .unwrap_or(0);
    for i in 0..n {
        this.conflict_editor_apply_hunk(path, i, choice.clone());
    }
}

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
            div()
                .flex_grow()
                .flex()
                .flex_col()
                .min_w(px(0.))
                .child(
                    div()
                        .text_size(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(path_str)),
                )
                .child(
                    div()
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
        .px(theme::scaled_px(9.))
        .py(theme::scaled_px(4.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(11.))
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
        .px(theme::scaled_px(9.))
        .py(theme::scaled_px(4.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(
            gpui::svg()
                .path(icon_path)
                .w(theme::scaled_px(13.))
                .h(theme::scaled_px(13.))
                .text_color(rgb(accent)),
        )
        .child(SharedString::from(label.to_string()))
        .on_click(handler)
}

// ────────────────────────────────────────────────────────────
// 3-pane body: (A | B) row  +  both-order strip  +  Result pane
// ────────────────────────────────────────────────────────────

fn render_panes(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // No hunk model (binary / single-sided) → guidance message.
    let Some(inputs) = chrome.inputs.as_ref().filter(|i| i.path == path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };
    if mode.buffer.hunk_model(path).is_none() {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    }

    let labels = mode.labels();
    let current_label =
        format!("{} — {}", Msg::EditorCurrentSide.t(), labels.current.name);
    let incoming_label =
        format!("{} — {}", Msg::EditorIncomingSide.t(), labels.incoming.name);

    let accept = accept_state(mode, path);

    // ── A | B row (resizable A|B), measured for the vertical divider drag ──
    let ab_geom = chrome.ab_geom.clone();
    let ab_measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            ab_geom.set((f32::from(bounds.origin.x), f32::from(bounds.origin.x + bounds.size.width)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();

    let a_pane = pane(
        "conflict-pane-a",
        current_label,
        theme().color_branch,
        Some(accept_toggle(
            "accept-a",
            accept.current,
            theme().color_branch,
            path,
            HunkChoice::AcceptCurrent,
            cx,
        )),
        Input::new(&inputs.current)
            .disabled(true)
            .appearance(true)
            .bordered(false)
            .h_full(),
    )
    .child(ab_measure);

    let b_pane = pane(
        "conflict-pane-b",
        incoming_label,
        theme().color_remote,
        Some(accept_toggle(
            "accept-b",
            accept.incoming,
            theme().color_remote,
            path,
            HunkChoice::AcceptIncoming,
            cx,
        )),
        Input::new(&inputs.incoming)
            .disabled(true)
            .appearance(true)
            .bordered(false)
            .h_full(),
    );

    let ab_row = div()
        .relative()
        .flex()
        .flex_row()
        .w_full()
        .h(relative(chrome.result_split))
        .min_h(theme::scaled_px(80.))
        .child(div().h_full().min_w(px(0.)).w(relative(chrome.ab_split)).child(a_pane))
        .child(vertical_divider())
        .child(div().h_full().min_w(px(0.)).flex_1().child(b_pane));

    // ── Both-order strip (between A·B and Result) — UX-011 ──
    let both_strip = both_order_strip(accept, path, cx);

    // ── Result pane (resizable A·B / Result) ──
    let result_pane = render_result_pane(mode, chrome, path, cx);

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
        .child(both_strip)
        .child(horizontal_divider())
        .child(result_pane)
        .into_any_element()
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
        .border_color(rgb(theme().surface))
        // Editor background a touch darker than the surrounding chrome (UI-002).
        .bg(rgb(theme().bg_base))
        .child(header)
        .child(
            div()
                .flex_grow()
                .w_full()
                .min_h(px(0.))
                .child(editor),
        )
}

/// A pane-header accept toggle (☑/☐) — UX-010.  Checking it accepts that side
/// for every hunk of the file (MVP file-level mapping); unchecking resets.
fn accept_toggle(
    id: &'static str,
    checked: bool,
    accent: u32,
    path: &std::path::Path,
    choice: HunkChoice,
    cx: &mut Context<KagiApp>,
) -> gpui::Stateful<gpui::Div> {
    let p = path.to_path_buf();
    let handler = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        if checked {
            // Already accepted → toggle off (reset all hunks to unresolved).
            apply_all_hunks(this, &p, HunkChoice::Unresolved);
        } else {
            apply_all_hunks(this, &p, choice.clone());
        }
        cx.notify();
    });
    let glyph = if checked { "\u{2611}" } else { "\u{2610}" }; // ☑ / ☐
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px(theme::scaled_px(6.))
        .py(theme::scaled_px(2.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(SharedString::from(glyph))
        .child(SharedString::from(Msg::EditorAccept.t()))
        .on_click(handler)
}

/// The both-order strip placed between the A·B row and the Result pane (UX-011).
/// Highlights the active order when both sides are currently accepted.
fn both_order_strip(
    accept: AcceptState,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::Div {
    let both = accept.current && accept.incoming;

    let p_cf = path.to_path_buf();
    let cf = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        apply_all_hunks(this, &p_cf, HunkChoice::BothCurrentFirst);
        cx.notify();
    });
    let p_if = path.to_path_buf();
    let iff = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        apply_all_hunks(this, &p_if, HunkChoice::BothIncomingFirst);
        cx.notify();
    });

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(5.))
        .bg(rgb(theme().surface))
        .child(
            div()
                .text_size(theme::scaled_px(10.))
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(Msg::EditorBothLabel.t())),
        )
        .child(both_button(
            "both-cf",
            Msg::EditorAcceptBothCurrentFirst.t(),
            both,
            cf,
        ))
        .child(both_button(
            "both-if",
            Msg::EditorAcceptBothIncomingFirst.t(),
            false,
            iff,
        ))
}

fn both_button<H>(
    id: &'static str,
    label: &str,
    active: bool,
    handler: H,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let accent = if active { theme().color_success } else { theme().text_sub };
    div()
        .id(id)
        .px(theme::scaled_px(8.))
        .py(theme::scaled_px(3.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(accent))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .child(SharedString::from(label.to_string()))
        .on_click(handler)
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

    // Body: read-only in Preview, editable in Edit (same InputState; the app's
    // sync pass pulls edits into the buffer via set_manual_text).
    let editor = Input::new(&inputs.result)
        .disabled(!editing)
        .appearance(true)
        .bordered(false)
        .h_full();

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
        .child(div().flex_grow().w_full().min_h(px(0.)).child(editor))
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
        .bg(rgb(theme().surface))
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
        .bg(rgb(theme().surface))
        .cursor_row_resize()
        .hover(|s| s.bg(rgb(theme().color_branch)))
        .on_drag(DividerDrag { kind: DividerKind::ConflictResult }, |_, _, _, cx| {
            cx.new(|_| DividerGhost)
        })
}
