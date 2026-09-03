//! ADR-0124: side-by-side (split) diff rows — pairing + rendering.
//!
//! The unified [`super::diff_view::DiffRow`] list stays the single source of
//! truth (line numbers, syntax highlights); this sibling builds index pairs
//! over it via the pure [`kagi_domain::diff::split_pairs`] and renders the
//! two-column rows. Mode selection (`theme::diff_split`) and the header
//! toggle live in `render_helpers::render_diff_list`.

use std::collections::HashSet;
use std::ops::Range;

use gpui::{div, prelude::*, px, rgb, HighlightStyle, Hsla, SharedString};

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

/// Memoized [`moved_rows`] (issue #399). `render_diff_list` runs once per
/// FRAME (scroll included), so move detection must not be recomputed there.
/// Keyed by the same title+row-count identity as
/// [`super::diff_selection::surface_key`]; a tiny FIFO holds the last few
/// diffs so every embedding (main / File History / Editor / PR) shares hits.
/// The lock is uncontended — renders are main-thread only.
static MOVED_CACHE: std::sync::Mutex<Vec<(u64, std::sync::Arc<HashSet<usize>>)>> =
    std::sync::Mutex::new(Vec::new());

pub(crate) fn moved_rows_cached(key: u64, rows: &[DiffRow]) -> std::sync::Arc<HashSet<usize>> {
    let mut cache = MOVED_CACHE.lock().unwrap();
    if let Some((_, set)) = cache.iter().find(|(k, _)| *k == key) {
        return set.clone();
    }
    let set = std::sync::Arc::new(moved_rows(rows));
    if cache.len() >= 8 {
        // ponytail: FIFO of 8 diffs; make it LRU if panes ever thrash.
        cache.remove(0);
    }
    cache.push((key, set.clone()));
    set
}

/// Row indices (into `rows`) that belong to a moved block (issue #349).
///
/// Feeds the pure [`kagi_domain::moves::detect_moves`] the diff's removed and
/// added *content* lines (sigil stripped) and maps the returned block ranges
/// back to `DiffRow` indices. A moved line is one git `--color-moved` would show
/// as relocated rather than a genuine add/delete. One O(rows) walk plus the
/// (budget-capped) greedy match — call through [`moved_rows_cached`] from
/// render paths so it runs once per diff, not once per frame (issue #399).
pub(crate) fn moved_rows(rows: &[DiffRow]) -> HashSet<usize> {
    let mut removed_text: Vec<&str> = Vec::new();
    let mut removed_ix: Vec<usize> = Vec::new();
    let mut added_text: Vec<&str> = Vec::new();
    let mut added_ix: Vec<usize> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if let DiffRow::Line { kind, text, .. } = row {
            let content = strip_sigil(text);
            match kind {
                DiffLineKind::Removed => {
                    removed_text.push(content);
                    removed_ix.push(i);
                }
                DiffLineKind::Added => {
                    added_text.push(content);
                    added_ix.push(i);
                }
                DiffLineKind::Context => {}
            }
        }
    }
    let mut out = HashSet::new();
    for block in kagi_domain::moves::detect_moves(&removed_text, &added_text) {
        for r in block.removed {
            out.insert(removed_ix[r]);
        }
        for a in block.added {
            out.insert(added_ix[a]);
        }
    }
    out
}

