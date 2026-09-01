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

/// Map a unified row index to the split-row index that displays it.
///
/// The list virtualizes over split rows when side-by-side is on, so a scroll
/// target computed against the unified rows lands somewhere else entirely.
/// Only meaningful for rows that survive as a whole row — a hunk header, which
/// is what the conflict jump targets are.
pub(crate) fn split_index_of(rows: &[SplitDiffRow], unified_ix: usize) -> Option<usize> {
    rows.iter().position(|r| match r {
        SplitDiffRow::Full(i) => *i == unified_ix,
        SplitDiffRow::Pair { left, right } => {
            *left == Some(unified_ix) || *right == Some(unified_ix)
        }
    })
}

#[cfg(test)]
mod split_index_tests {
    use super::*;
    use kagi_domain::diff::DiffLineKind;

    fn line(kind: DiffLineKind) -> DiffRow {
        DiffRow::Line {
            kind,
            text: "x".into(),
            old_lineno: None,
            new_lineno: None,
            highlights: Vec::new(),
        }
    }

    /// Pairing shortens the list, so a unified index is not a split index. A
    /// header after several removed/added lines is the case that drifts.
    #[test]
    fn a_header_after_paired_lines_maps_past_the_pairing() {
        let rows = vec![
            DiffRow::HunkHeader("h1".into()), // 0
            line(DiffLineKind::Removed),      // 1
            line(DiffLineKind::Added),        // 2  pairs with 1
            DiffRow::HunkHeader("h2".into()), // 3
        ];
        let split = split_rows(&rows);
        assert_eq!(split_index_of(&split, 0), Some(0));
        // 1 and 2 collapse into one split row, so the second header moves up.
        assert_eq!(split_index_of(&split, 3), Some(2));
        assert_ne!(
            split_index_of(&split, 3),
            Some(3),
            "unified index would miss"
        );
    }
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
fn split_cell(
    rows: &std::sync::Arc<Vec<DiffRow>>,
    idx: Option<usize>,
    side: SplitSide,
    sel_key: u64,
) -> gpui::AnyElement {
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

    // `py_px` lives INSIDE the coloured cell (not on the row container) so the
    // line spacing is painted in the cell's own background — padding on the
    // uncoloured row let the pane background bleed through as a 2px "border"
    // between rows (user report).
    let row_ix = idx.unwrap_or(0);
    let selected = idx.is_some() && crate::ui::diff_selection::contains(sel_key, row_ix);
    div()
        .id(("split-cell", i_id(side, row_ix)))
        .map(|el| {
            if idx.is_some() {
                crate::ui::diff_view::attach_selection_handlers(el, rows, row_ix, sel_key)
            } else {
                el
            }
        })
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_row()
        .items_start()
        .py_px()
        .map(|el| {
            if selected {
                el.bg(theme::selection_overlay())
            } else {
                el.bg(rgb(bg))
            }
        })
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

/// Stable element-id disambiguator for split cells (old/new share row idx).
fn i_id(side: SplitSide, ix: usize) -> usize {
    match side {
        SplitSide::Old => ix * 2,
        SplitSide::New => ix * 2 + 1,
    }
}

/// Render one side-by-side row (ADR-0124). Full-width rows delegate to the
/// unified renderer; pair rows render two half-width [`split_cell`]s around a
/// hairline divider.
pub(crate) fn render_main_diff_split_row(
    rows: &std::sync::Arc<Vec<DiffRow>>,
    srows: &[SplitDiffRow],
    i: usize,
    sel_key: u64,
) -> gpui::AnyElement {
    match srows.get(i) {
        None => div().into_any(),
        Some(SplitDiffRow::Full(idx)) => render_main_diff_row(rows, *idx, sel_key),
        // No padding and no `items_start` on the row itself: cells stretch to
        // the full row height (flex default), so a wrapped line on one side
        // never leaves an unpainted strip under the shorter cell, and rows sit
        // flush against each other like the unified view.
        Some(SplitDiffRow::Pair { left, right }) => div()
            .id(("main-diff-split", i))
            .w_full()
            .flex()
            .flex_row()
            .text_sm()
            .child(split_cell(rows, *left, SplitSide::Old, sel_key))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(1.))
                    .bg(rgb(theme::theme().surface)),
            )
            .child(split_cell(rows, *right, SplitSide::New, sel_key))
            .into_any(),
    }
}

