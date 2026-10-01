//! Rows of the shared plan card: warning / blocker notes (named for
//! assistive technology, #354: warnings `Role::Note`, blockers `Role::Alert`)
//! and "commits to push" preview rows.

use gpui::{div, prelude::*, rgb, SharedString};

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
            .child(SharedString::from(format!("{} {}", glyph, text)))
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
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(color))
                .child(SharedString::from(glyph)),
        )
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

/// One "commits to push" preview row (T-HT-004). Plain (`accent: None`):
/// unchanged single truncated line. Chip style: the sha gets its own small
/// badge, the summary follows in regular text — same visual family as
/// [`render_note_row`]'s chips.
pub(crate) fn render_commit_row(line: &str, accent: Option<PlanCardAccent>) -> gpui::AnyElement {
    let Some((_, color)) = accent else {
        return div()
            .font_family(MONO_FONT)
            .text_xs()
            .text_color(rgb(current_theme().text_sub))
            .overflow_hidden()
            .child(SharedString::from(line.to_string()))
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
    div()
        .flex()
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
                .child(SharedString::from(summary.to_string())),
        )
        .into_any_element()
}
