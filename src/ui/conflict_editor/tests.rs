use super::*;

use kagi_git::resolution::HunkModel;

#[test]
fn source_panes_contain_every_hunk_and_all_surrounding_lines() {
    let model = HunkModel::from_marker_text(
        "head\n<<<<<<< Current\nc1\nc2\n=======\ni1\n>>>>>>> Incoming\n\
             ctx1\nctx2\nctx3\nctx4\n<<<<<<< Current\nx\n=======\ny\n>>>>>>> Incoming\n\
             tail1\ntail2\n",
    );
    for side in [SelectionSide::Current, SelectionSide::Incoming] {
        let rows = build_side_rows(&model, side);
        assert_eq!(rows.len(), 12);
        let headers: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| match entry {
                SideRow::Line {
                    hunk_index,
                    line_index: 0,
                    ..
                } => Some((row, *hunk_index)),
                _ => None,
            })
            .collect();
        assert_eq!(headers, vec![(2, 0), (9, 1)]);
        assert!(matches!(rows[1], SideRow::Action { hunk_index: 0 }));
        assert!(matches!(rows[8], SideRow::Action { hunk_index: 1 }));
        assert!(matches!(rows.first(), Some(SideRow::Context { text, .. }) if text == "head"));
        assert!(matches!(rows.last(), Some(SideRow::Context { text, .. }) if text == "tail2"));
    }
}

fn counts(rows: &[SideRow]) -> (usize, usize, usize, usize) {
    let mut ctx = 0;
    let mut blank = 0;
    let mut line = 0;
    for r in rows {
        match r {
            SideRow::Context { .. } => ctx += 1,
            SideRow::Blank { .. } => blank += 1,
            SideRow::Line { .. } => line += 1,
            SideRow::Action { .. } => {}
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
fn side_panes_are_equal_length_and_include_all_context() {
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
    assert_eq!(cur.len(), 7, "one action row plus six source/context rows");
    assert!(matches!(cur[1], SideRow::Action { hunk_index: 0 }));
    assert!(matches!(inc[1], SideRow::Action { hunk_index: 0 }));

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
