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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::SharedString;
use kagi_domain::resolution::{HunkModel, Region};
use kagi_git::{PrConflictFile, PrConflictKind};

use super::diff_view::{DiffRow, MainDiffSource, MainDiffView};
use super::i18n::{self, Lang, Msg};
use super::theme::theme;
use kagi_domain::diff::DiffLineKind;

/// One loaded conflict file per PR tab. The backend caps marker input at
/// 512 KiB; only its rendered rows survive loading, not a second raw-text copy.
/// File selection or a new load replaces this owner. Snapshot clones share
/// rows and jump indices, so repainting also shares split-cache derivations.
pub(crate) struct ConflictPreview {
    path: PathBuf,
    view: MainDiffView,
    jumps: Arc<Vec<usize>>,
    empty_message: Option<Msg>,
    lang: Lang,
    theme_slug: &'static str,
}

impl ConflictPreview {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn snapshot(&mut self) -> (MainDiffView, Arc<Vec<usize>>) {
        let lang = i18n::lang();
        let theme_slug = theme().slug;
        if self.lang != lang || self.theme_slug != theme_slug {
            // Detach from outstanding render snapshots and weak split-cache
            // keys before changing presentation. Source text stays unchanged.
            let rows = Arc::make_mut(&mut self.view.rows);
            if self.lang != lang {
                for (index, &row) in self.jumps.iter().enumerate() {
                    rows[row] = DiffRow::HunkHeader(conflict_header(index + 1, self.jumps.len()));
                }
                if let Some(message) = self.empty_message {
                    rows[0] = DiffRow::HunkHeader(message.t().into());
                }
            }
            if self.theme_slug != theme_slug {
                super::diff_view::highlight_diff_rows(rows, &self.path);
            }
            self.lang = lang;
            self.theme_slug = theme_slug;
        }
        (self.view.clone(), Arc::clone(&self.jumps))
    }
}

fn conflict_header(index: usize, total: usize) -> SharedString {
    format!("@@ {} {index}/{total} @@", Msg::PrConflictHunk.t()).into()
}

