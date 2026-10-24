//! Non-interactive toolbar icon count overlay (#1067).
use crate::ui::{
    e2e,
    theme::{self, theme},
};
use gpui::{div, prelude::*, rgb, SharedString};

pub(crate) fn render(
    id: &'static str,
    count: usize,
    enabled: bool,
    foreground: u32,
) -> gpui::AnyElement {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let label = SharedString::from(text).into_any_element();
    #[cfg(feature = "gui-e2e")]
    let label = e2e::measure_control(format!("{id}-count-text"), label);
    div()
        .absolute()
        .top(theme::scaled_px(
            -2.0 - (theme::TOOLBAR_COUNT_HEIGHT - 14.0) / 2.0,
        ))
        .left(theme::scaled_px(22.0 - theme::TOOLBAR_COUNT_ICON_OVERLAP))
        .min_w(theme::scaled_px(theme::TOOLBAR_COUNT_HEIGHT))
        .h(theme::scaled_px(theme::TOOLBAR_COUNT_HEIGHT))
        .px(theme::scaled_px(3.0))
        .rounded_full()
        .bg(rgb(theme().ui_accent()))
        .when(!enabled, |el| el.opacity(0.5))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(foreground))
        .text_size(theme::scaled_px(theme::TOOLBAR_COUNT_TEXT))
        .font_weight(gpui::FontWeight::BOLD)
        .line_height(theme::scaled_px(theme::TOOLBAR_COUNT_LINE_HEIGHT))
        .child(label)
        .when(cfg!(feature = "gui-e2e"), |el| {
            el.child(e2e::measure_inside(format!("{id}-count")))
        })
        .into_any_element()
}
