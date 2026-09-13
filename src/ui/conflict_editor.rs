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
//! ADR-0135: the accept controls are Xcode-style **coloured accent bands**, not
//! checkboxes. Clicking a hunk's band header takes/releases that whole side;
//! clicking an individual line toggles just that line (untaken lines render
//! dimmed); a small "Use all" pill in each pane header takes the whole file's
//! side. All of it drives the same tri-state selection model (ADR-0071) — only
//! the control surface changed. A/B rows are syntax-highlighted through the
//! same tree-sitter + per-theme pipeline as the diff (ADR-0133), cached per
//! (path, theme) so selection clicks never re-parse.
//!
//! Terminology (ADR-0058): side labels come from `mode.labels()`; the words
//! "ours" / "theirs" never appear.  All prose is via [`Msg`] (en + ja).  Sizes
//! go through [`theme::scaled_px`] so the editor respects zoom.

use std::ops::Range;
use std::sync::Arc;

use gpui::{
    canvas, div, prelude::*, px, relative, rgb, uniform_list, AnyElement, Bounds, Context, Pixels,
    SharedString, UniformListScrollHandle, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::scroll::Scrollbar;
use gpui_component::{Disableable as _, Sizable as _};

use kagi_git::resolution::{LineOrder, LineOrigin, Region, SelectionSide, TriState};

use super::button_style::KagiButton;
use super::conflict_view::ConflictMode;
use super::conflict_view::ConflictView;
use super::conflict_view::EditorChrome;
use super::i18n::Msg;
use super::theme::{self, theme};
use super::{terminal, DividerDrag, DividerGhost, DividerKind};

/// Render the full Conflict Editor, replacing the normal body while editing.
pub fn render_editor(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    // Embedded as the CENTER main area; the right Conflict Dashboard is always
    // rendered alongside, so flex within the row instead of taking the window.
    div()
        .flex()
        .flex_col()
        .flex_grow(1.)
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
    cx: &mut Context<ConflictView>,
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
        this.conflict_editor_open_external(&p_ext, cx);
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
                .flex_grow(1.)
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
        .child(tool_button(
            "editor-prev",
            Msg::EditorPrevHunk.t(),
            theme().text_sub,
            prev,
            cx,
        ))
        .child(tool_button(
            "editor-next",
            Msg::EditorNextHunk.t(),
            theme().text_sub,
            next,
            cx,
        ))
        .child(icon_button(
            "editor-open-external",
            "icons/external-link.svg",
            Msg::EditorOpenExternal.t(),
            theme().text_sub,
            open_ext,
            cx,
        ))
        // Reset all — destructive: trash icon, armed → blocker colour + confirm label.
        .child(icon_button(
            "editor-reset",
            "icons/trash-2.svg",
            if reset_armed {
                Msg::EditorResetAllConfirm.t()
            } else {
                Msg::EditorReset.t()
            },
            if reset_armed {
                theme().color_blocker
            } else {
                theme().color_warning
            },
            reset,
            cx,
        ))
        .into_any_element()
}

fn tool_button<H>(id: &str, label: &str, accent: u32, handler: H, cx: &gpui::App) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    outline_if_neutral(
        KagiButton::accent(
            SharedString::from(id.to_string()),
            SharedString::from(label.to_string()),
            accent,
            cx,
        ),
        accent,
        cx,
    )
    .small()
    .on_click(handler)
}

/// `apply_accent` maps a neutral accent to the *ghost* variant: no border, no
/// fill, so prev/next/external were indistinguishable from the toolbar
/// background (user report). `.outline()` draws a border too close to the
/// surface to help, so neutral buttons get the same tinted chip the accent
/// actions use — a visible fill, in a neutral hue.
fn outline_if_neutral(btn: Button, accent: u32, cx: &gpui::App) -> Button {
    if accent == theme().text_sub || accent == theme().text_muted {
        btn.custom(super::button_style::tinted_action_variant(accent, cx))
    } else {
        btn
    }
}

/// An icon button with a compact text label beside the glyph (POLISH-040/041).
fn icon_button<H>(
    id: &str,
    icon_path: &'static str,
    label: &str,
    accent: u32,
    handler: H,
    cx: &gpui::App,
) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    outline_if_neutral(
        KagiButton::accent_icon(
            SharedString::from(id.to_string()),
            icon_path,
            SharedString::from(label.to_string()),
            accent,
            cx,
        ),
        accent,
        cx,
    )
    .small()
    .on_click(handler)
}

// ────────────────────────────────────────────────────────────
// 3-pane body: synchronized A/B row lists + Result pane
// ────────────────────────────────────────────────────────────