/// Strip the leading diff sigil (`+`/`-`/space) that `DiffRow::Line.text`
/// carries, returning the raw content. Sigils are single ASCII bytes, so
/// `&text[1..]` is a valid slice; an empty text (shouldn't happen) is passed
/// through.
fn strip_sigil(text: &str) -> &str {
    text.get(1..).unwrap_or(text)
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
    emphasis: &[Range<usize>],
    moved: bool,
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
    let text_str: &str = text.as_ref();
    let text_len = text_str.len();
    let valid_syntax: Vec<(Range<usize>, HighlightStyle)> = highlights
        .iter()
        .filter(|(r, _)| {
            r.start <= r.end
                && r.end <= text_len
                && text_str.is_char_boundary(r.start)
                && text_str.is_char_boundary(r.end)
        })
        .cloned()
        .collect();

    // #349 word diff: emphasise the changed byte ranges with a stronger
    // background, merged with the syntax runs so the intra-line highlight and
    // syntax colour coexist (StyledText needs sorted, disjoint runs).
    let content_el: gpui::AnyElement = if valid_syntax.is_empty() && emphasis.is_empty() {
        div()
            .flex_1()
            .min_w(px(0.))
            .text_color(rgb(text_color))
            .child(text.clone())
            .into_any()
    } else {
        let emph_bg = word_emphasis_bg(kind);
        let merged = merge_highlights(text_len, &valid_syntax, emphasis, emph_bg);
        div()
            .flex_1()
            .min_w(px(0.))
            .text_color(rgb(text_color))
            .child(gpui::StyledText::new(text.clone()).with_highlights(merged))
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
        // #349 move marker: a solid vertical BAR in the gutter (line-style, not
        // colour) marks a moved block. The signal is the bar's presence and
        // position — colourblind-safe (#354) — so a moved block reads as
        // relocated rather than a giant add/delete. Filler cells keep an inert
        // spacer of the same width so the two columns stay aligned.
        .child(div().flex_shrink_0().w(px(3.)).map(|el| {
            if moved {
                el.bg(rgb(theme::theme().text_main))
            } else {
                el
            }
        }))
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

/// Background colour for a word-diff–emphasised span: the side's change colour
/// at reduced alpha, so the changed word stands out against the line's own
/// add/removed wash while the text stays readable. Context lines get no
/// emphasis (they are never part of a modify pair).
fn word_emphasis_bg(kind: &DiffLineKind) -> Hsla {
    let base = match kind {
        DiffLineKind::Added => theme::theme().change_added,
        DiffLineKind::Removed => theme::theme().change_deleted,
        DiffLineKind::Context => theme::theme().text_muted,
    };
    let mut hsla: Hsla = rgb(base).into();
    // Lighter than before: the emphasis now also carries an opaque underline
    // (see merge_highlights), so the fill can stay subtle and keep the syntax
    // text readable instead of washing it out (#349 review feedback).
    hsla.a = 0.22;
    hsla
}

/// Merge syntax-highlight runs with word-diff emphasis ranges into the sorted,
/// disjoint run list `StyledText::with_highlights` requires. Both inputs are
/// individually sorted and disjoint and lie on char boundaries; the sweep over
/// their combined boundary points therefore yields char-boundary intervals,
/// each tagged with the syntax colour that covers it (if any) plus the emphasis
/// background (if the interval is inside a changed span).
fn merge_highlights(
    text_len: usize,
    syntax: &[(Range<usize>, HighlightStyle)],
    emphasis: &[Range<usize>],
    emph_bg: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    use std::collections::BTreeSet;
    let mut bounds: BTreeSet<usize> = BTreeSet::new();
    for (r, _) in syntax {
        bounds.insert(r.start);
        bounds.insert(r.end);
    }
    for r in emphasis {
        bounds.insert(r.start.min(text_len));
        bounds.insert(r.end.min(text_len));
    }
    let pts: Vec<usize> = bounds.into_iter().filter(|&b| b <= text_len).collect();
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b {
            continue;
        }
        let mut style = HighlightStyle::default();
        let mut any = false;
        if let Some((_, s)) = syntax.iter().find(|(r, _)| r.start <= a && b <= r.end) {
            style = *s;
            any = true;
        }
        if emphasis.iter().any(|r| r.start <= a && b <= r.end) {
            style.background_color = Some(emph_bg);
            // A hue-independent signal so the changed span stays legible even
            // when the syntax text colour is close to the emphasis tint (e.g.
            // red syntax on the removed side). The underline reads regardless
            // of the text/background hue clash (#349 review feedback).
            let mut ul = emph_bg;
            ul.a = 1.0;
            style.underline = Some(gpui::UnderlineStyle {
                thickness: px(1.5),
                color: Some(ul),
                wavy: false,
            });
            any = true;
        }
        if any {
            out.push((a..b, style));
        }
    }
    out
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
    moved: &HashSet<usize>,
) -> gpui::AnyElement {
    match srows.get(i) {
        None => div().into_any(),
        Some(SplitDiffRow::Full(idx)) => render_main_diff_row(rows, *idx, sel_key),
        // No padding and no `items_start` on the row itself: cells stretch to
        // the full row height (flex default), so a wrapped line on one side
        // never leaves an unpainted strip under the shorter cell, and rows sit
        // flush against each other like the unified view.
        Some(SplitDiffRow::Pair { left, right }) => {
            // #349 word diff: when this row pairs a removed line with an added
            // line (a modify), compute the changed spans and emphasise them on
            // both sides. Sigils occupy byte 0, so shift the domain spans by +1.
            let (emph_old, emph_new) = pair_emphasis(rows, *left, *right);
            let mv_l = left.is_some_and(|x| moved.contains(&x));
            let mv_r = right.is_some_and(|x| moved.contains(&x));
            div()
                .id(("main-diff-split", i))
                .w_full()
                .flex()
                .flex_row()
                .text_sm()
                .child(split_cell(
                    rows,
                    *left,
                    SplitSide::Old,
                    sel_key,
                    &emph_old,
                    mv_l,
                ))
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(1.))
                        .bg(rgb(theme::theme().surface)),
                )
                .child(split_cell(
                    rows,
                    *right,
                    SplitSide::New,
                    sel_key,
                    &emph_new,
                    mv_r,
                ))
                .into_any()
        }
    }
}

