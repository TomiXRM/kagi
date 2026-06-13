//! W32-CONFLICT-EDITOR: a dedicated hunk-level Conflict Editor (ADR-0064).
//!
//! This is the **UI half** of the hunk-level conflict feature.  It renders from
//! an immutable [`ConflictMode`] snapshot (held by [`KagiApp`]) plus the
//! per-file [`HunkModel`] the backend [`ResolutionBuffer`] holds (built in
//! `KagiApp::conflict_open_editor`).  No `git2` calls happen here.
//!
//! # Layout (ADR-0064)
//!
//! ```text
//! Top Toolbar: [‹ back] [path] [conflict n of m] [‹ prev] [next ›]
//!              [Open external tool] [Reset all] [Save]
//! Upper Split:  A = Current branch side          | B = Incoming side
//!               (per-hunk buttons under each pair, via uniform_list)
//! Lower:        Result / Output preview (per-line origin; unresolved hunks shown)
//! ```
//!
//! Every per-hunk action is an explicit **worded button** (never a checkbox):
//! Accept current / Accept incoming / Accept both (current then incoming /
//! incoming then current) / Edit result / Reset this hunk.  Selecting one
//! re-assembles the Result and the lower preview updates immediately.
//!
//! Terminology (ADR-0058): side labels come from `mode.labels()`; the words
//! "ours" / "theirs" never appear.  All prose is via [`Msg`] (en + ja).
//! Sizes go through [`theme::scaled_px`] so the editor respects zoom.

use gpui::{div, prelude::*, px, rgb, uniform_list, Context, SharedString, Window};

use kagi::git::resolution::{HunkChoice, LineOrigin, Region};

use super::conflict_view::ConflictMode;
use super::i18n::Msg;
use super::theme::{self, theme};
use super::KagiApp;

/// Render the full Conflict Editor, replacing the normal body while editing.
///
/// `path` is the conflicting file being edited (the selected file).  The hunk
/// model is read from the buffer; when it is absent (binary / single-sided /
/// no text merge) a guidance message is shown instead.
pub fn render_editor(
    mode: &ConflictMode,
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
        .child(render_toolbar(mode, path, cx))
        .child(render_split(mode, path, cx))
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Top toolbar
// ────────────────────────────────────────────────────────────

fn render_toolbar(
    mode: &ConflictMode,
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
        // Launch is W33's job; this lane only provides the entry point.
        this.conflict_editor_open_external(&p_ext);
        cx.notify();
    });
    let p_reset = path.to_path_buf();
    let reset = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_reset_all(&p_reset);
        cx.notify();
    });
    let p_save = path.to_path_buf();
    let save = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_save(&p_save);
        cx.notify();
    });

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
        .child(tool_button(
            "editor-open-external",
            Msg::EditorOpenExternal.t(),
            theme().text_sub,
            open_ext,
        ))
        .child(tool_button("editor-reset", Msg::EditorReset.t(), theme().color_warning, reset))
        .child(tool_button("editor-save", Msg::EditorSave.t(), theme().color_success, save))
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

// ────────────────────────────────────────────────────────────
// Upper split (A / B) + per-hunk buttons + lower Result preview
// ────────────────────────────────────────────────────────────