fn render_panes(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    // No hunk model (binary / single-sided) → guidance message.
    let Some(_inputs) = chrome.inputs.as_ref().filter(|i| i.path == path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };
    let Some(model) = mode.buffer.hunk_model(path) else {
        return guidance_pane(Msg::EditorNoTextMerge.t());
    };

    let labels = mode.labels();
    let current_label = format!("{} — {}", Msg::EditorCurrentSide.t(), labels.current.name);
    let incoming_label = format!("{} — {}", Msg::EditorIncomingSide.t(), labels.incoming.name);

    // ── A | B row (resizable A|B), measured for the vertical divider drag ──
    let ab_geom = chrome.ab_geom.clone();
    let ab_measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                eprintln!(
                    "[kagi] ab_geom left={:.1} right={:.1} width={:.1}",
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.x + bounds.size.width),
                    f32::from(bounds.size.width)
                );
            }
            ab_geom.set((
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.x + bounds.size.width),
            ));
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
        Some(side_use_all_pill(
            path,
            model.file_side_state(SelectionSide::Current),
            SelectionSide::Current,
            cx,
        )),
        side_row_list(
            path,
            model,
            SelectionSide::Current,
            scroll.clone(),
            chrome.selected_hunk,
            chrome,
            cx,
        ),
    );

    let b_pane = pane(
        "conflict-pane-b",
        incoming_label,
        theme().color_remote,
        Some(side_use_all_pill(
            path,
            model.file_side_state(SelectionSide::Incoming),
            SelectionSide::Incoming,
            cx,
        )),
        side_row_list(
            path,
            model,
            SelectionSide::Incoming,
            scroll,
            chrome.selected_hunk,
            chrome,
            cx,
        ),
    );

    let ab_row = div()
        .relative()
        .flex()
        .flex_row()
        .w_full()
        .flex_basis(relative(chrome.result_split))
        .flex_shrink(1.)
        .min_h(theme::scaled_px(80.))
        // Measure the FULL A·B row width (not just the A pane) so the divider
        // drag maps the cursor against the whole span — measuring inside A would
        // shrink the span and feed back on itself, making resize unusable.
        .child(ab_measure)
        .child(
            div()
                .h_full()
                .min_w(px(0.))
                .w(relative(chrome.ab_split))
                .child(a_pane),
        )
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
        .flex_shrink(1.)
        .child(render_result_pane(mode, chrome, path, cx));

    // The split region is measured for the horizontal divider drag.
    let geom = chrome.geom.clone();
    let measure = canvas(
        move |bounds: Bounds<Pixels>, _w, _cx| {
            geom.set((
                f32::from(bounds.origin.y),
                f32::from(bounds.origin.y + bounds.size.height),
            ));
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

/// Per-row syntax-highlight spans for one side's row list, aligned by row
/// index to the `Vec<SideRow>` build order (ADR-0135).
pub type RowHl = Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>;

/// Cache for both sides' highlights. Row text is fixed for a given file within
/// a conflict session (selection only flips `taken` flags), so the key is the
/// path plus the theme slug (a theme switch re-colours the spans).
pub struct SideHlCache {
    key: (std::path::PathBuf, &'static str),
    current: Arc<Vec<RowHl>>,
    incoming: Arc<Vec<RowHl>>,
}

/// Fetch (or lazily build) the highlight spans for `side`. Returns an empty
/// per-row vec when the language is unknown — rows then render as plain text.
fn side_highlights(
    chrome: &EditorChrome,
    path: &std::path::Path,
    model: &kagi_git::resolution::HunkModel,
    side: SelectionSide,
) -> Arc<Vec<RowHl>> {
    let key = (path.to_path_buf(), theme().slug);
    {
        let cache = chrome.hl_cache.borrow();
        if let Some(c) = cache.as_ref() {
            if c.key == key {
                return match side {
                    SelectionSide::Current => c.current.clone(),
                    SelectionSide::Incoming => c.incoming.clone(),
                };
            }
        }
    }
    let current = Arc::new(highlight_side_rows(
        &build_side_rows(model, SelectionSide::Current),
        path,
    ));
    let incoming = Arc::new(highlight_side_rows(
        &build_side_rows(model, SelectionSide::Incoming),
        path,
    ));
    let out = match side {
        SelectionSide::Current => current.clone(),
        SelectionSide::Incoming => incoming.clone(),
    };
    *chrome.hl_cache.borrow_mut() = Some(SideHlCache {
        key,
        current,
        incoming,
    });
    out
}

/// Tree-sitter highlight for a side's rows, mirroring `diff_view`'s proven
/// combine → parse once → distribute-spans-per-row approach (no sigil offset
/// here: ranges are row-local from byte 0).
fn highlight_side_rows(rows: &[SideRow], path: &std::path::Path) -> Vec<RowHl> {
    let texts: Vec<(usize, &str)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| match row {
            SideRow::Line { text, .. } => Some((i, text.as_str())),
            SideRow::Context { text, .. } => Some((i, text.as_str())),
            _ => None,
        })
        .collect();
    highlight_rows(rows.len(), &texts, path)
}

/// Tree-sitter highlight for `row_count` rows, of which `texts` names the ones
/// that carry code (row index → line text). Shared by the A/B panes and the
/// Result preview.
fn highlight_rows(row_count: usize, texts: &[(usize, &str)], path: &std::path::Path) -> Vec<RowHl> {
    use gpui_component::highlighter::SyntaxHighlighter;
    use gpui_component::Rope;

    let mut out: Vec<RowHl> = vec![Vec::new(); row_count];
    let Some(lang) = super::diff_view::lang_for_path(path) else {
        return out;
    };

    let mut line_offsets: Vec<(usize, usize)> = Vec::new(); // (row_index, byte_start)
    let mut combined = String::new();
    for (i, text) in texts {
        line_offsets.push((*i, combined.len()));
        combined.push_str(text);
        combined.push('\n');
    }
    if combined.is_empty() {
        return out;
    }

    let mut highlighter = SyntaxHighlighter::new(lang);
    let rope = Rope::from_str(&combined);
    highlighter.update(None, &rope, None);
    let hl_theme = theme::highlight_theme(theme::theme());
    let mut all_styles = highlighter.styles(&(0..combined.len()), &hl_theme);

    // Shared with the diff pane — see `distribute_highlights` for why the
    // obvious nested loop is not usable here. No sigil on these rows, so the
    // local offset is 0.
    super::diff_view::distribute_highlights(
        &mut all_styles,
        &line_offsets,
        combined.len(),
        0,
        |row_i, row_highlights| out[row_i] = row_highlights,
    );
    out
}

#[derive(Clone)]
enum SideRow {
    HunkHeader {
        hunk_index: usize,
        state: TriState,
        order: LineOrder,
        /// Whether this hunk has a decision yet — drives the red/green marker
        /// and the "still undecided" red band.
        resolved: bool,
    },
    Line {
        hunk_index: usize,
        line_index: usize,
        line_no: usize,
        text: String,
        taken: bool,
        /// Whether this line's hunk has a decision yet (red vs green marker,
        /// and whether an untaken line wears the undecided red band).
        resolved: bool,
        /// Whether to fade this line: it was NOT taken and the other side WAS.
        dim: bool,
        /// Byte ranges in `text` that differ from the same-index line on the
        /// other side of this hunk — the "where do the two sides disagree"
        /// highlight (word granularity, shared with the diff pane).
        diff_spans: Arc<Vec<Range<usize>>>,
    },
    /// A non-conflicting context line (a passthrough region) shown identically on
    /// both panes. Rendering these makes each pane show the full ours/theirs file
    /// — so the Merged Result Preview never contains lines the user couldn't see
    /// in the editor — and keeps the two panes aligned at shared context.
    Context { line_no: usize, text: String },
    /// Filler row so a hunk occupies the same number of rows on both sides when
    /// Current and Incoming have different line counts. Keeps the A and B panes
    /// row-aligned and gives them identical total heights so the shared scroll
    /// handle is not clamped to the shorter side.
    Blank { hunk_index: usize },
}

/// Pane-header pill that takes/releases the whole file for one side —
/// replaces the old file-level tri-checkbox (ADR-0135).
fn side_use_all_pill(
    path: &std::path::Path,
    state: TriState,
    side: SelectionSide,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let p = path.to_path_buf();
    let next = state != TriState::All;
    let handler = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_set_file_side(&p, side, next);
        cx.notify();
    });
    let accent = side_accent(side);
    let on = state == TriState::All;
    div()
        .id(SharedString::from(format!("file-side-{:?}", side)))
        .px(theme::scaled_px(6.))
        .py(theme::scaled_px(1.))
        .rounded_sm()
        .border_1()
        .border_color(rgb(if on { accent } else { theme().text_muted }))
        .bg(rgb(if on {
            theme().bg_base
        } else {
            theme().bg_row_alt
        }))
        .text_size(theme::scaled_px(9.))
        .text_color(rgb(if on { accent } else { theme().text_main }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().bg_row_alt)))
        .child(SharedString::from(Msg::EditorUseAll.t()))
        .on_click(handler)
}

