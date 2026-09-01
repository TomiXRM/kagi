//! The PR pane's Conflicts tab — read-only (ADR-0145).
//!
//! GitHub says a PR conflicts and stops there. This shows which files and what
//! the conflict looks like, so "is this a rename clash or did someone rewrite
//! the file" does not require checking the branch out.
//!
//! The conflict is rendered as an ordinary diff: the base side is the removed
//! side, the PR side the added one. That is not a cosmetic choice — going
//! through `MainDiffView` means the unified/side-by-side toggle, the
//! virtualized list, syntax highlighting and text selection all work here
//! exactly as they do in the Diff tab, instead of a second half-featured
//! renderer that would have to grow each of them again.
//!
//! Deliberately not interactive: no accept/reject, no editing. Resolving needs
//! a working tree, and the point of this tab is that you can open it while
//! standing somewhere else.

use gpui::SharedString;
use kagi_domain::resolution::{HunkModel, Region};
use kagi_git::{PrConflictFile, PrConflictKind};

use super::diff_view::{DiffRow, MainDiffSource, MainDiffView};
use super::i18n::Msg;
use super::theme::theme;
use kagi_domain::diff::DiffLineKind;

/// Build the diff view for one conflicted file.
///
/// The whole file is rendered, not just the clashing parts: a conflict you
/// cannot see the code around is not something you can judge. Unconflicted
/// lines are context; the two sides of each conflict are the removed and added
/// lines, which is what they are.
///
/// Line numbers follow diff semantics — the old column counts the base side,
/// the new column the PR side, so context advances both and each side of a
/// conflict advances only its own. That is the numbering you would see after
/// actually merging.
///
/// Returns the view and the row index of each conflict, for the jump control.
pub(crate) fn conflict_diff_view(
    f: &PrConflictFile,
    marker_text: Option<&str>,
) -> (MainDiffView, Vec<usize>) {
    let mut rows: Vec<DiffRow> = Vec::new();
    let mut jumps: Vec<usize> = Vec::new();
    let model = HunkModel::from_marker_text(marker_text.unwrap_or_default());
    let (mut old_no, mut new_no) = (1u32, 1u32);
    let mut n = 0usize;

    for region in &model.regions {
        match region {
            Region::Passthrough(lines) => {
                for l in lines {
                    rows.push(DiffRow::Line {
                        kind: DiffLineKind::Context,
                        text: SharedString::from(l.clone()),
                        old_lineno: Some(old_no),
                        new_lineno: Some(new_no),
                        highlights: Vec::new(),
                    });
                    old_no += 1;
                    new_no += 1;
                }
            }
            Region::Hunk(h) => {
                n += 1;
                // The header row is the jump target, so the conflict lands at
                // the top of the viewport with its own label above it.
                jumps.push(rows.len());
                rows.push(DiffRow::HunkHeader(SharedString::from(format!(
                    "@@ {} {}/{} @@",
                    Msg::PrConflictHunk.t(),
                    n,
                    0 // filled in below once the total is known
                ))));
                for l in &h.current {
                    rows.push(line(DiffLineKind::Removed, l, Some(old_no), None));
                    old_no += 1;
                }
                for l in &h.incoming {
                    rows.push(line(DiffLineKind::Added, l, None, Some(new_no)));
                    new_no += 1;
                }
            }
        }
    }

    // Now that the total is known, restate each header as "i/total".
    let total = jumps.len();
    for (i, &ix) in jumps.iter().enumerate() {
        rows[ix] = DiffRow::HunkHeader(SharedString::from(format!(
            "@@ {} {}/{} @@",
            Msg::PrConflictHunk.t(),
            i + 1,
            total
        )));
    }

    if rows.is_empty() {
        // No hunks: either the kind has no three-way text, or the file was
        // past the size cap. Say which, rather than showing an empty pane.
        rows.push(DiffRow::HunkHeader(SharedString::from(match f.kind {
            PrConflictKind::DeleteModify => Msg::PrConflictDeleteModify.t().to_string(),
            PrConflictKind::BothAdded | PrConflictKind::BothModified if marker_text.is_none() => {
                Msg::PrConflictTooLarge.t().to_string()
            }
            PrConflictKind::BothAdded => Msg::PrConflictBothAdded.t().to_string(),
            PrConflictKind::Binary => Msg::PrConflictBinary.t().to_string(),
            PrConflictKind::BothModified => String::new(),
        })));
    }

    let view = MainDiffView {
        title: SharedString::from(f.path.display().to_string()),
        stats: SharedString::from(format!("{total} conflict(s)")),
        rows: std::sync::Arc::new(rows),
        source: MainDiffSource::Synthetic,
        images: None,
    };
    (view, jumps)
}

