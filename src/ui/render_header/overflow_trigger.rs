use super::*;
use gpui_component::tooltip::Tooltip;
use kagi_ui_core::tooltip::HoverTooltip as _;

/// Shared named, ringed » affordance for toolbar and document overflow.
pub(crate) fn trigger(
    id: &'static str,
    label: &'static str,
    selected: bool,
    focus: &gpui::FocusHandle,
    configure: impl FnOnce(gpui::Stateful<gpui::Div>) -> gpui::Stateful<gpui::Div>,
) -> gpui::AnyElement {
    let trigger = configure(keyboard_nav::focusable(
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
            .child("»"),
    ));
    if selected {
        trigger.into_any_element()
    } else {
        trigger
            .hover_tooltip_builder((id, 0usize), move |window, cx| {
                Tooltip::element(move |_, _| {
                    super::super::e2e::measure_control(format!("{id}-tooltip"), div().child(label))
                })
                .build(window, cx)
            })
            .into_any_element()
    }
}