/// Byte ranges in `text` that differ from `other`, the same-index line on the
/// opposite side of the hunk. Index pairing IS the alignment model here: a line
/// with no counterpart pairs with `""` and so highlights whole. Identical lines
/// produce no spans, which is the useful half of the signal — the rows that
/// carry no highlight are the ones both sides agree on.
fn intra_line_spans(text: &str, other: &str, side: SelectionSide) -> Vec<Range<usize>> {
    use kagi_domain::word_diff::{word_diff, Side};
    let (old, new, want) = match side {
        SelectionSide::Current => (text, other, Side::Old),
        SelectionSide::Incoming => (other, text, Side::New),
    };
    word_diff(old, new)
        .into_iter()
        .filter(|span| span.side == want)
        .map(|span| span.range)
        .collect()
}

fn build_side_rows(model: &kagi_git::resolution::HunkModel, side: SelectionSide) -> Vec<SideRow> {
    let mut rows = Vec::new();
    let mut hunk_index = 0usize;
    let mut line_no = 1usize;
    for region in &model.regions {
        if let Region::Passthrough(lines) = region {
            // Context lines belong to both sides identically; show them so the
            // pane reflects the whole file and the gutter numbers are the real
            // ours/theirs line numbers (not hunk-only counts).
            for text in lines {
                rows.push(SideRow::Context {
                    line_no,
                    text: text.clone(),
                });
                line_no += 1;
            }
        }
        if let Region::Hunk(hunk) = region {
            let order = hunk
                .line_select
                .as_ref()
                .map(|selection| selection.order)
                .unwrap_or_else(|| match hunk.choice {
                    kagi_git::resolution::HunkChoice::BothIncomingFirst => LineOrder::IncomingFirst,
                    _ => LineOrder::CurrentFirst,
                });
            let resolved = hunk.is_resolved();
            rows.push(SideRow::HunkHeader {
                hunk_index,
                state: hunk.side_state(side),
                order,
                resolved,
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
            let other = match side {
                SelectionSide::Current => &hunk.incoming,
                SelectionSide::Incoming => &hunk.current,
            };
            // Fading an untaken line only makes sense once the OTHER side has
            // been taken. While a hunk is untouched both sides were untaken, so
            // both faded to 22% — the conflict rows were the *dimmest* rows in
            // the pane, which is what made the conflict hard to find (user
            // report). Untouched hunks now render at full strength.
            let other_state = hunk.side_state(match side {
                SelectionSide::Current => SelectionSide::Incoming,
                SelectionSide::Incoming => SelectionSide::Current,
            });
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
                    resolved,
                    dim: !is_taken && other_state != TriState::None,
                    diff_spans: Arc::new(intra_line_spans(
                        text,
                        other.get(line_index).map(String::as_str).unwrap_or(""),
                        side,
                    )),
                });
                line_no += 1;
            }
            // Pad the shorter side so this hunk occupies max(current, incoming)
            // line rows on both panes — keeps the two sides row-aligned and gives
            // them equal total height (so the shared scroll handle isn't clamped
            // to the shorter side).
            let max_lines = hunk.current.len().max(hunk.incoming.len());
            for _ in lines.len()..max_lines {
                rows.push(SideRow::Blank { hunk_index });
            }
            hunk_index += 1;
        }
    }
    rows
}