fn line(
    kind: DiffLineKind,
    text: &str,
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
) -> DiffRow {
    DiffRow::Line {
        kind,
        text: SharedString::from(text.to_string()),
        old_lineno,
        new_lineno,
        highlights: Vec::new(),
    }
}

/// Prev/next across the conflicts in the open file, with the position.
///
/// Scrolling is `ListState::scroll_to_reveal_item` on the conflict's header
/// row, so the label lands at the top of the view rather than the first
/// clashing line arriving with no indication of which side it is.
pub(crate) fn render_jump_nav(
    jumps: Vec<usize>,
    at: usize,
    cx: &mut gpui::Context<crate::ui::KagiApp>,
) -> gpui::AnyElement {
    use gpui::{div, prelude::*};
    use gpui_component::Sizable as _;

    let total = jumps.len();
    let at = at.min(total.saturating_sub(1));
    let step = move |delta: isize| {
        let jumps = jumps.clone();
        move |this: &mut crate::ui::KagiApp,
              _: &gpui::ClickEvent,
              _w: &mut gpui::Window,
              cx: &mut gpui::Context<crate::ui::KagiApp>| {
            // Wraps: with several conflicts in a file, walking off the end and
            // round to the first is what you want, not a dead button.
            let next = (at as isize + delta).rem_euclid(total as isize) as usize;
            this.pr_mode_jump_conflict(next, jumps.get(next).copied(), cx);
        }
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            gpui_component::button::Button::new("conflict-prev")
                .label("‹")
                .outline()
                .small()
                .on_click(cx.listener(step(-1))),
        )
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme().text_sub))
                .child(SharedString::from(format!("{}/{}", at + 1, total))),
        )
        .child(
            gpui_component::button::Button::new("conflict-next")
                .label("›")
                .outline()
                .small()
                .on_click(cx.listener(step(1))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file() -> PrConflictFile {
        PrConflictFile {
            path: PathBuf::from("f.txt"),
            kind: PrConflictKind::BothModified,
        }
    }

    /// Context lines advance both sides; each side of a conflict advances only
    /// its own. Getting this wrong is invisible until you compare against the
    /// real file and find the numbers drifting after the first conflict.
    #[test]
    fn line_numbers_follow_diff_semantics() {
        let text = "a\nb\n<<<<<<< base\nBASE1\nBASE2\n=======\nPR1\n>>>>>>> PR\nz\n";
        let (view, jumps) = conflict_diff_view(&file(), Some(text));
        assert_eq!(jumps.len(), 1);

        let nums: Vec<(Option<u32>, Option<u32>)> = view
            .rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Line {
                    old_lineno,
                    new_lineno,
                    ..
                } => Some((*old_lineno, *new_lineno)),
                _ => None,
            })
            .collect();
        assert_eq!(
            nums,
            vec![
                (Some(1), Some(1)), // a       — context
                (Some(2), Some(2)), // b       — context
                (Some(3), None),    // BASE1   — base side only
                (Some(4), None),    // BASE2
                (None, Some(3)),    // PR1     — PR side only
                // `z` resumes at 5 on the base side (two lines consumed) and 4
                // on the PR side (one).
                (Some(5), Some(4)),
            ]
        );
    }

    /// Every conflict is reachable, and the header says which of how many —
    /// the jump control is useless if the position is not also on the row.
    #[test]
    fn each_conflict_has_a_numbered_header_and_a_jump_target() {
        let text = "x\n<<<<<<< base\nA\n=======\nB\n>>>>>>> PR\ny\n\
                    <<<<<<< base\nC\n=======\nD\n>>>>>>> PR\nz\n";
        let (view, jumps) = conflict_diff_view(&file(), Some(text));
        assert_eq!(jumps.len(), 2, "both conflicts must be jump targets");
        for (i, &ix) in jumps.iter().enumerate() {
            match &view.rows[ix] {
                DiffRow::HunkHeader(h) => assert!(
                    h.contains(&format!("{}/2", i + 1)),
                    "header at the jump target must state the position: {h}"
                ),
                _ => panic!("jump target {ix} is not a conflict header"),
            }
        }
    }

    /// A file with no readable text (binary, deleted side, over the cap) still
    /// renders a row saying why, rather than an empty pane.
    #[test]
    fn an_untextable_conflict_still_explains_itself() {
        for kind in [
            PrConflictKind::Binary,
            PrConflictKind::DeleteModify,
            PrConflictKind::BothModified,
        ] {
            let f = PrConflictFile {
                path: PathBuf::from("f.bin"),
                kind,
            };
            let (view, jumps) = conflict_diff_view(&f, None);
            assert!(jumps.is_empty());
            assert_eq!(view.rows.len(), 1, "{kind:?}");
        }
    }
}