fn render_split(
    mode: &ConflictMode,
    path: &std::path::Path,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let Some(model) = mode.buffer.hunk_model(path) else {
        return div()
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
                    .child(SharedString::from(Msg::EditorNoTextMerge.t())),
            )
            .into_any_element();
    };

    let labels = mode.labels();
    let current_label = format!("{} — {}", Msg::EditorCurrentSide.t(), labels.current.name);
    let incoming_label = format!("{} — {}", Msg::EditorIncomingSide.t(), labels.incoming.name);

    // Collect a flat per-hunk view-model (index + the three side groups + choice).
    let hunks: Vec<HunkVM> = model
        .regions
        .iter()
        .filter_map(|r| match r {
            Region::Hunk(h) => Some(h),
            Region::Passthrough(_) => None,
        })
        .enumerate()
        .map(|(i, h)| HunkVM {
            index: i,
            current: h.current.clone(),
            incoming: h.incoming.clone(),
            resolved: h.is_resolved(),
        })
        .collect();
    let hunk_count = hunks.len();
    let path_owned = path.to_path_buf();

    let upper = div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(theme::scaled_px(120.))
        .w_full()
        // Side headers (A | B).
        .child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .border_b_1()
                .border_color(rgb(theme().surface))
                .child(side_header(current_label, theme().color_branch))
                .child(side_header(incoming_label, theme().color_remote)),
        )
        // Virtualized hunk rows.
        .child(
            uniform_list(
                "conflict-editor-hunks",
                hunk_count,
                cx.processor(move |_this, range: std::ops::Range<usize>, _w, cx| {
                    range
                        .filter_map(|i| hunks.get(i).cloned())
                        .map(|vm| render_hunk_row(&path_owned, &vm, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .flex_grow()
            .w_full(),
        );

    div()
        .flex()
        .flex_col()
        .size_full()
        .child(upper)
        .child(render_result_preview(mode, path))
        .into_any_element()
}

fn side_header(label: String, accent: u32) -> gpui::Div {
    div()
        .flex_1()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(5.))
        .text_size(theme::scaled_px(11.))
        .text_color(rgb(accent))
        .child(SharedString::from(label))
}

/// A flattened per-hunk view-model carried into the `uniform_list` processor.
#[derive(Clone)]
struct HunkVM {
    index: usize,
    current: Vec<String>,
    incoming: Vec<String>,
    resolved: bool,
}

fn render_hunk_row(
    path: &std::path::Path,
    vm: &HunkVM,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let idx = vm.index;
    let bg = if idx % 2 == 0 { theme().bg_base } else { theme().bg_row_alt };

    // Side text columns (A | B).
    let side_a = side_column(&vm.current, theme().color_branch);
    let side_b = side_column(&vm.incoming, theme().color_remote);

    // Per-hunk worded buttons (NOT checkboxes).
    let mk = |id: &'static str, label: &str, accent: u32, choice: HunkChoice| {
        let p = path.to_path_buf();
        let handler = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
            this.conflict_editor_apply_hunk(&p, idx, choice.clone());
            cx.notify();
        });
        hunk_button(format!("{}-{}", id, idx), label, accent, handler)
    };

    let buttons = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(6.))
        .child(mk("acc-current", Msg::EditorAcceptCurrent.t(), theme().color_branch, HunkChoice::AcceptCurrent))
        .child(mk("acc-incoming", Msg::EditorAcceptIncoming.t(), theme().color_remote, HunkChoice::AcceptIncoming))
        .child(mk(
            "acc-both-cf",
            Msg::EditorAcceptBothCurrentFirst.t(),
            theme().text_sub,
            HunkChoice::BothCurrentFirst,
        ))
        .child(mk(
            "acc-both-if",
            Msg::EditorAcceptBothIncomingFirst.t(),
            theme().text_sub,
            HunkChoice::BothIncomingFirst,
        ))
        // "Edit result" seeds a manual edit with the current side (MVP: the
        // backend `set_manual_text` / in-app text entry is W30/v0.2; here the
        // button commits a manual hunk pre-filled with the current side so the
        // provenance path is exercised and the wording is present per ADR-0064).
        .child(mk(
            "edit-result",
            Msg::EditorEditResult.t(),
            theme().text_label,
            HunkChoice::Manual(join_lines(&vm.current)),
        ))
        .child(mk("reset-hunk", Msg::EditorResetHunk.t(), theme().color_blocker, HunkChoice::Unresolved));

    let status = if vm.resolved {
        None
    } else {
        Some(
            div()
                .px(theme::scaled_px(10.))
                .pb(theme::scaled_px(4.))
                .text_size(theme::scaled_px(10.))
                .text_color(rgb(theme().color_warning))
                .child(SharedString::from(Msg::EditorHunkUnresolved.t())),
        )
    };

    div()
        .id(("conflict-editor-hunk", idx))
        .flex()
        .flex_col()
        .w_full()
        .bg(rgb(bg))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(div().flex().flex_row().w_full().child(side_a).child(side_b))
        .child(buttons)
        .children(status)
        .into_any_element()
}

