//! Collapsed CLI command disclosures shared by plan and restore cards.
//! Kagi performs its safe operation itself; equivalent commands are reference text.

use super::i18n::Msg;
use super::modal_copy::modal_copy_button;
use super::modal_shell::section_open;
use super::theme::{self, theme as current_theme};
use super::{KagiApp, MONO_FONT};
use gpui::{div, prelude::*, rgb, Context, SharedString};
use kagi_domain::plan::OperationPlan;
use kagi_domain::plan_note::PlanDisposition;
use kagi_ui_core::i18n::oplog_panel::{self, OplogPanelMsg};

/// A blocked or no-op plan must not offer a command that looks executable.
/// Both card renderers and Copy all use this same gate.
pub(crate) fn plan_ready(plan: &OperationPlan) -> bool {
    plan.disposition == PlanDisposition::Ready && plan.blockers.is_empty()
}

pub(crate) fn equivalent_command(plan: &OperationPlan) -> Option<&str> {
    if plan_ready(plan) {
        plan.equivalent_command.as_deref()
    } else {
        None
    }
}

const SECTION_EQUIVALENT_COMMAND: &str = "plan-equivalent-command";

struct CommandDisclosure<'a> {
    id: &'static str,
    copy_id: &'static str,
    body_id: &'static str,
    summary: SharedString,
    ax_label: SharedString,
    text: &'a str,
}

pub(crate) fn render_equivalent_command(
    cmd: &str,
    lines: Option<usize>,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_command_disclosure(
        CommandDisclosure {
            id: SECTION_EQUIVALENT_COMMAND,
            copy_id: "plan-equivalent-command-copy",
            body_id: "plan-equivalent-command-body",
            summary: lines
                .map(oplog_panel::restore_command_lines)
                .unwrap_or_else(|| cmd.to_owned())
                .into(),
            ax_label: format!("{} {cmd}", Msg::ModalEquivalentCommand.t()).into(),
            text: cmd,
        },
        overrides,
        cx,
    )
}

pub(crate) fn render_recovery_commands(
    commands: &[String],
    id: &'static str,
    copy_id: &'static str,
    body_id: &'static str,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let text = commands.join("\n");
    let summary = if commands.len() == 1 {
        commands[0].clone()
    } else {
        format!("{} · +{}", commands[0], commands.len() - 1)
    };
    let label = format!("{} {text}", Msg::ModalRecoveryCommands.t());
    render_command_disclosure(
        CommandDisclosure {
            id,
            copy_id,
            body_id,
            summary: summary.into(),
            ax_label: label.into(),
            text: &text,
        },
        overrides,
        cx,
    )
}

fn render_command_disclosure(
    spec: CommandDisclosure<'_>,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let CommandDisclosure {
        id,
        copy_id,
        body_id,
        summary,
        ax_label,
        text,
    } = spec;
    let open = section_open(overrides, id, false).is_open();
    #[cfg(feature = "gui-e2e")]
    DISCLOSURES.with(|items| {
        items.borrow_mut().insert(
            id,
            (summary.to_string(), ax_label.to_string(), open),
        );
    });
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .min_w(gpui::px(0.))
        .child(
            super::keyboard_nav::focusable(
                div()
                    .id(id)
                    .relative()
                    .flex_1()
                    .min_w(gpui::px(0.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .role(gpui::Role::Button)
                    .aria_label(ax_label)
                    .aria_expanded(open)
                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                        this.toggle_modal_section(id, open);
                        cx.notify();
                    })),
            )
            .child(if open { "▾" } else { "▸" })
            .child(
                div()
                    .min_w(gpui::px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(MONO_FONT)
                    .child(summary),
            )
            .child(super::e2e::measure_inside(id)),
        )
        .child(modal_copy_button(
            copy_id,
            Msg::OplogPanel(OplogPanelMsg::CopyCommand).t(),
            text.to_owned(),
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
                    .id(body_id)
                    .relative()
                    .max_h(theme::scaled_px(120.))
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .rounded_sm()
                    .bg(gpui::rgba(theme::panel_style().0))
                    .p_1()
                    .font_family(MONO_FONT)
                    .text_color(rgb(current_theme().text_main))
                    .child(SharedString::from(text.to_owned()))
                    .child(super::e2e::measure_inside(body_id)),
            )
        })
        .into_any_element()
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static DISCLOSURES: std::cell::RefCell<std::collections::HashMap<&'static str, (String, String, bool)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Renderer-set visible summary, accessible name and expanded state.
#[cfg(feature = "gui-e2e")]
pub(crate) fn recorded_disclosure(id: &str) -> Option<(String, String, bool)> {
    DISCLOSURES.with(|items| items.borrow().get(id).cloned())
}