fn side_row_list(
    path: &std::path::Path,
    model: &kagi_git::resolution::HunkModel,
    side: SelectionSide,
    scroll: UniformListScrollHandle,
    selected_hunk: usize,
    chrome: &EditorChrome,
    cx: &mut Context<ConflictView>,
) -> gpui::Stateful<gpui::Div> {
    let rows = Arc::new(build_side_rows(model, side));
    let highlights = side_highlights(chrome, path, model, side);
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
                    render_side_rows(
                        &rows_for_list,
                        &highlights,
                        p.clone(),
                        side,
                        selected_hunk,
                        range,
                        cx,
                    )
                }),
            )
            .track_scroll(&scroll)
            .flex_1()
            .min_h(px(0.)),
        )
        .child(Scrollbar::vertical(&scroll))
}

fn render_side_rows(
    rows: &[SideRow],
    highlights: &[RowHl],
    path: Arc<std::path::PathBuf>,
    side: SelectionSide,
    selected_hunk: usize,
    range: std::ops::Range<usize>,
    cx: &mut Context<ConflictView>,
) -> Vec<AnyElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row.clone())))
        .map(|(i, row)| match row {
            SideRow::HunkHeader {
                hunk_index,
                state,
                order,
                resolved,
            } => render_hunk_header_row(
                i,
                path.clone(),
                hunk_index,
                state,
                order,
                resolved,
                side,
                selected_hunk,
                cx,
            ),
            SideRow::Line {
                hunk_index,
                line_index,
                line_no,
                text,
                taken,
                resolved,
                dim,
                diff_spans,
            } => render_code_line_row(
                i,
                path.clone(),
                hunk_index,
                line_index,
                line_no,
                text,
                highlights.get(i).cloned().unwrap_or_default(),
                diff_spans,
                taken,
                resolved,
                dim,
                side,
                selected_hunk,
                cx,
            ),
            SideRow::Context { line_no, text } => render_context_row(
                i,
                line_no,
                text,
                highlights.get(i).cloned().unwrap_or_default(),
            ),
            SideRow::Blank { hunk_index } => render_blank_row(i, hunk_index, selected_hunk),
        })
        .collect()
}

/// A non-conflicting context line: no checkbox, muted text, real gutter number.
/// Shown identically on both panes so the editor mirrors the full file.
fn render_context_row(
    row_index: usize,
    line_no: usize,
    text_value: String,
    highlights: RowHl,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("side-ctx-{}", row_index)))
        .flex()
        .flex_row()
        .items_center()
        .min_w(relative(1.0))
        .h(theme::scaled_px(17.))
        .pr(theme::scaled_px(4.))
        .gap_1()
        // Transparent band keeps context columns aligned with hunk rows.
        .border_l(theme::scaled_px(3.))
        .border_color(rgb(theme().bg_base))
        .bg(rgb(theme().bg_base))
        .child(gutter_marker(None))
        .child(
            div()
                .w(theme::scaled_px(34.))
                .flex_shrink_0()
                .text_size(theme::scaled_px(11.))
                .line_height(theme::scaled_px(17.))
                .font_family(terminal::pick_font_family())
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{:>4}", line_no))),
        )
        .child(code_text(
            text_value,
            highlights,
            theme().text_sub,
            &[],
            gpui::transparent_black(),
        ))
        .into_any_element()
}

fn render_blank_row(row_index: usize, hunk_index: usize, selected_hunk: usize) -> AnyElement {
    div()
        .id(SharedString::from(format!("side-blank-{}", row_index)))
        .w_full()
        .h(theme::scaled_px(17.))
        // Transparent band so filler rows align with the banded hunk rows.
        .border_l(theme::scaled_px(3.))
        .border_color(rgb(theme().bg_base))
        .bg(rgb(if selected_hunk == hunk_index {
            theme().bg_row_alt
        } else {
            theme().bg_base
        }))
        .into_any_element()
}

