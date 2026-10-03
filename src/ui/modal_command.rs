//! A compact equivalent-CLI disclosure shared by plan and restore cards.
//! The application performs its own safe operation; this command is reference text.

use super::i18n::Msg;
use super::modal_copy::modal_copy_button;
use super::modal_shell::section_open;
use super::theme::{self, theme as current_theme};
use super::{KagiApp, MONO_FONT};
use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_ui_core::i18n::oplog_panel::{self, OplogPanelMsg};

const SECTION_EQUIVALENT_COMMAND: &str = "plan-equivalent-command";

pub(crate) fn render_equivalent_command(
    cmd: &str,
    lines: Option<usize>,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let open = section_open(overrides, SECTION_EQUIVALENT_COMMAND, false).is_open();
    let summary = match lines {
        Some(count) => oplog_panel::restore_command_lines(count),
        None => Msg::PlanEquivalentTo.t().replace("{}", cmd),
    };
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .min_w(gpui::px(0.))
        .child(
            div()
                .id(SECTION_EQUIVALENT_COMMAND)
                .relative()
                .flex_1()
                .min_w(gpui::px(0.))
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .role(gpui::Role::Group)
                .aria_label(SharedString::from(format!("{summary}: {cmd}")))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _ev, _window, cx| {
                        this.toggle_modal_section(SECTION_EQUIVALENT_COMMAND, open);
                        cx.notify();
                    }),
                )
                .child(if open { "▾" } else { "▸" })
                .child(
                    div()
                        .min_w(gpui::px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(summary)),
                )
                .child(super::e2e::measure_inside(SECTION_EQUIVALENT_COMMAND)),
        )
        .child(modal_copy_button(
            "plan-equivalent-command-copy",
            Msg::OplogPanel(OplogPanelMsg::CopyCommand).t(),
            cmd.to_owned(),
            cx,
        ));
    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_1()
        .text_xs()
        .text_color(rgb(current_theme().text_main))
        .child(header)
        .when(open, |section| {
            section.child(
                div()
                    .id("plan-equivalent-command-body")
                    .relative()
                    .max_h(theme::scaled_px(120.))
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .rounded_sm()
                    .bg(gpui::rgba(theme::panel_style().0))
                    .p_1()
                    .font_family(MONO_FONT)
                    .text_color(rgb(current_theme().text_main))
                    .child(SharedString::from(cmd.to_owned()))
                    .child(super::e2e::measure_inside("plan-equivalent-command-body")),
            )
        })
        .into_any_element()
}
