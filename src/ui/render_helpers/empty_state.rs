//! Shared icon, explanatory copy and one primary action for open empty states.
use super::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName};

pub(crate) fn empty_state(
    id: &'static str,
    probes: [&'static str; 4],
    title: SharedString,
    hint: SharedString,
    icon: IconName,
    icon_color: u32,
    action: impl IntoElement,
) -> gpui::AnyElement {
    div()
        .id(id)
        .flex_1()
        .min_h(px(0.))
        .min_w(px(0.))
        .w_full()
        .overflow_y_scroll()
        .bg(rgb(theme().panel))
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .p_3()
        .child(e2e::measure_inside(id))
        .child(e2e::measure_control(
            probes[0],
            Icon::new(icon)
                .size(theme::scaled_px(20.))
                .text_color(rgb(icon_color)),
        ))
        .child(
            div()
                .relative()
                .w_full()
                .flex_shrink_0()
                .text_center()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(theme().text_main))
                .child(e2e::measure_inside(probes[1]))
                .child(title),
        )
        .child(
            div()
                .id(probes[2])
                .relative()
                .w_full()
                .min_h(px(0.))
                .overflow_y_scroll()
                .text_center()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(e2e::measure_inside(probes[2]))
                .child(hint),
        )
        .child(e2e::measure_control(probes[3], action))
        .into_any()
}

impl KagiApp {
    pub(crate) fn render_unborn_empty_state(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let title: SharedString = Msg::UnbornEmptyTitle.t().into();
        let hint: SharedString = Msg::UnbornEmptyHint.t().into();
        let name = "Changes";
        let action = Button::new("unborn-open-changes")
            .primary()
            .label(name)
            .on_click(cx.listener(|app, _, window, cx| {
                app.open_commit_panel(window, cx);
                cx.notify();
            }));
        #[cfg(feature = "gui-e2e")]
        e2e::unborn::record(&title, &hint, name);
        #[cfg(feature = "gui-e2e")]
        let action = e2e::unborn::ChangesButton(action);
        empty_state(
            "unborn-empty-state",
            [
                "unborn-state-icon",
                "unborn-state-title",
                "unborn-state-hint",
                "unborn-open-changes",
            ],
            title,
            hint,
            IconName::Inbox,
            theme().text_sub,
            action,
        )
    }
}