// Row renderers take the row's full identity + selection context by value; a
// params struct would only move the same fields behind one more indirection.
#[allow(clippy::too_many_arguments)]
fn render_hunk_header_row(
    row_index: usize,
    path: Arc<std::path::PathBuf>,
    hunk_index: usize,
    state: TriState,
    order: LineOrder,
    resolved: bool,
    side: SelectionSide,
    selected_hunk: usize,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    // ADR-0135 (Xcode-style): clicking the header takes/releases this whole
    // side of the hunk — the header IS the control, no checkbox.
    let next = state != TriState::All;
    let p = path.clone();
    let take = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_select_hunk(hunk_index);
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
    let order_label = match order {
        LineOrder::CurrentFirst => Msg::EditorCurrentFirst.t(),
        LineOrder::IncomingFirst => Msg::EditorIncomingFirst.t(),
    };
    let accent = side_accent(side);
    // Band opacity mirrors the tri-state: solid / half / off.
    let band = match state {
        TriState::All => rgb(accent),
        TriState::Partial => rgb(accent),
        TriState::None if resolved => rgb(theme().selected),
        TriState::None => rgb(theme().color_blocker),
    };
    div()
        .id(SharedString::from(format!("side-hunk-{}", row_index)))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .h(theme::scaled_px(22.))
        .pr(theme::scaled_px(4.))
        .border_l(theme::scaled_px(3.))
        .border_color(band)
        .map(|el| {
            let focused = selected_hunk == hunk_index;
            if state != TriState::None {
                el.bg(side_tint(side, if focused { 0.28 } else { 0.18 }))
            } else if focused {
                el.bg(rgb(theme().bg_row_alt))
            } else {
                el.bg(rgb(theme().bg_base))
            }
        })
        .cursor_pointer()
        .hover(|s| s.bg(side_tint(side, 0.35)))
        .on_click(take)
        .child(
            div()
                .pl(theme::scaled_px(6.))
                .text_size(theme::scaled_px(10.))
                .text_color(rgb(if state == TriState::None {
                    theme().text_muted
                } else {
                    accent
                }))
                .child(SharedString::from(format!(
                    "{} {}",
                    Msg::EditorHunkLabel.t(),
                    hunk_index + 1
                ))),
        )
        .when(state == TriState::Partial, |el| {
            el.child(
                div()
                    .text_size(theme::scaled_px(9.))
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("±")),
            )
        })
        .child(
            div()
                .id(SharedString::from(format!("hunk-order-{}", row_index)))
                .ml_auto()
                .px(theme::scaled_px(6.))
                .py(theme::scaled_px(1.))
                .rounded_sm()
                .border_1()
                .border_color(rgb(theme().text_muted))
                .bg(rgb(theme().bg_base))
                .text_size(theme::scaled_px(9.))
                .text_color(rgb(theme().text_main))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().bg_row_alt)))
                .child(SharedString::from(order_label))
                .on_click(order_click),
        )
        .into_any_element()
}

/// The gutter marker at the head of every conflicting line, VS Code
/// breakpoint-style (user request). It marks *where the conflict is*, so it
/// never disappears — it only changes colour: red while the hunk is undecided,
/// then the accent of the side that was actually adopted (so the marker answers
/// "which side is in the Result?", not just "done"). `None` renders the cell
/// empty, which keeps the code columns aligned across panes.
fn gutter_marker(color: Option<u32>) -> gpui::Div {
    div()
        .flex_shrink_0()
        .w(theme::scaled_px(16.))
        .h(theme::scaled_px(17.))
        .flex()
        .items_center()
        .justify_center()
        .when_some(color, |el, color| {
            el.child(
                div()
                    .w(theme::scaled_px(9.))
                    .h(theme::scaled_px(9.))
                    .rounded_full()
                    .bg(rgb(color)),
            )
        })
}

/// Marker colour for one A/B code line: red while its hunk is undecided, the
/// side's own accent once this line is part of the Result, and the neutral
/// band colour for the side that lost.
fn line_marker_color(resolved: bool, taken: bool, side: SelectionSide) -> u32 {
    if !resolved {
        theme().color_blocker
    } else if taken {
        side_accent(side)
    } else {
        theme().selected
    }
}

/// The per-side accent colour (Current = branch, Incoming = remote).
fn side_accent(side: SelectionSide) -> u32 {
    match side {
        SelectionSide::Current => theme().color_branch,
        SelectionSide::Incoming => theme().color_remote,
    }
}

/// The side accent as a translucent row background — the diff-view idiom
/// (coloured code rows), so conflict regions read as *colour*, not just
/// bright-vs-dim (user report).
fn side_tint(side: SelectionSide, alpha: f32) -> gpui::Hsla {
    let mut c: gpui::Hsla = rgb(side_accent(side)).into();
    c.a = alpha;
    c
}