#[cfg(test)]
mod split_rows_tests {
    use super::{split_rows, SplitDiffRow};
    use crate::ui::diff_view::DiffRow;
    use kagi_git::DiffLineKind;

    fn line(kind: DiffLineKind) -> DiffRow {
        DiffRow::Line {
            kind,
            text: "x".into(),
            old_lineno: None,
            new_lineno: None,
            highlights: Vec::new(),
        }
    }

    /// `Full(i)` -> `("full", i, i)`, `Pair` -> `("pair", left, right)` with
    /// `usize::MAX` for a filler cell, so the index arithmetic is asserted
    /// exactly (`SplitDiffRow` has no `PartialEq`).
    fn shape(rows: &[DiffRow]) -> Vec<(&'static str, usize, usize)> {
        const F: usize = usize::MAX;
        split_rows(rows)
            .into_iter()
            .map(|r| match r {
                SplitDiffRow::Full(i) => ("full", i, i),
                SplitDiffRow::Pair { left, right } => {
                    ("pair", left.unwrap_or(F), right.unwrap_or(F))
                }
            })
            .collect()
    }

    const F: usize = usize::MAX;

    #[test]
    fn all_added_hunk_fills_the_left_column() {
        let rows = vec![
            DiffRow::HunkHeader("@@".into()),
            line(DiffLineKind::Added),
            line(DiffLineKind::Added),
        ];
        assert_eq!(
            shape(&rows),
            vec![("full", 0, 0), ("pair", F, 1), ("pair", F, 2)]
        );
    }

    #[test]
    fn all_removed_hunk_fills_the_right_column() {
        let rows = vec![
            DiffRow::HunkHeader("@@".into()),
            line(DiffLineKind::Removed),
            line(DiffLineKind::Removed),
        ];
        assert_eq!(
            shape(&rows),
            vec![("full", 0, 0), ("pair", 1, F), ("pair", 2, F)]
        );
    }

    /// Uneven replacement: 3 removed against 1 added — the tail of the longer
    /// side pairs against filler, and every index is still offset by `start`.
    #[test]
    fn mixed_hunk_with_uneven_sides() {
        let rows = vec![
            DiffRow::HunkHeader("@@".into()),
            line(DiffLineKind::Context),
            line(DiffLineKind::Removed),
            line(DiffLineKind::Removed),
            line(DiffLineKind::Removed),
            line(DiffLineKind::Added),
            line(DiffLineKind::Context),
        ];
        assert_eq!(
            shape(&rows),
            vec![
                ("full", 0, 0),
                ("pair", 1, 1),
                ("pair", 2, 5),
                ("pair", 3, F),
                ("pair", 4, F),
                ("pair", 6, 6),
            ]
        );
    }

    /// Several hunks: the `start` offset must be re-taken per run, otherwise
    /// the second hunk's pairs point back into the first.
    #[test]
    fn offset_accumulates_across_hunks() {
        let rows = vec![
            DiffRow::HunkHeader("@@ 1 @@".into()),
            line(DiffLineKind::Context),
            line(DiffLineKind::Added),
            DiffRow::HunkHeader("@@ 2 @@".into()),
            line(DiffLineKind::Removed),
            line(DiffLineKind::Added),
            DiffRow::Binary,
            line(DiffLineKind::Context),
        ];
        assert_eq!(
            shape(&rows),
            vec![
                ("full", 0, 0),
                ("pair", 1, 1),
                ("pair", F, 2),
                ("full", 3, 3),
                ("pair", 4, 5),
                ("full", 6, 6),
                ("pair", 7, 7),
            ]
        );
        assert!(split_rows(&[]).is_empty());
    }
}
