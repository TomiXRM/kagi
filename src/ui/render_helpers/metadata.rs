//! Compact Graph commit-row author and age cells (#1003).

use super::*;

pub(super) fn subject(row: &CommitRow, ix: usize) -> impl IntoElement {
    div()
        .id(("graph-subject", ix))
        .flex_1()
        .min_w(px(0.))
        .text_color(rgb(theme().text_main))
        .truncate()
        .when(ix == 0 && cfg!(feature = "gui-e2e"), |el| {
            el.relative()
                .child(super::super::e2e::measure_inside("graph-commit-subject"))
        })
        .child(row.summary.clone())
}

pub(super) fn author(row: &CommitRow, ix: usize) -> impl IntoElement {
    // Preserve the full, sanitized author in the tooltip and AX row label.
    div()
        .id(("graph-author", ix))
        .w(theme::scaled_px(96.))
        .flex_shrink_0()
        .pl(theme::scaled_px(8.))
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .truncate()
        .tooltip({
            let author = row.author.clone();
            move |window, cx| {
                gpui_component::tooltip::Tooltip::new(author.clone()).build(window, cx)
            }
        })
        .when(ix == 0 && cfg!(feature = "gui-e2e"), |el| {
            el.relative()
                .child(super::super::e2e::measure_inside("graph-commit-author"))
        })
        .child(row.author.clone())
}

pub(super) fn age(row: &CommitRow, ix: usize) -> impl IntoElement {
    div()
        .w(theme::scaled_px(48.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .font_family(super::super::MONO_FONT)
        .text_xs()
        .text_color(rgb(theme().text_muted))
        .when(ix == 0 && cfg!(feature = "gui-e2e"), |el| {
            el.relative()
                .child(super::super::e2e::measure_inside("graph-commit-time"))
        })
        .child(row.date_short.clone())
}
