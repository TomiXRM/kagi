//! The input-driven plan modal (branch rename and friends).
//!
//! Split out of `modal_renderers.rs` when that file crossed the 800-LOC gate:
//! this card is the one that pairs a text input with a plan, and it shares
//! nothing with the plan card except the shell helpers.

use super::button_style::{modal_button, modal_icon_button, ModalButtonKind};
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
    div, prelude::*, rgb, Context, Entity, FocusHandle, KeyDownEvent, Role, SharedString, Window,
};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use kagi_domain::plan_note::{CommonNote, PlanNote, PushNote};
use kagi_git::{BranchRenameValidation, OperationPlan};
use kagi_ui_core::i18n::{plan::plan_heading_text, plan_note_text};
use kagi_ui_core::tooltip::HoverTooltip as _;

/// The input-confirm cards share form density and readable wrapping feedback,
/// while retaining their own plan and confirmation handlers.
pub(crate) fn render_input_modal_field(
    label: &'static str,
    state: Option<&Entity<InputState>>,
    reason: Option<SharedString>,
) -> gpui::Div {
    input_modal_field(
        label,
        state.map(|s| {
            super::e2e::measure_control(
                "input-modal-field",
                Input::new(s)
                    .context_menu(kagi_ui_core::input::context_menu(s, false))
                    .aria_label(label),
            )
        }),
        reason,
    )
}

/// #1043: a path field with the system folder dialog's button on its left.
/// The input keeps its height and gives up only the button's width.
pub(crate) fn render_input_modal_path_field(
    label: &'static str,
    state: Option<&Entity<InputState>>,
    reason: Option<SharedString>,
    button_id: &'static str,
    choose_folder: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Div {
    let row = state.map(|state| {
        // The measuring wrapper is the flex item in Tier A builds, so the
        // growing box sits outside it.
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(crate::ui::e2e::measure_control(
                button_id,
                modal_icon_button(
                    button_id,
                    gpui_component::IconName::FolderOpen,
                    Msg::InputChooseFolder.t(),
                    choose_folder,
                ),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(crate::ui::e2e::measure_control(
                        format!("{button_id}-input"),
                        Input::new(state)
                            .context_menu(kagi_ui_core::input::context_menu(state, false))
                            .aria_label(label),
                    )),
            )
            .into_any_element()
    });
    input_modal_field(label, row, reason)
}

fn input_modal_field(
    label: &'static str,
    control: Option<gpui::AnyElement>,
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
        .children(control);
    match reason {
        Some(reason) => field.child(crate::ui::e2e::measure_control(
            "input-field-error",
            div()
                .id(label)
                .role(Role::Alert)
                .aria_label(reason.clone())
                .text_sm()
                .text_color(rgb(current_theme().input_field_error_text()))
                .whitespace_normal()
                .child(reason.clone())
                .when(
                    label == Msg::InputBranchName.t() || label == Msg::InputNewBranchName.t(),
                    |error| {
                        error
                            .relative()
                            .child(super::e2e::measure_inside("input-branch-name-error"))
                    },
                )
                .hover_tooltip_builder((label, 0usize), move |window, cx| {
                    Tooltip::new(reason.clone()).build(window, cx)
                }),
        )),
        None => field,
    }
}

/// Both creation sheets keep pristine empty-name requests quiet without
/// removing the plan's blocker or inventing another branch-name validator.
pub(crate) fn create_branch_name_reason(
    plan: Option<&OperationPlan>,
    name: &str,
    interacted: bool,
) -> Option<SharedString> {
    if name.is_empty() && !interacted {
        return None;
    }
    plan?.blockers.iter().find_map(|note| {
        matches!(note, PlanNote::Common(CommonNote::BranchNameErrorKeyed(_)))
            .then(|| SharedString::from(plan_note_text(note)))
    })
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
                    .child(target.clone())
                    .hover_tooltip_builder("input-modal-target-help", move |window, cx| {
                        Tooltip::new(target.clone()).build(window, cx)
                    }),
            )
        }
        None => heading,
    }
}

