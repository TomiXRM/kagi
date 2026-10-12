use super::*;
use gpui_component::tooltip::Tooltip;

/// Shared named, ringed » affordance for toolbar and document overflow.
pub(crate) fn trigger(
    id: &'static str,
    label: &'static str,
    selected: bool,
    focus: &gpui::FocusHandle,
) -> gpui::Stateful<gpui::Div> {
    keyboard_nav::focusable(
        div()
            .id(id)
            .role(gpui::Role::Button)
            .aria_label(label)
            .track_focus(&focus.clone().tab_index(0).tab_stop(true))
            .w(theme::scaled_px(36.))
            .h(theme::scaled_px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(theme().text_main))
            .rounded_md()
            .cursor_pointer()
            .when(selected, |el| el.bg(rgb(theme().surface)))
            .hover(|style| style.bg(rgb(theme().surface)))
            .when(!selected, |el| {
                el.tooltip(move |window, cx| {
                    Tooltip::element(move |_, _| {
                        super::super::e2e::measure_control(
                            format!("{id}-tooltip"),
                            div().child(label),
                        )
                    })
                    .build(window, cx)
                })
            })
            .child("»"),
    )
}
