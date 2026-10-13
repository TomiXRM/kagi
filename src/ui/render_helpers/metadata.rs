//! Compact Graph commit-row author and age cells (#1003).

use super::*;

pub(super) fn subject(row: &CommitRow, ix: usize, min_width: f32) -> impl IntoElement {
    div()
        .id(("graph-subject", ix))
        .flex_1()
        .min_w(theme::scaled_px(min_width))
        .text_color(rgb(theme().text_main))
        .truncate()
        .when(ix == 0 && cfg!(feature = "gui-e2e"), |el| {
            el.relative()
                .child(super::super::e2e::measure_inside("graph-commit-subject"))
        })
        .child(row.summary.clone())
}

pub(crate) fn subject_tooltip(
    subject: SharedString,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> gpui::AnyView {
    #[cfg(feature = "gui-e2e")]
    super::super::e2e::text_caps::record_tooltip(subject.clone());
    // Kit tooltips are intrinsically sized. Give the text a definite wrapping
    // width, including the kit's horizontal padding and outer viewport margins.
    let width = theme::scaled_px(480.)
        .min((window.viewport_size().width - window.rem_size() * 2.).max(px(1.)));
    let text_width = (width - window.rem_size() - px(2.)).max(px(1.));
    let tooltip = gpui_component::tooltip::Tooltip::element(move |_, _| {
        div()
            .w(text_width)
            .min_w(px(0.))
            .whitespace_normal()
            .child(subject.clone())
    })
    .w(width)
    .max_w(width)
    .build(window, cx);
    #[cfg(feature = "gui-e2e")]
    {
        use gpui::AppContext as _;
        cx.new(|_| MeasuredSubjectTooltip(tooltip)).into()
    }
    #[cfg(not(feature = "gui-e2e"))]
    tooltip
}

#[cfg(feature = "gui-e2e")]
struct MeasuredSubjectTooltip(gpui::AnyView);

#[cfg(feature = "gui-e2e")]
impl gpui::Render for MeasuredSubjectTooltip {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .child(self.0.clone())
            .child(super::super::e2e::measure_inside("graph-subject-tooltip"))
    }
}

pub(super) fn author(row: &CommitRow, ix: usize) -> impl IntoElement {
    // Preserve the full, sanitized author in the tooltip and AX row label.
    div()
        .id(("graph-author", ix))
        .w(theme::scaled_px(
            super::super::graph_columns::AUTHOR_PREFERRED,
        ))
        .min_w(theme::scaled_px(super::super::graph_columns::AUTHOR_MIN))
        .flex_shrink(1.)
        .pl(theme::scaled_px(8.))
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
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
        .w(theme::scaled_px(super::super::graph_columns::TIME_WIDTH))
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
        .child(kagi_ui_core::time::relative_time_short(
            row.author_secs,
            kagi_ui_core::time::now_unix_secs(),
        ))
}
