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
use kagi_domain::diff::DiffLineKind;

/// Build the diff view for one conflicted file.
///
/// Each hunk becomes a header plus the two sides. Unconflicted context is
/// dropped: this answers "what clashes", and the surrounding file is what the
/// Diff tab is already for.
pub(crate) fn conflict_diff_view(f: &PrConflictFile) -> MainDiffView {
    let mut rows: Vec<DiffRow> = Vec::new();
    let model = HunkModel::from_marker_text(&f.marker_text);
    let mut n = 0usize;
    for region in &model.regions {
        let Region::Hunk(h) = region else { continue };
        n += 1;
        rows.push(DiffRow::HunkHeader(SharedString::from(format!(
            "@@ conflict {n} @@"
        ))));
        for l in &h.current {
            rows.push(line(DiffLineKind::Removed, l));
        }
        for l in &h.incoming {
            rows.push(line(DiffLineKind::Added, l));
        }
    }
    if rows.is_empty() {
        // Delete/modify has no three-way text; say which way round it is
        // rather than showing an empty pane.
        rows.push(DiffRow::HunkHeader(SharedString::from(match f.kind {
            PrConflictKind::DeleteModify => Msg::PrConflictDeleteModify.t().to_string(),
            PrConflictKind::BothAdded => Msg::PrConflictBothAdded.t().to_string(),
            PrConflictKind::BothModified => String::new(),
        })));
    }
    MainDiffView {
        title: SharedString::from(f.path.display().to_string()),
        stats: SharedString::from(format!("{n} conflict(s)")),
        rows: std::sync::Arc::new(rows),
        source: MainDiffSource::Synthetic,
        images: None,
    }
}

fn line(kind: DiffLineKind, text: &str) -> DiffRow {
    DiffRow::Line {
        kind,
        text: SharedString::from(text.to_string()),
        old_lineno: None,
        new_lineno: None,
        highlights: Vec::new(),
    }
}
