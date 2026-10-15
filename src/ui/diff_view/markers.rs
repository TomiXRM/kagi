//! Invisible-source feedback shared by Unified rows and both Split columns.
use gpui::{div, prelude::*, rgb, AnyElement, SharedString};
use gpui_component::tooltip::Tooltip;
use kagi_domain::diff::LineEnding;
use kagi_ui_core::tooltip::HoverTooltip as _;

use crate::ui::{theme, Msg};

pub(crate) fn unsafe_unicode(side: &'static str, row: usize) -> AnyElement {
    let label = SharedString::from(Msg::UnsafeUnicodeBadge.t());
    let help = SharedString::from(Msg::UnsafeUnicodeTooltip.t());
    #[cfg(feature = "gui-e2e")]
    crate::ui::e2e::diff_content::warning(side, row, &label, &help);
    let (bg, border, text) = theme::badge_style(theme::theme().color_warning);
    div()
        .id((
            match side {
                "old" => "diff-unsafe-old",
                "new" => "diff-unsafe-new",
                _ => "diff-unsafe-unified",
            },
            row,
        ))
        .relative()
        .role(gpui::Role::Label)
        .aria_label(label.clone())
        .aria_description(help.clone())
        .flex_shrink_0()
        .ml_2()
        .px_1()
        .rounded_sm()
        .text_xs()
        .bg(gpui::rgba(bg))
        .border_1()
        .border_color(gpui::rgba(border))
        .text_color(rgb(text))
        .child(label)
        .map(|el| {
            #[cfg(feature = "gui-e2e")]
            let el = el.child(crate::ui::e2e::measure_inside(format!(
                "diff-unsafe-{side}-{row}"
            )));
            el
        })
        .hover_tooltip_builder(
            (
                match side {
                    "old" => "diff-unsafe-old-help",
                    "new" => "diff-unsafe-new-help",
                    _ => "diff-unsafe-unified-help",
                },
                row,
            ),
            move |window, cx| {
                #[cfg(feature = "gui-e2e")]
                crate::ui::e2e::diff_content::tooltip(&help);
                Tooltip::new(help.clone()).build(window, cx)
            },
        )
        .into_any_element()
}

pub(crate) fn line_ending(side: &'static str, row: usize, ending: LineEnding) -> AnyElement {
    let label = SharedString::from(match ending {
        LineEnding::Lf => Msg::DiffLineEndingLf.t(),
        LineEnding::CrLf => Msg::DiffLineEndingCrLf.t(),
        LineEnding::None => Msg::DiffLineEndingNone.t(),
    });
    #[cfg(feature = "gui-e2e")]
    crate::ui::e2e::diff_content::ending(side, row, &label);
    div()
        .id((
            match side {
                "old" => "diff-ending-old",
                "new" => "diff-ending-new",
                _ => "diff-ending-unified",
            },
            row,
        ))
        .relative()
        .role(gpui::Role::Label)
        .aria_label(label.clone())
        .flex_shrink_0()
        .ml_2()
        .px_1()
        .text_xs()
        .text_color(rgb(theme::theme().text_muted))
        .child(label)
        .map(|el| {
            #[cfg(feature = "gui-e2e")]
            let el = el.child(crate::ui::e2e::measure_inside(format!(
                "diff-ending-{side}-{row}"
            )));
            el
        })
        .into_any()
}
