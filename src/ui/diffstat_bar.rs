//! Diffstat mini bar — W16-DIFFSTAT (T-DIFFSTAT-004 / 006 / 007)
//!
//! Renders the trailing `+N -M [bar]` unit appended to a changed-file row in
//! the Inspector and Commit Panel.  Layout (right → left within the unit):
//!
//! ```text
//!   +N            (green, additions, digit-aligned)
//!   -M            (red,   deletions, digit-aligned)
//!   [▮▮▮▯▯]       (green segments then red segments)
//! ```
//!
//! Fallbacks (spec §Diffstat Mini Bar 仕様 / T-DIFFSTAT-007):
//! * binary       → `BIN`, no bar.
//! * 0 changes    → faint placeholder bar (no numbers).
//! * deleted      → red-only bar + `-N`.
//! * renamed      → handled by the caller's badge (`R`); the bar still shows
//!   additions/deletions of the rename's content change.
//!
//! All colours come from [`theme()`]; nothing is hard-coded.  A gpui-component
//! [`Tooltip`] (which requires the element carry an `.id`) shows
//! `"N additions, M deletions"`.

use gpui::{rgb, IntoElement, SharedString, Styled, ParentElement, InteractiveElement, StatefulInteractiveElement};
use gpui_component::tooltip::Tooltip;

use kagi::git::{bar_segments, FileDiffStat};

use super::theme::{self, theme};

/// Maximum number of bar segments (spec: 5–8). 5 keeps the unit compact.
const MAX_SEGMENTS: usize = 5;
/// Fixed pixel width reserved for the bar so the numbers stay column-aligned.
const SEG_W: f32 = 4.0;
const SEG_H: f32 = 9.0;
const SEG_GAP: f32 = 1.0;

/// Build the trailing diffstat unit for one file row.
///
/// `id_seed` must be unique within the parent list (e.g. the file index) so the
/// tooltip's stateful element id does not collide.
///
/// Returns a right-aligned, `flex_shrink_0` row element.  When `stat` is `None`
/// (diffstat unavailable for this file), an empty spacer of the same width is
/// returned so column alignment is preserved.
pub fn diffstat_unit(id_seed: usize, stat: Option<&FileDiffStat>) -> impl IntoElement {
    let t = theme();
    let base = gpui::div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap_1()
        .flex_shrink_0();

    let stat = match stat {
        Some(s) => s,
        // No diffstat data — render nothing (keeps the row height stable via
        // the parent's layout; we intentionally avoid reserving width so the
        // path can use the full row when stats are unavailable).
        None => return base.into_any_element(),
    };

    // ── Binary: show BIN, no numbers/bar ─────────────────────────────────
    if stat.is_binary {
        return base
            .child(
                gpui::div()
                    .text_xs()
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from("BIN")),
            )
            .into_any_element();
    }

    let additions = stat.additions;
    let deletions = stat.deletions;
    let total = additions + deletions;

    // ── Numbers: `+N` (green) and `-M` (red); omit a side that is 0 unless
    //    both are 0 (placeholder case handled below). ─────────────────────
    let mut numbers = gpui::div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .flex_shrink_0();
    if total > 0 {
        if additions > 0 {
            numbers = numbers.child(
                gpui::div()
                    .text_xs()
                    .text_color(rgb(t.change_added))
                    .child(SharedString::from(format!("+{additions}"))),
            );
        }
        if deletions > 0 {
            numbers = numbers.child(
                gpui::div()
                    .text_xs()
                    .text_color(rgb(t.change_deleted))
                    .child(SharedString::from(format!("-{deletions}"))),
            );
        }
    }

    // ── Bar segments ──────────────────────────────────────────────────────
    let (green, red) = bar_segments(additions, deletions, MAX_SEGMENTS);
    let mut bar = gpui::div()
        .flex()
        .flex_row()
        .items_center()
        .gap(theme::scaled_px(SEG_GAP))
        .flex_shrink_0();

    if total == 0 {
        // Faint placeholder: empty muted track.
        for _ in 0..MAX_SEGMENTS {
            bar = bar.child(
                gpui::div()
                    .w(theme::scaled_px(SEG_W))
                    .h(theme::scaled_px(SEG_H))
                    .rounded_sm()
                    .bg(rgb(t.surface)),
            );
        }
    } else {
        for _ in 0..green {
            bar = bar.child(
                gpui::div()
                    .w(theme::scaled_px(SEG_W))
                    .h(theme::scaled_px(SEG_H))
                    .rounded_sm()
                    .bg(rgb(t.change_added)),
            );
        }
        for _ in 0..red {
            bar = bar.child(
                gpui::div()
                    .w(theme::scaled_px(SEG_W))
                    .h(theme::scaled_px(SEG_H))
                    .rounded_sm()
                    .bg(rgb(t.change_deleted)),
            );
        }
        // Pad the remainder with muted track so the bar keeps a fixed width.
        for _ in (green + red)..MAX_SEGMENTS {
            bar = bar.child(
                gpui::div()
                    .w(theme::scaled_px(SEG_W))
                    .h(theme::scaled_px(SEG_H))
                    .rounded_sm()
                    .bg(rgb(t.surface)),
            );
        }
    }

    // ── Tooltip text ──────────────────────────────────────────────────────
    let tip = if total == 0 {
        SharedString::from("No line changes")
    } else {
        SharedString::from(format!("{additions} additions, {deletions} deletions"))
    };

    base.id(("diffstat-unit", id_seed))
        .child(numbers)
        .child(bar)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .into_any_element()
}