// Existing input-plan renderer contract shared by Rename and Set Upstream.
/// Field presentation is distinct from backend rename validation.
pub(crate) enum InputPlanValidation {
    Unchanged,
    Rename(BranchRenameValidation),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_input_plan_modal(
    short_title: &'static str,
    heading_target: &str,
    title: String,
    label: &'static str,
    input_state: Option<Entity<InputState>>,
    plan: Option<std::sync::Arc<OperationPlan>>,
    validation: Option<InputPlanValidation>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    accent: PlanCardAccent,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    overrides: &super::modal_shell::ModalSections,
    focus: Option<&FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let field_reason = match validation.as_ref() {
        Some(InputPlanValidation::Rename(BranchRenameValidation::Invalid(reason))) => Some(
            SharedString::from(crate::ui::i18n::branch_name_error(reason)),
        ),
        _ => plan.as_ref().and_then(|p| {
            p.blockers
                .iter()
                .find(|note| matches!(note, PlanNote::Push(PushNote::UpstreamFormatInvalid)))
                .map(|note| SharedString::from(plan_note_text(note)))
        }),
    };
    let has_blockers =
        field_reason.is_some() || plan.as_ref().is_none_or(|p| !p.blockers.is_empty());
    let disabled_reason = if matches!(validation, Some(InputPlanValidation::Unchanged)) {
        Some(Msg::InputRenameEnterNewName.t().into())
    } else if has_blockers {
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
    let title = SharedString::from(title);
    let card = modal_card(MODAL_W_MD).child(
        render_plan_heading(heading_title, chips, (icon, color), None)
            .id("input-plan-heading")
            .role(Role::Heading)
            .aria_label(title.clone()),
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
                // Rename validation already explains this exact cause at the field.
                if matches!(
                    (validation.as_ref(), blocker),
                    (
                        Some(InputPlanValidation::Rename(BranchRenameValidation::Invalid(reason))),
                        PlanNote::Common(CommonNote::BranchNameErrorKeyed(plan_reason))
                    ) if reason == plan_reason
                ) {
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

    let buttons = super::button_style::modal_button_row()
        .child(modal_button(
            "branch-input-cancel",
            Msg::PlanCancel.t(),
            ModalButtonKind::Cancel,
            None,
            cancel_handler,
            cx,
        ))
        .child(crate::ui::e2e::measure_confirm(modal_button(
            "branch-input-confirm",
            confirm_label,
            ModalButtonKind::Primary,
            disabled_reason,
            confirm_handler,
            cx,
        )));
    let card = card.child(body).child(div().flex_shrink_0().child(buttons));

    modal_overlay(super::modal_renderers::input_dialog_frame(
        "branch-input-card",
        title.as_ref(),
        (!has_blockers).then_some(confirm_label),
        card,
        focus,
        |frame| frame,
        cx,
    ))
    .into_any_element()
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
    let mut body = modal_scroll_body()
        .child(div().flex_shrink_0().child(SharedString::from(modal.name)))
        .child(div().flex_shrink_0().child(Msg::WorktreeLockReason.t()))
        .children(modal.input_state.as_ref().map(|input| {
            measure_control(
                "worktree-lock-reason-input",
                Input::new(input)
                    .context_menu(kagi_ui_core::input::context_menu(input, false))
                    .aria_label(Msg::WorktreeLockReason.t()),
            )
        }));
    if let Some(error) = modal.error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_color(rgb(current_theme().color_blocker))
                .child(error),
        );
    }
    let buttons = super::button_style::modal_button_row()
        .flex_shrink_0()
        .child(measure_control(
            "worktree-lock-reason-cancel",
            modal_button(
                "worktree-lock-reason-cancel",
                Msg::PlanCancel.t(),
                ModalButtonKind::Cancel,
                None,
                cancel,
                cx,
            ),
        ))
        .child(measure_control(
            "worktree-lock-reason-review",
            modal_button(
                "worktree-lock-reason-review",
                Msg::WorktreeLockReview.t(),
                ModalButtonKind::Primary,
                None,
                review,
                cx,
            ),
        ));
    let card = modal_card(MODAL_W_MD)
        .child(div().flex_shrink_0().child(render_modal_title_row(
            SharedString::from(Msg::MenuLockWorktree.t()),
            Some((
                ModalIcon::Path("icons/lock-keyhole.svg"),
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
    modal_overlay(super::modal_renderers::input_dialog_frame(
        "worktree-lock-reason-card",
        Msg::MenuLockWorktree.t(),
        Some(Msg::WorktreeLockReview.t()),
        div().on_key_down(keys).child(card),
        focus_handle.as_ref(),
        |frame| frame,
        cx,
    ))
    .into_any_element()
}
