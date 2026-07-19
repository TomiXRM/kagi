//! ADR-0124: side-by-side (split) diff rows — pairing + rendering.
//!
//! The unified [`super::diff_view::DiffRow`] list stays the single source of
//! truth (line numbers, syntax highlights); this sibling builds index pairs
//! over it via the pure [`kagi_domain::diff::split_pairs`] and renders the
//! two-column rows. Mode selection (`theme::diff_split`) and the header
//! toggle live in `render_helpers::render_diff_list`.

use gpui::{div, prelude::*, px, rgb, SharedString};

use kagi_git::DiffLineKind;

use super::diff_view::{render_main_diff_row, DiffRow};
use super::theme;

// ──────────────────────────────────────────────────────────────
// ADR-0124: side-by-side (split) diff rows
// ──────────────────────────────────────────────────────────────

/// One visual row of the side-by-side diff: either a full-width row reused
/// from the unified renderer (hunk header / binary placeholder) or a pair of
/// cells indexing into the source [`DiffRow`] slice (ADR-0124).
pub(crate) enum SplitDiffRow {
    /// Render `rows[idx]` full-width (non-content row).
    Full(usize),
    /// Left (old) / right (new) cells; `None` is a filler cell.
    Pair {
        left: Option<usize>,
        right: Option<usize>,
    },
}

/// Build the side-by-side row list from the unified rows (ADR-0124).
///
/// Contiguous `DiffRow::Line` segments are paired by the pure
/// [`kagi_domain::diff::split_pairs`]; every other row renders full-width in
/// place. Indices refer into `rows`, so highlights / line numbers are reused
/// as-is.
pub(crate) fn split_rows(rows: &[DiffRow]) -> Vec<SplitDiffRow> {
    let mut out = Vec::with_capacity(rows.len());
    let mut i = 0usize;
    while i < rows.len() {
        if matches!(rows[i], DiffRow::Line { .. }) {
            let start = i;
            let mut kinds: Vec<DiffLineKind> = Vec::new();
            while let Some(DiffRow::Line { kind, .. }) = rows.get(i) {
                kinds.push(kind.clone());
                i += 1;
            }
            for pair in kagi_domain::diff::split_pairs(&kinds) {
                out.push(SplitDiffRow::Pair {
                    left: pair.left.map(|k| start + k),
                    right: pair.right.map(|k| start + k),
                });
            }
        } else {
            out.push(SplitDiffRow::Full(i));
            i += 1;
        }
    }
    out
}

/// Which column of the split view a cell belongs to (picks the line number
/// and the background for context lines' counterpart side).
#[derive(Clone, Copy)]
enum SplitSide {
    Old,
    New,
}

/// One half-width cell of a split row: line-number column + (highlighted)
/// content, or an empty filler when `idx` is `None`. Mirrors the unified
/// Line-arm styling in [`render_main_diff_row`] (wrap enabled, top-aligned).
fn split_cell(rows: &[DiffRow], idx: Option<usize>, side: SplitSide) -> gpui::AnyElement {
    let Some(DiffRow::Line {
        kind,
        text,
        old_lineno,
        new_lineno,
        highlights,
    }) = idx.and_then(|i| rows.get(i))
    else {
        // Filler cell: keep the lane visible but visually inert.
        return div()
            .flex_1()
            .min_w(px(0.))
            .bg(rgb(theme::theme().surface))
            .into_any();
    };

    let bg = match kind {
        DiffLineKind::Added => theme::theme().diff_added_bg,
        DiffLineKind::Removed => theme::theme().diff_removed_bg,
        DiffLineKind::Context => theme::theme().bg_base,
    };
    let text_color = match kind {
        DiffLineKind::Added => theme::theme().change_added,
        DiffLineKind::Removed => theme::theme().change_deleted,
        DiffLineKind::Context => theme::theme().text_main,
    };
    let lineno = match side {
        SplitSide::Old => *old_lineno,
        SplitSide::New => *new_lineno,
    };
    let lineno_str = match lineno {
        Some(n) => format!("{:5}", n),
        None => "     ".to_string(),
    };

    // Same highlight-span validation as the unified renderer (drop
    // out-of-bounds spans instead of panicking).
    let content_el: gpui::AnyElement = if highlights.is_empty() {
        div()
            .flex_1()
            .min_w(px(0.))
            .text_color(rgb(text_color))
            .child(text.clone())
            .into_any()
    } else {
        let text_str: &str = text.as_ref();
        let text_len = text_str.len();
        let valid_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = highlights
            .iter()
            .filter(|(r, _)| {
                r.start <= r.end
                    && r.end <= text_len
                    && text_str.is_char_boundary(r.start)
                    && text_str.is_char_boundary(r.end)
            })
            .cloned()
            .collect();
        div()
            .flex_1()
            .min_w(px(0.))
            .text_color(rgb(text_color))
            .child(gpui::StyledText::new(text.clone()).with_highlights(valid_highlights))
            .into_any()
    };

    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_row()
        .items_start()
        .bg(rgb(bg))
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(44.))
                .text_color(rgb(theme::theme().text_muted))
                .child(SharedString::from(lineno_str)),
        )
        .child(content_el)
        .into_any()
}

/// Render one side-by-side row (ADR-0124). Full-width rows delegate to the
/// unified renderer; pair rows render two half-width [`split_cell`]s around a
/// hairline divider.
pub(crate) fn render_main_diff_split_row(
    rows: &[DiffRow],
    srows: &[SplitDiffRow],
    i: usize,
) -> gpui::AnyElement {
    match srows.get(i) {
        None => div().into_any(),
        Some(SplitDiffRow::Full(idx)) => render_main_diff_row(rows, *idx),
        Some(SplitDiffRow::Pair { left, right }) => div()
            .id(("main-diff-split", i))
            .w_full()
            .flex()
            .flex_row()
            .items_start()
            .py_px()
            .text_sm()
            .child(split_cell(rows, *left, SplitSide::Old))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(1.))
                    .h_full()
                    .bg(rgb(theme::theme().surface)),
            )
            .child(split_cell(rows, *right, SplitSide::New))
            .into_any(),
    }
}