/// Word-diff emphasis ranges for a split pair. Returns `(old_spans, new_spans)`
/// as byte ranges into the cells' displayed text (sigil included, so +1). Empty
/// unless the pair is a genuine removed↔added modify.
fn pair_emphasis(
    rows: &[DiffRow],
    left: Option<usize>,
    right: Option<usize>,
) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let (Some(l), Some(r)) = (left, right) else {
        return (Vec::new(), Vec::new());
    };
    let (
        Some(DiffRow::Line {
            kind: DiffLineKind::Removed,
            text: old_text,
            ..
        }),
        Some(DiffRow::Line {
            kind: DiffLineKind::Added,
            text: new_text,
            ..
        }),
    ) = (rows.get(l), rows.get(r))
    else {
        return (Vec::new(), Vec::new());
    };
    let mut old = Vec::new();
    let mut new = Vec::new();
    for span in kagi_domain::word_diff::word_diff(strip_sigil(old_text), strip_sigil(new_text)) {
        // +1 for the leading sigil the displayed text carries.
        let range = (span.range.start + 1)..(span.range.end + 1);
        match span.side {
            kagi_domain::word_diff::Side::Old => old.push(range),
            kagi_domain::word_diff::Side::New => new.push(range),
        }
    }
    (old, new)
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

    #[test]
    fn moved_rows_cached_reuses_the_same_set_per_key() {
        // issue #399: the second render of the same diff must hit the cache
        // (same Arc back), not rerun detect_moves.
        let rows = vec![line(DiffLineKind::Removed), line(DiffLineKind::Added)];
        let key = crate::ui::diff_selection::surface_key("cache-test.rs", rows.len());
        let first = super::moved_rows_cached(key, &rows);
        let second = super::moved_rows_cached(key, &rows);
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        // A different diff identity gets its own entry.
        let other_key = crate::ui::diff_selection::surface_key("other.rs", rows.len());
        assert!(!std::sync::Arc::ptr_eq(
            &first,
            &super::moved_rows_cached(other_key, &rows)
        ));
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
