//! The input-driven plan modal (branch rename and friends).
//!
//! Split out of `modal_renderers.rs` when that file crossed the 800-LOC gate:
//! this card is the one that pairs a text input with a plan, and it shares
//! nothing with the plan card except the shell helpers.

use super::i18n::Msg;
use super::modal_renderers::{
    modal_overlay, render_current_predicted, render_modal_title_row, PlanCardAccent,
};
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_MD};
use super::theme::theme as current_theme;
use gpui::{div, prelude::*, rgb, Entity, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::Sizable as _;
use kagi_git::{BranchRenameValidation, OperationPlan};
use kagi_ui_core::i18n::plan_note_text;

pub(crate) fn render_input_plan_modal(
    title: String,
    label: &'static str,
    input_state: Option<Entity<InputState>>,
    plan: Option<std::sync::Arc<OperationPlan>>,
    validation: Option<BranchRenameValidation>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    accent: Option<PlanCardAccent>,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
    // #454 layer 4: adopt the shared shell — fixed title, scrolling middle,
    // fixed button row. Plan notes are unbounded (a rename can carry many
    // warnings/blockers), so the body is this card's single scroll region.
    let card = modal_card(MODAL_W_MD).child(div().flex_shrink_0().child(render_modal_title_row(
        SharedString::from(title),
        accent.clone(),
    )));
    let mut body = modal_scroll_body().child(
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().text_label))
                    .child(SharedString::from(label)),
            )
            .children(input_state.as_ref().map(|st| Input::new(st).small())),
    );

    if let Some(BranchRenameValidation::Invalid(reason)) = validation {
        // W29-I18N-WAVE2: localize the keyed branch-name reason.
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(SharedString::from(crate::ui::i18n::branch_name_error(
                    &reason,
                ))),
        );
    }

    if let Some(plan) = plan {
        body = body.child(
            div()
                .flex_shrink_0()
                .child(render_current_predicted(&plan, accent.clone())),
        );

        if !plan.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for warning in &plan.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{26a0} {}",
                            plan_note_text(warning)
                        ))),
                );
            }
            body = body.child(warn_col.flex_shrink_0());
        }
        if !plan.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for blocker in &plan.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{2717} {}",
                            plan_note_text(blocker)
                        ))),
                );
            }
            body = body.child(block_col.flex_shrink_0());
        }
    }

    if let Some(err) = error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err),
        );
    }

    let mut buttons = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("branch-input-cancel")
            .label(Msg::PlanCancel.t())
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if !has_blockers {
        buttons = buttons.child(
            Button::new("branch-input-confirm")
                .label(SharedString::from(confirm_label))
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }
    let card = card.child(body).child(div().flex_shrink_0().child(buttons));

    modal_overlay(card).into_any_element()
}
