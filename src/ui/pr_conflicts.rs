//! The PR pane's Conflicts tab — read-only (ADR-0145).
//!
//! GitHub says a PR conflicts and stops there. This shows which files and
//! what the conflict looks like, so the answer to "is this a rename clash or
//! did someone rewrite the file" does not require checking the branch out.
//!
//! Deliberately not interactive: no accept/reject, no editing. Resolving needs
//! a working tree, and the point of this tab is that you can open it while
//! standing somewhere else.

use gpui::{div, prelude::*, px, rgb, SharedString};
use kagi_domain::resolution::{HunkModel, Region};
use kagi_git::{PrConflictFile, PrConflictKind};

use super::i18n::Msg;
use super::theme::theme;

/// Render the tab body for `conflicts`, which is `None` while it is still being
/// computed.
pub(crate) fn render_conflicts(
    conflicts: Option<&Result<Vec<PrConflictFile>, String>>,
) -> gpui::AnyElement {
    let body = match conflicts {
        None => div()
            .p_3()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("…"))
            .into_any_element(),
        Some(Err(e)) => div()
            .p_3()
            .text_sm()
            .text_color(rgb(theme().color_blocker))
            .child(SharedString::from(e.clone()))
            .into_any_element(),
        Some(Ok(files)) if files.is_empty() => div()
            .p_3()
            .text_sm()
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(Msg::PrConflictsNone.t()))
            .into_any_element(),
        Some(Ok(files)) => div()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .children(files.iter().map(render_file))
            .into_any_element(),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .child(
            div()
                .px_3()
                .pt_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrConflictsHint.t())),
        )
        .child(body)
        .into_any_element()
}

fn render_file(f: &PrConflictFile) -> gpui::AnyElement {
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(f.path.display().to_string())),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(match f.kind {
                    PrConflictKind::BothModified => String::new(),
                    PrConflictKind::DeleteModify => Msg::PrConflictDeleteModify.t().to_string(),
                    PrConflictKind::BothAdded => Msg::PrConflictBothAdded.t().to_string(),
                })),
        );

    let mut col = div().flex().flex_col().gap_1().child(header);
    // A delete/modify conflict has no three-way text: the header is the whole
    // story, and an empty code box below it would only look broken.
    if !f.marker_text.is_empty() {
        col = col.child(render_hunks(&f.marker_text));
    }
    col.into_any_element()
}

/// The conflict, side by side, using the same parser the conflict editor uses
/// so the two cannot disagree about what a hunk is.
fn render_hunks(marker_text: &str) -> gpui::AnyElement {
    let model = HunkModel::from_marker_text(marker_text);
    let rows = model.regions.iter().filter_map(|r| match r {
        Region::Hunk(h) => Some(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(side(&h.current, theme().diff_removed_bg))
                .child(side(&h.incoming, theme().diff_added_bg))
                .into_any_element(),
        ),
        // Passthrough context is dropped: this is a summary of the conflicts,
        // not a file viewer, and unconflicted lines are what the Diff tab is
        // already for.
        Region::Passthrough(_) => None,
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(rows)
        .into_any_element()
}

fn side(lines: &[String], bg: u32) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .p_1()
        .rounded_sm()
        .bg(rgb(bg))
        .font_family(super::MONO_FONT)
        .text_xs()
        .text_color(rgb(theme().text_main))
        .children(lines.iter().map(|l| {
            div()
                .whitespace_normal()
                .child(SharedString::from(l.clone()))
                .into_any_element()
        }))
        .into_any_element()
}
