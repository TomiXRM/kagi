//! Rows of the shared plan card: warning / blocker notes (named for
//! assistive technology, #354: warnings `Role::Note`, blockers `Role::Alert`)
//! and "commits to push" preview rows.

use gpui::{div, prelude::*, rgb, SharedString};
use kagi_ui_core::tooltip::HoverTooltip as _;

use super::modal_renderers::PlanCardAccent;
use super::theme::{self, theme as current_theme};
use super::MONO_FONT;

/// One warning/blocker line. `chip: false` (every modal except Pull/Push)
/// collapses to the original single-line "glyph␠text" row, unchanged.
/// `chip: true` wraps the same glyph + text in a tinted, bordered row (same
/// `theme::badge_style` recipe as the ref-badge chips in the commit graph).
pub(crate) fn render_note_row(
    id: SharedString,
    blocker: bool,
    glyph: &'static str,
    color: u32,
    text: &str,
    chip: bool,
) -> gpui::AnyElement {
    if !chip {
        return super::dialog_a11y::apply_note(id.clone(), div().id(id), blocker, text)
            .text_sm()
            .text_color(rgb(color))
            .overflow_hidden()
            .child(if glyph.is_empty() {
                SharedString::from(text.to_owned())
            } else {
                SharedString::from(format!("{glyph} {text}"))
            })
            .into_any_element();
    }
    let (bg, border, _) = theme::badge_style(color);
    super::dialog_a11y::apply_note(id.clone(), div().id(id), blocker, text)
        .flex()
        .flex_row()
        .items_start()
        .gap_2()
        .rounded_md()
        .bg(gpui::rgba(bg))
        .border_1()
        .border_color(gpui::rgba(border))
        .px_2()
        .py(theme::scaled_px(4.))
        .when(!glyph.is_empty(), |row| {
            row.child(div().flex_shrink_0().text_color(rgb(color)).child(glyph))
        })
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .text_sm()
                .text_color(rgb(current_theme().text_main))
                .overflow_hidden()
                .child(SharedString::from(text.to_string())),
        )
        .into_any_element()
}

/// Shared warning-list presentation; blocking status remains a separate axis.
pub(crate) fn note_presentation(
    kind: kagi_domain::plan_note::PlanNoteKind,
    palette: &theme::Theme,
) -> (&'static str, u32) {
    match kind {
        kagi_domain::plan_note::PlanNoteKind::Information => ("", palette.text_sub),
        kagi_domain::plan_note::PlanNoteKind::UnexpectedDataLoss => {
            ("\u{26a0}", palette.color_warning)
        }
    }
}

/// Advisory form-plan warnings: wrap full localized text with shared Note semantics.
pub(crate) fn render_warnings_list(
    prefix: &'static str,
    warnings: &[kagi_domain::plan_note::PlanNote],
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_shrink_0()
        .children(warnings.iter().enumerate().map(|(index, warning)| {
            let id: SharedString = format!("{prefix}-{index}").into();
            let text = kagi_ui_core::i18n::plan_note_text(warning);
            let (glyph, color) = note_presentation(warning.kind(), &current_theme());
            super::e2e::measure_control(
                id.clone(),
                render_note_row(id, false, glyph, color, &text, true),
            )
        }))
}