#[allow(clippy::too_many_arguments)]
fn render_code_line_row(
    row_index: usize,
    path: Arc<std::path::PathBuf>,
    hunk_index: usize,
    line_index: usize,
    line_no: usize,
    text_value: String,
    highlights: RowHl,
    diff_spans: Arc<Vec<Range<usize>>>,
    taken: bool,
    resolved: bool,
    dim: bool,
    side: SelectionSide,
    selected_hunk: usize,
    cx: &mut Context<ConflictView>,
) -> AnyElement {
    // ADR-0135: the line itself is the control — clicking it toggles just this
    // line in/out of the taken set (the old line checkbox, without the glyph).
    let p = path.clone();
    let toggle = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_select_hunk(hunk_index);
        this.conflict_editor_set_hunk_line(&p, hunk_index, side, line_index, !taken);
        cx.notify();
    });
    let accent = side_accent(side);
    div()
        .id(SharedString::from(format!("side-line-{}", row_index)))
        .flex()
        .flex_row()
        .items_center()
        .min_w(relative(1.0))
        .h(theme::scaled_px(17.))
        .pr(theme::scaled_px(4.))
        .gap_1()
        .border_l(theme::scaled_px(3.))
        // An untaken line in an UNDECIDED hunk wears the red "still yours to
        // decide" band; once the hunk has a decision the rejected side drops
        // back to the neutral band (user request).
        .border_color(if taken {
            rgb(accent)
        } else if resolved {
            rgb(theme().selected)
        } else {
            rgb(theme().color_blocker)
        })
        .bg(rgb(if selected_hunk == hunk_index {
            theme().bg_row_alt
        } else {
            theme().bg_base
        }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().selected)))
        .on_click(toggle)
        // Untaken lines keep their syntax colours but fade — Xcode-style
        // "the other side is dimmed", per line. 0.22: at 0.4 the rejected side
        // still read almost as clearly as the taken one (user report). The fade
        // is on the code, NOT the row: the pin must survive it, or the rejected
        // side stops showing where the conflict was.
        .child(gutter_marker(Some(line_marker_color(
            resolved, taken, side,
        ))))
        .child(
            div()
                .when(dim, |el| el.opacity(0.22))
                .w(theme::scaled_px(34.))
                .flex_shrink_0()
                .text_size(theme::scaled_px(11.))
                .line_height(theme::scaled_px(17.))
                .font_family(terminal::pick_font_family())
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(format!("{:>4}", line_no))),
        )
        .child(
            code_text(
                text_value,
                highlights,
                theme().text_main,
                &diff_spans,
                side_tint(side, 0.30),
            )
            .when(dim, |el| el.opacity(0.22)),
        )
        .into_any_element()
}

/// Monospace code text with validated syntax-highlight spans (the same
/// out-of-bounds guard as the diff renderers).
fn code_text(
    text_value: String,
    highlights: RowHl,
    base_color: u32,
    diff_spans: &[Range<usize>],
    emph_bg: gpui::Hsla,
) -> gpui::Div {
    let shared = SharedString::from(text_value);
    let el = div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .text_size(theme::scaled_px(12.))
        .line_height(theme::scaled_px(17.))
        .font_family(terminal::pick_font_family())
        .text_color(rgb(base_color));
    if highlights.is_empty() && diff_spans.is_empty() {
        return el.child(shared);
    }
    let text_str: &str = shared.as_ref();
    let text_len = text_str.len();
    let valid: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = highlights
        .into_iter()
        .filter(|(r, _)| {
            r.start <= r.end
                && r.end <= text_len
                && text_str.is_char_boundary(r.start)
                && text_str.is_char_boundary(r.end)
        })
        .collect();
    // The word-diff emphasis merges with the syntax runs exactly as in the diff
    // pane (same helper, same fill + opaque underline), so a conflict row marks
    // the changed words instead of only the whole side.
    let merged = super::diff_split::merge_highlights(text_len, &valid, diff_spans, emph_bg);
    el.child(gpui::StyledText::new(shared.clone()).with_highlights(merged))
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
                .flex_grow(1.)
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
                .flex_grow(1.)
                .w_full()
                .min_h(px(0.))
                .child(editor),
        )
}

// ────────────────────────────────────────────────────────────
// Result pane: Preview (read-only) / Edit (editable) — UX-015
// ────────────────────────────────────────────────────────────

/// Cache for the Result preview's highlight spans. Unlike the A/B rows the
/// assembled text changes with every selection, so the key IS the text: the
/// cheap `String` compare replaces a tree-sitter parse on every frame, and a
/// real selection change re-parses exactly once.
pub struct ResultHlCache {
    key: (std::path::PathBuf, &'static str, String),
    rows: Arc<Vec<RowHl>>,
}

fn result_highlights(
    chrome: &EditorChrome,
    path: &std::path::Path,
    lines: &[kagi_git::resolution::ResolvedLine],
) -> Arc<Vec<RowHl>> {
    let text: String = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let key = (path.to_path_buf(), theme().slug, text);
    {
        let cache = chrome.result_hl.borrow();
        if let Some(c) = cache.as_ref() {
            if c.key == key {
                return c.rows.clone();
            }
        }
    }
    let texts: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i, l.text.as_str()))
        .collect();
    let rows = Arc::new(highlight_rows(lines.len(), &texts, path));
    *chrome.result_hl.borrow_mut() = Some(ResultHlCache {
        key,
        rows: rows.clone(),
    });
    rows
}

