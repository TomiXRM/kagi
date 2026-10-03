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
use gpui::{
    div, prelude::*, rgb, Context, Entity, FocusHandle, Hsla, KeyDownEvent, Role, SharedString,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Sizable as _};
use kagi_domain::plan_note::{PlanNote, PushNote};
use kagi_git::{BranchRenameValidation, OperationPlan};
use kagi_ui_core::i18n::plan_note_text;

/// The input-confirm cards share form density, but keep their own plan and
/// confirmation handlers. A field error is one line here; the full reason
/// remains available to pointer and assistive-technology users.
pub(crate) fn render_input_modal_field(
    label: &'static str,
    state: Option<&Entity<InputState>>,
    reason: Option<SharedString>,
) -> gpui::Div {
    let field = div()
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
        .children(state.map(Input::new));
    match reason {
        Some(reason) => field.child(
            div()
                .id(label)
                .role(Role::Alert)
                .aria_label(reason.clone())
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .truncate()
                .tooltip({
                    let reason = reason.clone();
                    move |window, cx| Tooltip::new(reason.clone()).build(window, cx)
                })
                .child(reason),
        ),
        None => field,
    }
}

pub(crate) fn render_input_modal_heading(
    title: &'static str,
    target: Option<(&str, &str)>,
) -> gpui::Div {
    let heading = div().flex_shrink_0().flex().flex_col().gap_1().child(
        div()
            .text_lg()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(current_theme().text_main))
            .child(SharedString::from(title)),
    );
    match target {
        Some((short_sha, summary)) => {
            let target = SharedString::from(format!("{short_sha}  {summary}"));
            heading.child(
                div()
                    .id("input-modal-target")
                    .text_sm()
                    .text_color(rgb(current_theme().text_sub))
                    .truncate()
                    .aria_label(target.clone())
                    .tooltip({
                        let target = target.clone();
                        move |window, cx| Tooltip::new(target.clone()).build(window, cx)
                    })
                    .child(target),
            )
        }
        None => heading,
    }
}

/// gpui-component's disabled Button suppresses clicks and focus but exposes an
/// enabled AX node. Draw the unavailable state as an inert, reason-bearing AX
/// Button instead. Ready actions keep the actual gpui-component Button.
pub(crate) fn render_input_modal_action(
    button: impl FnOnce() -> Button,
    label: &'static str,
    accent: u32,
    reason: Option<SharedString>,
    cx: &gpui::App,
) -> gpui::AnyElement {
    match reason {
        Some(reason) => div()
            .id(label)
            .role(Role::Button)
            .aria_label(SharedString::from(label))
            .aria_description(reason.clone())
            .a11y_synthetic_children(|builder: &mut gpui::A11ySubtreeBuilder| {
                builder.parent_node().set_disabled();
            })
            .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .h_8()
            .px_4()
            .rounded(cx.theme().radius)
            .bg(Hsla::from(rgb(accent)).opacity(0.15))
            .text_color(cx.theme().muted_foreground.opacity(0.5))
            .child(label)
            .into_any_element(),
        None => button().into_any_element(),
    }
}

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
    cx: &gpui::App,
) -> gpui::AnyElement {
    let field_reason = match validation.as_ref() {
        Some(BranchRenameValidation::Invalid(reason)) => Some(SharedString::from(
            crate::ui::i18n::branch_name_error(reason),
        )),
        _ => plan.as_ref().and_then(|p| {
            p.blockers
                .iter()
                .find(|note| matches!(note, PlanNote::Push(PushNote::UpstreamFormatInvalid)))
                .map(|note| SharedString::from(plan_note_text(note)))
        }),
    };
    let has_blockers =
        field_reason.is_some() || plan.as_ref().is_none_or(|p| !p.blockers.is_empty());
    let disabled_reason = if has_blockers {
        error.clone().or_else(|| field_reason.clone()).or_else(|| {
            plan.as_ref()
                .and_then(|p| p.blockers.first())
                .map(|b| SharedString::from(plan_note_text(b)))
                .or_else(|| Some(Msg::InputPlanPending.t().into()))
        })
    } else {
        None
    };
    // #454 layer 4: adopt the shared shell — fixed title, scrolling middle,
    // fixed button row. Plan notes are unbounded (a rename can carry many
    // warnings/blockers), so the body is this card's single scroll region.
    let card = modal_card(MODAL_W_MD).child(div().flex_shrink_0().child(render_modal_title_row(
        SharedString::from(title),
        accent.clone(),
    )));
    let mut body = modal_scroll_body().child(render_input_modal_field(
        label,
        input_state.as_ref(),
        field_reason,
    ));

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

    let buttons = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            Button::new("branch-input-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .on_click(cancel_handler),
        )
        .child(crate::ui::e2e::measure_confirm(render_input_modal_action(
            || {
                Button::new("branch-input-confirm")
                    .label(SharedString::from(confirm_label))
                    .primary()
                    .on_click(confirm_handler)
            },
            confirm_label,
            current_theme().color_branch,
            disabled_reason,
            cx,
        )));
    let card = card.child(body).child(div().flex_shrink_0().child(buttons));

    modal_overlay(card).into_any_element()
}