/// One "commits to push" preview row (T-HT-004). Plain (`accent: None`):
/// unchanged single truncated line. Chip style: the sha gets its own small
/// badge, the summary follows in regular text — same visual family as
/// [`render_note_row`]'s chips.
pub(crate) fn render_commit_row(
    index: usize,
    line: &str,
    accent: Option<PlanCardAccent>,
) -> gpui::AnyElement {
    let label = SharedString::from(line.to_string());
    #[cfg(feature = "gui-e2e")]
    super::e2e::plan_subjects::record(index, label.clone());
    let tooltip = label.clone();
    let row = div()
        .id(("plan-commit-row", index))
        .role(gpui::Role::Label)
        .aria_label(label.clone());
    let Some((_, color)) = accent else {
        return row
            .w_full()
            .min_w(gpui::px(0.))
            .font_family(MONO_FONT)
            .text_xs()
            .text_color(rgb(current_theme().text_sub))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(label)
            .hover_tooltip_builder(("plan-commit-help", index), move |window, cx| {
                super::render_helpers::subject_tooltip(tooltip.clone(), window, cx)
            })
            .into_any_element();
    };
    // Producer format is "<8-char-sha>  <summary>" (ops/push.rs). Fall back to
    // the whole line as the "sha" slot if that shape ever changes — this is
    // purely cosmetic splitting, never a behavioural decision (ADR-0129).
    let (sha, summary) = match line.split_once("  ") {
        Some((s, rest)) if s.len() == 8 && s.bytes().all(|c| c.is_ascii_hexdigit()) => {
            (s, rest.trim_start())
        }
        _ => (line, ""),
    };
    let (bg, border, _) = theme::badge_style(color);
    row.flex()
        .w_full()
        .min_w(gpui::px(0.))
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            // The sha is monospace: a column of hashes only reads as a column
            // when the glyphs are fixed width (user request 2026-09-06). The
            // summary next to it stays proportional — it is prose.
            div()
                .flex_shrink_0()
                .px_1()
                .rounded_sm()
                .bg(gpui::rgba(bg))
                .border_1()
                .border_color(gpui::rgba(border))
                .text_color(rgb(color))
                .font_family(MONO_FONT)
                .text_xs()
                .child(SharedString::from(sha.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .text_xs()
                .text_color(rgb(current_theme().text_sub))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(summary.to_string())),
        )
        .hover_tooltip_builder(("plan-commit-help", index), move |window, cx| {
            super::render_helpers::subject_tooltip(tooltip.clone(), window, cx)
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagi_domain::plan_note::PlanNoteKind;

    #[test]
    fn only_unexpected_loss_gets_caution_ink_and_symbol() {
        for palette in theme::THEMES {
            assert_eq!(
                note_presentation(PlanNoteKind::Information, palette),
                ("", palette.text_sub),
            );
            assert_eq!(
                note_presentation(PlanNoteKind::UnexpectedDataLoss, palette),
                ("\u{26a0}", palette.color_warning),
            );
        }
    }

    #[test]
    fn specialized_plan_notes_use_typed_presentation() {
        use kagi_domain::plan_note::{
            commit::CommitLeftoverParts, BranchNote, CloneNote, CommitNote, DiscardNote,
            HistoryNote, OplogRestoreNote, PlanNote, StashNote,
        };

        // These bespoke surfaces must not infer caution from warning-list membership,
        // operation destructiveness, or words in a branch/path.
        let information = [
            PlanNote::Branch(BranchNote::RenameRemoteNotRenamed),
            PlanNote::Clone(CloneNote::ForkAddsUpstream),
            PlanNote::History(HistoryNote::AmendDivergesFromRemote {
                sha: "abc1234".into(),
                branch: "unexpected loss".into(),
            }),
            PlanNote::Discard(DiscardNote::UntrackedWillBeDeleted { count: 1 }),
            PlanNote::Commit(CommitNote::LeftoverNotIncluded {
                count: 1,
                parts: CommitLeftoverParts {
                    modified: 1,
                    untracked: 0,
                },
            }),
            PlanNote::Stash(StashNote::UntrackedIncluded { count: 1 }),
            PlanNote::Stash(StashNote::ApplyWouldConflict {
                count: 1,
                files: vec!["file".into()],
            }),
            PlanNote::Stash(StashNote::PopWouldConflict {
                count: 1,
                files: vec!["file".into()],
            }),
            PlanNote::OplogRestore(OplogRestoreNote::MovesCheckedOutBranch {
                branch: "main".into(),
                path: "unexpected loss".into(),
            }),
            PlanNote::OplogRestore(OplogRestoreNote::CheckedOutDirty {
                branch: "main".into(),
                path: "worktree".into(),
            }),
        ];
        let loss = PlanNote::Stash(StashNote::IndexOnlyNotRestored { files: vec![] });
        for palette in theme::THEMES {
            for note in &information {
                assert_eq!(
                    note_presentation(note.kind(), palette),
                    ("", palette.text_sub),
                    "{note:?}",
                );
            }
            assert_eq!(
                note_presentation(loss.kind(), palette),
                ("\u{26a0}", palette.color_warning),
            );
        }
    }
}