/// The provenance badge at the head of a Result line: the adopted side's own
/// accent, filled for Current and a ring for Incoming, so the two read apart by
/// shape as well as hue (#354). Context and hand-edited lines carry no badge.
fn origin_badge(origin: LineOrigin) -> gpui::Div {
    let cell = div()
        .flex_shrink_0()
        .w(theme::scaled_px(16.))
        .h(theme::scaled_px(17.))
        .flex()
        .items_center()
        .justify_center();
    let dot = |color: u32, filled: bool| {
        div()
            .w(theme::scaled_px(9.))
            .h(theme::scaled_px(9.))
            .rounded_full()
            .map(|el| {
                if filled {
                    el.bg(rgb(color))
                } else {
                    el.border_2().border_color(rgb(color))
                }
            })
    };
    match origin {
        LineOrigin::Current => cell.child(dot(side_accent(SelectionSide::Current), true)),
        LineOrigin::Incoming => cell.child(dot(side_accent(SelectionSide::Incoming), false)),
        LineOrigin::Manual | LineOrigin::Context => cell,
    }
}

/// The read-only Result preview: the assembled output, one row per line, each
/// tagged with the side it came from (user request — "show me whether Current or
/// Incoming was adopted, at the head of the line"). Edit mode still uses the
/// CodeEditor; this list is what Preview shows instead.
fn render_result_preview(
    model: &kagi_git::resolution::HunkModel,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
) -> gpui::AnyElement {
    let lines = Arc::new(model.assemble());
    let highlights = result_highlights(chrome, path, &lines);
    let scroll = chrome.result_scroll.clone();
    let row_count = lines.len();
    div()
        .id("conflict-result-rows")
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .overflow_x_scroll()
        .child(
            uniform_list(
                "conflict-result-lines",
                row_count,
                cx.processor(move |_this, range: std::ops::Range<usize>, _window, _cx| {
                    range
                        .filter_map(|i| lines.get(i).map(|line| (i, line)))
                        .map(|(i, line)| {
                            div()
                                .id(SharedString::from(format!("result-line-{}", i)))
                                .flex()
                                .flex_row()
                                .items_center()
                                .min_w(relative(1.0))
                                .h(theme::scaled_px(17.))
                                .pr(theme::scaled_px(4.))
                                .gap_1()
                                .bg(rgb(theme().bg_base))
                                .child(origin_badge(line.origin))
                                .child(
                                    div()
                                        .w(theme::scaled_px(34.))
                                        .flex_shrink_0()
                                        .text_size(theme::scaled_px(11.))
                                        .line_height(theme::scaled_px(17.))
                                        .font_family(terminal::pick_font_family())
                                        .text_color(rgb(theme().text_muted))
                                        .child(SharedString::from(format!("{:>4}", i + 1))),
                                )
                                .child(code_text(
                                    line.text.clone(),
                                    highlights.get(i).cloned().unwrap_or_default(),
                                    theme().text_main,
                                    &[],
                                    gpui::transparent_black(),
                                ))
                                .into_any_element()
                        })
                        .collect()
                }),
            )
            .track_scroll(&scroll)
            .flex_1()
            .min_h(px(0.)),
        )
        .child(Scrollbar::vertical(&scroll))
        .into_any_element()
}

fn render_result_pane(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    path: &std::path::Path,
    cx: &mut Context<ConflictView>,
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
    let status_color = if all_resolved {
        theme().color_success
    } else {
        theme().color_warning
    };

    let toggle = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_editor_toggle_result_mode();
        cx.notify();
    });
    // File-level "Save resolution" lives near the Result (deliverable #4).
    // Save writes the working tree + stages + re-detects (reload) → it touches
    // `app.conflict`, so it MUST defer to the parent (calling it synchronously
    // here would re-lease this leased entity and panic).
    let p_save = path.to_path_buf();
    let save = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
            let weak_app = view.app.clone();
            let intent = view.freeze_save_intent(&p_save);
            cx.spawn_in(window, async move |_view, acx| {
                let _ = weak_app.update_in(acx, |app, _window, cx| {
                    app.accept_conflict_intent(intent, cx)
                });
            })
            .detach();
        },
    );

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
                .flex_grow(1.)
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
        .child(super::e2e::measure_control(
            "conflict-save",
            tool_button(
                "editor-save",
                Msg::EditorSave.t(),
                theme().color_success,
                save,
                cx,
            )
            .disabled(chrome.writer_busy),
        ));

    // Body: ONE CodeEditor for both modes — Preview is the same component
    // with `disabled(true)` (user request: the two modes previously used
    // different renderers and their font/size drifted). The CodeEditor
    // highlights via the ADR-0133 per-theme pipeline; disabled only skips the
    // interaction handlers and keeps the syntax colours.
    // Preview renders our own rows so each line can carry its provenance badge;
    // Edit stays the CodeEditor (it has to be a real text buffer).
    let preview_body: gpui::AnyElement = if editing {
        div()
            .flex_grow(1.)
            .w_full()
            .min_h(px(0.))
            // Font via the wrapper text-style cascade (Snapshot-pane pattern, #219).
            .font_family(terminal::pick_font_family())
            .child(
                Input::new(&inputs.result)
                    .disabled(chrome.writer_busy)
                    .appearance(false)
                    .bordered(false)
                    .h_full(),
            )
            .into_any_element()
    } else if let Some(model) = model {
        render_result_preview(model, chrome, path, cx)
    } else {
        div().flex_grow(1.).into_any_element()
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
        let accent = if active {
            theme().color_branch
        } else {
            theme().text_sub
        };
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
        .on_drag(
            DividerDrag {
                kind: DividerKind::ConflictAB,
            },
            |_, _, _, cx| cx.new(|_| DividerGhost),
        )
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
        .on_drag(
            DividerDrag {
                kind: DividerKind::ConflictResult,
            },
            |_, _, _, cx| cx.new(|_| DividerGhost),
        )
}

