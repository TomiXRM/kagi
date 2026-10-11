//! Analyze Help follows the named task-dialog and single-modal contracts.
use super::button_style::{modal_button, ModalButtonKind};
use super::modal_renderers::{input_dialog_frame, modal_overlay};
use super::*;
use gpui::AnyElement;

pub(super) fn render_analyze_help(
    focus: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> AnyElement {
    if let Some(focus) = &focus {
        if !focus.is_focused(window) && !focus.contains_focused(window, cx) {
            window.focus(focus, cx);
        }
    }
    let close = modal_button(
        "eco-help-close",
        Msg::RemoteClose.t(),
        ModalButtonKind::Cancel,
        None,
        cx.listener(|app, _, _, cx| {
            app.clear_analyze_help_modal();
            cx.notify();
        }),
        cx,
    );
    let card = super::modal_shell::modal_card(super::modal_shell::MODAL_W_MD)
        .occlude()
        .child(
            div()
                .flex_shrink_0()
                .text_size(theme::scaled_px(16.))
                .text_color(rgb(theme().text_main))
                .child(Msg::EcoHelpTitle.t()),
        )
        .child(help_body())
        .child(
            super::button_style::modal_button_row()
                .child(super::e2e::measure_control("eco-help-close", close)),
        );
    let frame = input_dialog_frame(
        "eco-help-card",
        Msg::EcoHelpTitle.t(),
        None,
        card,
        focus.as_ref(),
        |frame| {
            #[cfg(feature = "gui-e2e")]
            kagi_ui_ecosystem::e2e::record(
                "eco-help-card",
                kagi_ui_ecosystem::e2e::Node {
                    role: gpui::Role::Dialog,
                    name: Msg::EcoHelpTitle.t().into(),
                    text: Msg::EcoHelpTitle.t().into(),
                    selected: None,
                    expanded: None,
                    disabled: false,
                },
            );
            frame
                .relative()
                .child(super::e2e::measure_inside("eco-help-card"))
        },
        cx,
    );
    modal_overlay(frame).into_any_element()
}

fn help_body() -> gpui::Stateful<gpui::Div> {
    let sections = kagi_ui_core::i18n::eco_help_sections();
    let count = sections.len();
    let mut body = super::modal_shell::modal_scroll_body()
        .relative()
        .child(super::e2e::measure_inside("eco-help-body"));
    for (index, (heading, text)) in sections.into_iter().enumerate() {
        body = body.child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(theme::scaled_px(13.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme().text_main))
                        .child(heading),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(12.))
                        .text_color(rgb(theme().text_sub))
                        .child(text),
                )
                .when(index + 1 == count, |el| {
                    el.child(super::e2e::measure_inside("eco-help-last"))
                }),
        );
    }
    body
}
