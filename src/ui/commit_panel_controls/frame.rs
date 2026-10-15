//! Keyboard/accessibility frame shared by composer actions.
use crate::ui::{e2e, keyboard_nav};
use gpui::{div, prelude::*, px, App, ClickEvent, FocusHandle, Role, Window};
use kagi_ui_core::tooltip::HoverTooltip as _;

/// The named focus target extends outside the paint box without changing flow.
/// Negative vertical margins collapse the shrinking composer's intrinsic basis.
pub(super) fn action_frame(
    id: &'static str,
    slot: usize,
    label: &'static str,
    focus: FocusHandle,
    tab_stop: bool,
    disabled: bool,
    pressed: Option<bool>,
    reason: Option<gpui::SharedString>,
    content: impl IntoElement,
    activate: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    e2e::commit_panel_labels::focus(id, &focus);
    let disabled = e2e::commit_panel_labels::disabled(id, disabled);
    let pointer_focus = focus.clone();
    let frame = keyboard_nav::focusable(div().id(id))
        .absolute()
        .top(px(-keyboard_nav::RING))
        .bottom(px(-keyboard_nav::RING))
        .left(px(-keyboard_nav::RING))
        .right(px(-keyboard_nav::RING))
        .track_focus(&focus.tab_index(0).tab_stop(tab_stop))
        .rounded_sm()
        .role(e2e::commit_panel_labels::role(id, Role::Button))
        .aria_label(e2e::commit_panel_labels::name(id, label))
        .when_some(pressed, |el, pressed| {
            el.aria_toggled(if e2e::commit_panel_labels::pressed(id, pressed) {
                gpui::Toggled::True
            } else {
                gpui::Toggled::False
            })
        })
        .a11y_synthetic_children(move |builder: &mut gpui::A11ySubtreeBuilder| {
            if disabled {
                builder.parent_node().set_disabled();
            }
        })
        .when_some(reason.clone(), |el, reason| {
            el.aria_description(e2e::commit_panel_labels::description(id, reason))
        })
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            pointer_focus.focus(window, cx);
        })
        .on_click(move |event, window, cx| {
            cx.stop_propagation();
            if !disabled {
                activate(event, window, cx);
            }
        });
    let frame = if let Some(reason) = reason {
        frame
            .hover_tooltip_builder(("cp-control-help", slot), move |w, cx| {
                gpui_component::tooltip::Tooltip::new(reason.clone()).build(w, cx)
            })
            .into_any_element()
    } else {
        frame.into_any_element()
    };
    div()
        .id(("cp-control-layout", slot))
        .relative()
        .flex_shrink_0()
        .child(content)
        .child(frame)
}