#[cfg(test)]
mod side_row_tests {
    use super::*;
    use kagi_git::resolution::HunkModel;

    fn counts(rows: &[SideRow]) -> (usize, usize, usize, usize) {
        let mut ctx = 0;
        let mut blank = 0;
        let mut line = 0;
        for r in rows {
            match r {
                SideRow::Context { .. } => ctx += 1,
                SideRow::Blank { .. } => blank += 1,
                SideRow::Line { .. } => line += 1,
                SideRow::HunkHeader { .. } => {}
            }
        }
        (rows.len(), ctx, blank, line)
    }

    fn spans(rows: &[SideRow]) -> Vec<Vec<Range<usize>>> {
        rows.iter()
            .filter_map(|r| match r {
                SideRow::Line { diff_spans, .. } => Some(diff_spans.as_ref().clone()),
                _ => None,
            })
            .collect()
    }

    /// The conflict rows must mark WHERE the two sides disagree: a line that is
    /// identical on both sides carries no span, and a changed line spans only
    /// the changed word (not the whole line).
    #[test]
    fn diff_spans_mark_only_the_words_that_differ_between_the_sides() {
        let markers =
            "ctx\n<<<<<<< Current\nsame\nlet a = 1;\n=======\nsame\nlet a = 2;\n>>>>>>> Incoming\n";
        let model = HunkModel::from_marker_text(markers);

        let cur = spans(&build_side_rows(&model, SelectionSide::Current));
        let inc = spans(&build_side_rows(&model, SelectionSide::Incoming));

        assert_eq!(cur.len(), 2);
        assert!(cur[0].is_empty(), "identical line is not highlighted");
        assert!(inc[0].is_empty(), "identical line is not highlighted");
        // "let a = 1;" → only the "1" differs.
        assert_eq!(cur[1], vec![8..9]);
        assert_eq!(inc[1], vec![8..9]);
    }

    /// A line with no counterpart on the other side differs wholesale.
    #[test]
    fn unpaired_line_is_highlighted_whole() {
        let markers = "<<<<<<< Current\nonly\n=======\n>>>>>>> Incoming\n";
        let model = HunkModel::from_marker_text(markers);
        let cur = spans(&build_side_rows(&model, SelectionSide::Current));
        assert_eq!(cur, vec![vec![0..4]]);
    }

    /// Bug 1 (scroll clamp) + Bug 2 (preview/editor consistency): both panes must
    /// have an identical row count (so the shared scroll handle is not clamped to
    /// the shorter side), passthrough context must be shown on both sides, and the
    /// shorter side of a length-mismatched hunk must be blank-padded.
    #[test]
    fn side_panes_are_equal_length_and_show_context() {
        // 1 leading context line; a hunk with 3 Current vs 1 Incoming lines;
        // 2 trailing context lines.
        let markers = "ctxA\n\
            <<<<<<< Current\nC1\nC2\nC3\n=======\nI1\n>>>>>>> Incoming\n\
            ctxB\nctxC\n";
        let model = HunkModel::from_marker_text(markers);

        let cur = build_side_rows(&model, SelectionSide::Current);
        let inc = build_side_rows(&model, SelectionSide::Incoming);

        // Equal total length → shared scroll handle covers the full content.
        assert_eq!(
            cur.len(),
            inc.len(),
            "A and B panes must have equal row counts (cur={:?} inc={:?})",
            counts(&cur),
            counts(&inc),
        );

        let (_c_total, c_ctx, c_blank, c_line) = counts(&cur);
        let (_i_total, i_ctx, i_blank, i_line) = counts(&inc);

        // Context (3 passthrough lines) is shown identically on both sides.
        assert_eq!(c_ctx, 3, "current pane shows all context lines");
        assert_eq!(i_ctx, 3, "incoming pane shows all context lines");

        // Hunk has 3 Current vs 1 Incoming code lines.
        assert_eq!(c_line, 3);
        assert_eq!(i_line, 1);
        // The shorter (Incoming) side is padded with max(3,1)-1 = 2 blanks; the
        // longer (Current) side needs none.
        assert_eq!(c_blank, 0, "current side needs no padding");
        assert_eq!(
            i_blank, 2,
            "incoming side padded to the hunk's max line count"
        );
    }
}