impl super::pr_mode::PrTab {
    pub(super) fn apply_conflict_text(
        &mut self,
        path: &Path,
        text: Option<&str>,
    ) -> Option<(usize, Arc<Vec<DiffRow>>)> {
        let files = self.conflicts.as_ref()?.as_ref().ok()?;
        let file = files.get(
            self.conflict_selected
                .unwrap_or(0)
                .min(files.len().saturating_sub(1)),
        )?;
        if file.path != path {
            return None;
        }
        let preview = conflict_diff_view(file, text);
        let first = preview
            .jumps
            .first()
            .map(|&row| (row, Arc::clone(&preview.view.rows)));
        self.conflict_preview = Some(preview);
        self.conflict_at = 0;
        first
    }
}

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
/// Returns the loaded preview, including conflict jump targets.
pub(crate) fn conflict_diff_view(f: &PrConflictFile, marker_text: Option<&str>) -> ConflictPreview {
    let mut rows: Vec<DiffRow> = Vec::new();
    let mut jumps: Vec<usize> = Vec::new();
    let model = HunkModel::from_marker_text(marker_text.unwrap_or_default());
    let (mut old_no, mut new_no) = (1u32, 1u32);
    let mut n = 0usize;

    for region in &model.regions {
        match region {
            Region::Passthrough(lines) => {
                for l in lines {
                    rows.push(line(DiffLineKind::Context, l, Some(old_no), Some(new_no)));
                    old_no += 1;
                    new_no += 1;
                }
            }
            Region::Hunk(h) => {
                n += 1;
                // The header row is the jump target, so the conflict lands at
                // the top of the viewport with its own label above it.
                jumps.push(rows.len());
                rows.push(DiffRow::HunkHeader(conflict_header(n, 0)));
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
        rows[ix] = DiffRow::HunkHeader(conflict_header(i + 1, total));
    }

    let empty_message = rows
        .is_empty()
        .then(|| match f.kind {
            PrConflictKind::DeleteModify => Some(Msg::PrConflictDeleteModify),
            PrConflictKind::BothAdded | PrConflictKind::BothModified if marker_text.is_none() => {
                Some(Msg::PrConflictTooLarge)
            }
            PrConflictKind::BothAdded => Some(Msg::PrConflictBothAdded),
            PrConflictKind::Binary => Some(Msg::PrConflictBinary),
            PrConflictKind::BothModified => None,
        })
        .flatten();
    if rows.is_empty() {
        // No hunks: either the kind has no three-way text, or the file was
        // past the size cap. Say which, rather than showing an empty pane.
        rows.push(DiffRow::HunkHeader(
            empty_message.map(Msg::t).unwrap_or_default().into(),
        ));
    }

    // Syntax highlighting, the same pass the Diff tab runs — the language is
    // taken from the real path, so a conflict in a .rs file reads like Rust.
    super::diff_view::highlight_diff_rows(&mut rows, &f.path);

    let view = MainDiffView {
        title: SharedString::from(f.path.display().to_string()),
        stats: SharedString::from(format!("{total} conflict(s)")),
        rows: std::sync::Arc::new(rows),
        source: MainDiffSource::Synthetic,
        images: None,
    };
    ConflictPreview {
        path: f.path.clone(),
        view,
        jumps: Arc::new(jumps),
        empty_message,
        lang: i18n::lang(),
        theme_slug: theme().slug,
    }
}

/// One row, carrying the leading sigil the diff renderer and the syntax
/// highlighter both expect: they strip the first character to recover the
/// source line, so a row without one loses its first character.
fn line(
    kind: DiffLineKind,
    text: &str,
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
) -> DiffRow {
    let sigil = match kind {
        DiffLineKind::Added => '+',
        DiffLineKind::Removed => '-',
        DiffLineKind::Context => ' ',
    };
    DiffRow::Line {
        kind,
        text: SharedString::from(format!("{sigil}{text}")),
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
    jumps: Arc<Vec<usize>>,
    rows: std::sync::Arc<Vec<DiffRow>>,
    at: usize,
    cx: &mut gpui::Context<crate::ui::KagiApp>,
) -> gpui::AnyElement {
    use gpui::{div, prelude::*};
    use gpui_component::button::ButtonVariants as _;
    use gpui_component::Sizable as _;

    let total = jumps.len();
    let at = at.min(total.saturating_sub(1));
    let step = move |delta: isize| {
        let jumps = jumps.clone();
        let rows = rows.clone();
        move |this: &mut crate::ui::KagiApp,
              _: &gpui::ClickEvent,
              _w: &mut gpui::Window,
              cx: &mut gpui::Context<crate::ui::KagiApp>| {
            // Wraps: with several conflicts in a file, walking off the end and
            // round to the first is what you want, not a dead button.
            let next = (at as isize + delta).rem_euclid(total as isize) as usize;
            this.pr_mode_jump_conflict(next, jumps.get(next).copied(), Some(rows.clone()), cx);
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
                // Accent-coloured and xsmall: this sits in a header row of
                // neutral controls, and it is the one thing there that moves
                // you somewhere — worth being both smaller and louder than the
                // outlined toggle beside it.
                .primary()
                .xsmall()
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
                .primary()
                .xsmall()
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
        let ConflictPreview { view, jumps, .. } = conflict_diff_view(&file(), Some(text));
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
        let ConflictPreview { view, jumps, .. } = conflict_diff_view(&file(), Some(text));
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

    /// The renderer and the syntax highlighter both strip the first character
    /// to recover the source line. A row built without a sigil silently loses
    /// its first character — invisible in a summary view, wrong in a real one.
    #[test]
    fn every_line_carries_its_sigil() {
        let text = "keep\n<<<<<<< base\nBASE\n=======\nPR\n>>>>>>> PR\n";
        let ConflictPreview { view, .. } = conflict_diff_view(&file(), Some(text));
        let seen: Vec<(&str, &str)> = view
            .rows
            .iter()
            .filter_map(|r| match r {
                DiffRow::Line { kind, text, .. } => Some((
                    match kind {
                        DiffLineKind::Added => "+",
                        DiffLineKind::Removed => "-",
                        DiffLineKind::Context => " ",
                    },
                    text.as_ref(),
                )),
                _ => None,
            })
            .collect();
        for (sigil, text) in &seen {
            assert!(
                text.starts_with(sigil),
                "{text:?} must start with {sigil:?}"
            );
        }
        // …and the content survives the sigil intact.
        assert_eq!(
            seen.iter().map(|(_, t)| &t[1..]).collect::<Vec<_>>(),
            vec!["keep", "BASE", "PR"]
        );
    }

    /// A file with no readable text (binary, deleted side, over the cap) still
    /// renders a row saying why, rather than an empty pane.
    #[test]
    fn an_untextable_conflict_still_explains_itself() {
        for (kind, reason) in [
            (PrConflictKind::Binary, Msg::PrConflictBinary),
            (PrConflictKind::DeleteModify, Msg::PrConflictDeleteModify),
            (PrConflictKind::BothModified, Msg::PrConflictTooLarge),
        ] {
            let f = PrConflictFile {
                path: PathBuf::from("f.bin"),
                kind,
            };
            let ConflictPreview { view, jumps, .. } = conflict_diff_view(&f, None);
            assert!(jumps.is_empty());
            match view.rows.as_slice() {
                [DiffRow::HunkHeader(message)] => assert_eq!(message.as_ref(), reason.t()),
                _ => panic!("a file without text must explain its conflict kind"),
            }
        }
    }

    #[test]
    fn loaded_preview_snapshots_preserve_visible_rows_jumps_and_move_marks() {
        let text = "keep\n<<<<<<< base\nlong_function_name_moved_unchanged\n=======\n\
                    long_function_name_moved_unchanged\n>>>>>>> PR\nend\n";
        let mut preview = conflict_diff_view(&file(), Some(text));
        for _ in 0..3 {
            let (view, jumps) = preview.snapshot();
            assert_eq!(jumps.as_slice(), &[1]);
            let projection = super::super::diff_split::split_projection(&view.rows);
            assert_eq!(projection.moved, std::collections::HashSet::from([2, 3]));
            let lines: Vec<_> = view
                .rows
                .iter()
                .filter_map(|row| match row {
                    DiffRow::Line { text, .. } => Some(text.as_ref()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                lines,
                [
                    " keep",
                    "-long_function_name_moved_unchanged",
                    "+long_function_name_moved_unchanged",
                    " end",
                ]
            );
        }
    }

    #[test]
    fn replacing_loaded_text_updates_jumps_and_moves_and_releases_old_source() {
        let first_text = "keep\n<<<<<<< base\nlong_function_name_moved_unchanged\n=======\n\
                          long_function_name_moved_unchanged\n>>>>>>> PR\nend\n";
        let next_text = "start\n<<<<<<< base\nremoved\n=======\nadded\n>>>>>>> PR\nmiddle\n\
                         <<<<<<< base\nother\n=======\nnew\n>>>>>>> PR\n";
        let mut preview = conflict_diff_view(&file(), Some(first_text));
        let (first, first_jumps) = preview.snapshot();
        let first_lifetime = Arc::downgrade(&first.rows);
        preview = conflict_diff_view(&file(), Some(next_text));
        let (next, next_jumps) = preview.snapshot();
        assert_eq!(first_jumps.as_slice(), &[1]);
        assert_eq!(next_jumps.as_slice(), &[1, 5]);
        assert!(super::super::diff_split::split_projection(&next.rows)
            .moved
            .is_empty());
        assert_eq!(
            super::super::diff_split::split_projection(&first.rows).moved,
            std::collections::HashSet::from([2, 3]),
        );
        drop(first);
        assert!(
            first_lifetime.upgrade().is_none(),
            "replacement must not retain old source rows"
        );
    }
}