fn side_column(lines: &[String], accent: u32) -> gpui::Div {
    let mut col = div()
        .flex_1()
        .flex()
        .flex_col()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(4.))
        .border_r_1()
        .border_color(rgb(theme().surface));
    if lines.is_empty() {
        col = col.child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(empty)")),
        );
    }
    for l in lines {
        col = col.child(
            div()
                .text_size(theme::scaled_px(12.))
                .text_color(rgb(accent))
                .child(SharedString::from(l.clone())),
        );
    }
    col
}

fn hunk_button<H>(id: String, label: &str, accent: u32, handler: H) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(id))
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

/// Join side lines into a single text block (with trailing newline) for a
/// manual-edit seed.  `chars()`-safe (no byte slicing).
fn join_lines(lines: &[String]) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
    out
}

// ────────────────────────────────────────────────────────────
// Lower: Result / Output preview (per-line provenance)
// ────────────────────────────────────────────────────────────

fn render_result_preview(mode: &ConflictMode, path: &std::path::Path) -> gpui::AnyElement {
    let all_resolved = mode.buffer.hunks_all_resolved(path);
    let model = mode.buffer.hunk_model(path);
    let unresolved = model
        .map(|m| m.hunk_count() - m.resolved_hunk_count())
        .unwrap_or(0);

    let header_text = if all_resolved {
        Msg::EditorAllResolved.t().to_string()
    } else {
        format!("{} {}", unresolved, Msg::EditorUnresolvedHunks.t())
    };
    let header_color = if all_resolved {
        theme().color_success
    } else {
        theme().color_warning
    };

    // Assembled lines with provenance from the hunk model (falls back to the
    // file Result if the model is somehow absent).
    let mut body = div().flex().flex_col().w_full();
    if let Some(m) = model {
        for line in m.assemble() {
            let (tag, color) = origin_tag(line.origin);
            body = body.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .w(theme::scaled_px(56.))
                            .text_size(theme::scaled_px(10.))
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(tag)),
                    )
                    .child(
                        div()
                            .flex_grow()
                            .text_size(theme::scaled_px(12.))
                            .text_color(rgb(color))
                            .child(SharedString::from(line.text.clone())),
                    ),
            );
        }
    }

    div()
        .id("conflict-editor-result")
        .flex()
        .flex_col()
        .w_full()
        .h(theme::scaled_px(220.))
        .border_t_1()
        .border_color(rgb(theme().surface))
        .overflow_y_scroll()
        .px(theme::scaled_px(10.))
        .py(theme::scaled_px(6.))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .pb(px(4.))
                .child(
                    div()
                        .text_size(theme::scaled_px(11.))
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from(Msg::EditorResultOutput.t())),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(11.))
                        .text_color(rgb(header_color))
                        .child(SharedString::from(header_text)),
                ),
        )
        .child(body)
        .into_any_element()
}

/// Localized provenance tag + display color for a Result line origin.
fn origin_tag(origin: LineOrigin) -> (&'static str, u32) {
    match origin {
        LineOrigin::Context => (Msg::EditorOriginContext.t(), theme().text_sub),
        LineOrigin::Current => (Msg::EditorOriginCurrent.t(), theme().color_branch),
        LineOrigin::Incoming => (Msg::EditorOriginIncoming.t(), theme().color_remote),
        LineOrigin::Manual => (Msg::EditorOriginManual.t(), theme().text_label),
    }
}