/// Input-only lock step; the existing plan card owns the final confirmation.
pub(crate) fn render_worktree_lock_reason_modal(
    modal: super::modals::worktree::WorktreeLockReasonModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<super::KagiApp>,
) -> gpui::AnyElement {
    use super::e2e::measure_control;
    let cancel = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
        this.clear_worktree_lock_reason_modal();
        if let Some(focus) = &this.root_focus {
            window.focus(focus, cx);
        }
        cx.notify();
    });
    let review = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
        this.confirm_worktree_lock_reason(cx);
        if this.lock_worktree_modal().is_some() {
            if let Some(focus) = &this.root_focus {
                window.focus(focus, cx);
            }
        }
        cx.notify();
    });
    let mut body =
        modal_scroll_body()
            .child(div().flex_shrink_0().child(SharedString::from(modal.name)))
            .child(div().flex_shrink_0().child(Msg::WorktreeLockReason.t()))
            .children(modal.input_state.as_ref().map(|input| {
                measure_control("worktree-lock-reason-input", Input::new(input).small())
            }));
    if let Some(error) = modal.error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_color(rgb(current_theme().color_blocker))
                .child(error),
        );
    }
    let buttons = div()
        .flex_shrink_0()
        .flex()
        .gap_2()
        .justify_end()
        .child(measure_control(
            "worktree-lock-reason-cancel",
            Button::new("worktree-lock-reason-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .small()
                .on_click(cancel),
        ))
        .child(measure_control(
            "worktree-lock-reason-review",
            Button::new("worktree-lock-reason-review")
                .label(Msg::WorktreeLockReview.t())
                .primary()
                .small()
                .on_click(review),
        ));
    let card = modal_card(MODAL_W_MD)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(Msg::MenuLockWorktree.t()),
            Some((
                gpui_component::IconName::WindowRestore.into(),
                current_theme().color_warning,
            )),
        )))
        .child(body)
        .child(buttons);
    let keys = cx.listener(|this, event: &KeyDownEvent, window, cx| {
        let key = &event.keystroke;
        if key.key == "escape" {
            this.clear_worktree_lock_reason_modal();
        } else if key.key == "enter"
            && !key.modifiers.platform
            && !key.modifiers.control
            && !key.modifiers.alt
            && !key.modifiers.shift
        {
            this.confirm_worktree_lock_reason(cx);
        } else {
            return;
        }
        // Replacing the input drops its focus handle. Move focus before the
        // next key, and never let this Enter also execute the new plan.
        if this.worktree_lock_reason_modal().is_none() {
            if let Some(focus) = &this.root_focus {
                window.focus(focus, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    });
    let wrapper = div().on_key_down(keys);
    let wrapper = match focus_handle {
        Some(focus) => wrapper.track_focus(&focus),
        None => wrapper,
    };
    modal_overlay(wrapper.child(card)).into_any_element()
}
