//! The input-driven plan modal (branch rename and friends).
//!
//! Split out of `modal_renderers.rs` when that file crossed the 800-LOC gate:
//! this card is the one that pairs a text input with a plan, and it shares
//! nothing with the plan card except the shell helpers.

use super::i18n::Msg;
use super::modal_command::{plan_ready, render_recovery_commands};
use super::modal_renderers::{
    modal_overlay, render_current_predicted, render_modal_title_row, render_plan_heading,
    ModalIcon, PlanCardAccent,
};
use super::modal_renderers_plan::offered_recovery_commands;
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_MD};
use super::theme::theme as current_theme;
use super::KagiApp;
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
use kagi_ui_core::i18n::{plan::plan_heading_text, plan_note_text};

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
        Some(reason) => field.child(crate::ui::e2e::measure_control(
            "input-field-error",
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
        )),
        None => field,
    }
}

pub(crate) fn render_input_modal_heading(
    title: impl Into<SharedString>,
    target: Option<(&str, &str)>,
    icon: ModalIcon,
    color: u32,
) -> gpui::Div {
    let heading = div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(render_modal_title_row(title.into(), Some((icon, color))));
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

/// Size of the shared action, including its inert disabled representation.
#[derive(Clone, Copy)]
pub(crate) enum InputActionSize {
    Regular,
    Small,
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
    render_input_modal_action_with_size(button, label, accent, reason, InputActionSize::Regular, cx)
}

pub(crate) fn render_input_modal_action_with_size(
    button: impl FnOnce() -> Button,
    label: &'static str,
    accent: u32,
    reason: Option<SharedString>,
    size: InputActionSize,
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
            .when(matches!(size, InputActionSize::Small), |button| {
                button.h_6().px_3().text_xs()
            })
            .rounded(cx.theme().radius)
            .bg(Hsla::from(rgb(accent)).opacity(0.15))
            .text_color(cx.theme().muted_foreground.opacity(0.5))
            .child(label)
            .into_any_element(),
        None => button().into_any_element(),
    }
}

pub(crate) fn render_input_plan_modal(
    short_title: &'static str,
    heading_target: &str,
    title: String,
    label: &'static str,
    input_state: Option<Entity<InputState>>,
    plan: Option<std::sync::Arc<OperationPlan>>,
    validation: Option<BranchRenameValidation>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    accent: PlanCardAccent,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
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
    // Plan notes are unbounded, so only this card's middle body scrolls.
    let (icon, color) = accent;
    let (heading_title, mut chips) = plan
        .as_ref()
        .map(|plan| plan_heading_text(&plan.title))
        .unwrap_or((short_title, [None, None]));
    if chips[0].is_none() {
        chips[0] = Some(std::borrow::Cow::Borrowed(heading_target));
    }
    let card = modal_card(MODAL_W_MD).child(
        render_plan_heading(heading_title, chips, (icon, color), None)
            .id("input-plan-heading")
            .role(Role::Heading)
            .aria_label(SharedString::from(title)),
    );
    let mut body = modal_scroll_body().child(render_input_modal_field(
        label,
        input_state.as_ref(),
        field_reason,
    ));

    let input_valid = input_state
        .as_ref()
        .is_some_and(|state| !state.read(cx).value().trim().is_empty());
    if let Some(plan) = plan {
        body = body.child(div().flex_shrink_0().child(render_current_predicted(&plan)));

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
            let mut displayed = false;
            for blocker in &plan.blockers {
                // The upstream format blocker is already the field's error.
                if matches!(blocker, PlanNote::Push(PushNote::UpstreamFormatInvalid)) {
                    continue;
                }
                displayed = true;
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
            if displayed {
                body = body.child(crate::ui::e2e::measure_control(
                    "input-plan-blockers",
                    block_col.flex_shrink_0(),
                ));
            }
        }
        if input_valid && !has_blockers && plan_ready(&plan) {
            if let Some(commands) = offered_recovery_commands(plan.recovery.as_ref()) {
                body = body.child(div().flex_shrink_0().child(render_recovery_commands(
                    commands,
                    "input-recovery",
                    "input-recovery-copy",
                    "input-recovery-body",
                    overrides,
                    cx,
                )));
            }
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
