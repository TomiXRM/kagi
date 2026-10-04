//! Stash push / apply modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). Pure physical move — behaviour
//! unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::{modal_button, ModalButtonKind};
use super::dialog_a11y::{apply_dialog, dialog_a11y, ConfirmStage, DialogHandler};
use super::i18n::Msg;
use super::modal_command::{plan_ready, render_recovery_commands};
use super::modal_copy::{modal_copy_button, plan_clipboard_text};
use super::modal_renderers::{modal_overlay, render_current_predicted, render_plan_heading};
use super::modal_renderers_input::render_input_modal_heading;
use super::modal_renderers_plan::{offered_recovery_commands, render_input_recovery_commands};
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_MD};
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, FocusHandle, KeyDownEvent, SharedString};
use gpui_component::{Icon, IconName};
use kagi_domain::plan_note::{PlanNote, ShellKind, StashNote};
use kagi_ui_core::i18n::{
    plan::plan_heading_text, plan_note_text, plan_recovery_text, plan_title_text,
};
use std::rc::Rc;

// ──────────────────────────────────────────────────────────────
// Stash push modal renderer (T015)
// ──────────────────────────────────────────────────────────────

/// Render the stash push confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Optional message text input (reuses T014 key-input pattern)
///   - Live plan: Current → Predicted state
///   - Warnings (yellow) if any
///   - Blockers (red) if any
///   - Error message (if execute failed)
///   - `[Cancel]` and an always-visible, disabled-until-ready `[Stash]`
pub(crate) fn render_stash_push_modal(
    modal: StashPushModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_push_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_push(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // A message is optional. The placeholder describes the input; the group
    // label stays available to assistive technology without a second visual
    // label pushing the state preview away from the title.
    let card = modal_card(MODAL_W_MD)
        .bg(rgb(current_theme().bg_base))
        .border_color(rgb(current_theme().surface))
        .rounded(theme::scaled_px(14.))
        .when(!super::modal_shell::modal_compact(), |card| {
            card.p(theme::scaled_px(24.)).gap(theme::scaled_px(18.))
        })
        .child(render_input_modal_heading(
            Msg::InputStashTitle.t(),
            None,
            IconName::Inbox.into(),
            current_theme().color_warning,
        ));
    let mut body = modal_scroll_body()
        .when(!super::modal_shell::modal_compact(), |body| {
            body.gap(theme::scaled_px(18.))
        })
        .child(
            div()
                .id("stash-push-message")
                .role(gpui::Role::Group)
                .aria_label(Msg::InputStashMessage.t())
                .children(modal.input_state.as_ref().map(|state| {
                    gpui_component::input::Input::new(state)
                        .h(theme::scaled_px(32.))
                        .rounded(theme::scaled_px(8.))
                        .bg(rgb(current_theme().panel))
                        .text_size(theme::scaled_px(14.))
                })),
        );

    // ── Plan state (current → predicted) ─────────────────
    if let Some(ref p) = plan {
        body = body.child(
            div()
                .w_full()
                .flex_shrink_0()
                .child(render_current_predicted(p)),
        );

        // ── Warnings ──────────────────────────────────────
        if !p.warnings.is_empty() {
            let t = current_theme();
            let warning_color =
                gpui::rgba((t.change_modified << 8) | if t.dark { 0xd9 } else { 0xff });
            let mut warn_col = div().flex().flex_col().gap_1();
            for (index, w) in p.warnings.iter().enumerate() {
                let full = SharedString::from(plan_note_text(w));
                let short = match w {
                    PlanNote::Stash(StashNote::UntrackedIncluded { count }) => SharedString::from(
                        Msg::InputStashUntrackedWarning
                            .t()
                            .replace("{}", &count.to_string()),
                    ),
                    _ => full.clone(),
                };
                warn_col = warn_col.child(
                    div()
                        .id(("stash-push-warning", index))
                        .role(gpui::Role::Alert)
                        .aria_label(full.clone())
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                        })
                        .flex()
                        .items_start()
                        .gap_2()
                        .text_sm()
                        .text_color(warning_color)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size_4()
                                .flex_shrink_0()
                                .mt(theme::scaled_px(3.)),
                        )
                        .child(short),
                );
            }
            body = body.child(warn_col.flex_shrink_0());
        }

        // ── Blockers ──────────────────────────────────────
        if !p.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for b in &p.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{2717} {}",
                            plan_note_text(b)
                        ))),
                );
            }
            body = body.child(block_col.flex_shrink_0());
        }

        // A blank message is valid for the operation, but has no recovery
        // preview. Keep guidance command-only when a message and plan are ready.
        let message_filled = modal
            .input_state
            .as_ref()
            .is_some_and(|state| !state.read(cx).value().trim().is_empty());
        if message_filled && super::modal_command::plan_ready(p) {
            if let Some(commands) = offered_recovery_commands(p.recovery.as_ref()) {
                body = body.child(div().flex_shrink_0().child(render_input_recovery_commands(
                    commands,
                    current_theme().color_warning,
                )));
            }
        }
    }

    // ── Error message ──────────────────────────────────
    if let Some(ref err) = modal.error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let disabled_reason = if has_blockers {
        modal.error.clone().or_else(|| {
            plan.as_ref()
                .and_then(|p| p.blockers.first())
                .map(|b| SharedString::from(plan_note_text(b)))
                .or_else(|| Some(Msg::InputPlanPending.t().into()))
        })
    } else {
        None
    };
    let button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(modal_button(
            "stash-push-cancel",
            Msg::PlanCancel.t(),
            ModalButtonKind::Cancel,
            None,
            cancel_handler,
            cx,
        ))
        .child(crate::ui::e2e::measure_confirm(modal_button(
            "stash-push-confirm",
            Msg::InputStash.t(),
            ModalButtonKind::Primary,
            disabled_reason,
            confirm_handler,
            cx,
        )));

    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_stash_push_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });
    let focusable_card = {
        let base = div().on_key_down(esc_cancel);
        if let Some(ref fh) = focus_handle {
            base.track_focus(fh).child(card)
        } else {
            base.child(card)
        }
    };

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(focusable_card)
}

/// The "planning…" placeholder every stash confirmation shows between the
/// modal opening and its plan arriving (or the error that replaced it).
///
/// #462: it lives here rather than in `modal_renderers_plan.rs`, which holds
/// wrappers around the shared plan card — this card has no plan to wrap, and
/// its callers are the three stash confirmations.
pub(crate) fn render_stash_planning(
    error: Option<SharedString>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let card = modal_card(MODAL_W_MD)
        .child(error.unwrap_or_else(|| Msg::EditorWorkspaceLoading.t().into()))
        .child(modal_button(
            "stash-planning-cancel",
            Msg::PlanCancel.t(),
            ModalButtonKind::Cancel,
            None,
            cx.listener(|this, _, _, cx| {
                this.cancel_active_modal(cx);
                cx.notify();
            }),
            cx,
        ));
    modal_overlay(card).into_any_element()
}

// ──────────────────────────────────────────────────────────────
// Stash apply modal renderer (T015)
// ──────────────────────────────────────────────────────────────

/// Render the stash apply confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title (showing stash index)
///   - Current → Predicted state
///   - Blockers (red) if any
///   - Ready recovery commands in one collapsed disclosure
///   - Error message (if execute failed)
///   - `[Cancel]` and an always-visible, disabled-when-blocked `[Apply]`
pub(crate) fn render_stash_apply_modal(
    modal: StashApplyModal,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let Some(plan) = modal.plan.clone() else {
        return render_stash_planning(modal.error, cx);
    };
    let has_blockers = !plan.blockers.is_empty();
    let disabled_reason = if has_blockers {
        modal.error.clone().or_else(|| {
            plan.blockers
                .first()
                .map(|blocker| SharedString::from(plan_note_text(blocker)))
        })
    } else {
        None
    };

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &(), window, cx| {
        this.cancel_stash_apply_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &(), window, cx| {
        this.confirm_stash_apply(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // #454: the blockers and command disclosure share this card's single
    // scroll region; the action row stays pinned.
    let (title, chips) = plan_heading_text(&plan.title);
    let title_row = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            render_plan_heading(
                title,
                chips,
                (IconName::Inbox.into(), current_theme().color_success),
                None,
            )
            .flex_1()
            .min_w(gpui::px(0.)),
        )
        .child(modal_copy_button(
            "stash-apply-card-copy",
            Msg::ModalCopyAll.t(),
            plan_clipboard_text(
                &plan,
                &plan
                    .preview_files
                    .iter()
                    .map(|f| f.path.display().to_string())
                    .collect::<Vec<_>>(),
                ShellKind::current(),
            ),
            cx,
        ));
    let cancel: DialogHandler = Rc::new(move |w, a| cancel_handler(&(), w, a));
    let confirm: DialogHandler = Rc::new(move |w, a| confirm_handler(&(), w, a));
    let spec = dialog_a11y(
        &plan_title_text(&plan.title),
        disabled_reason.is_none().then_some(Msg::InputApply.t()),
        plan.destructive,
        ConfirmStage::Single,
    )
    .with_recovery(&plan_recovery_text(plan.recovery.as_ref()));
    let card = apply_dialog(
        "stash-apply-card",
        modal_card(MODAL_W_MD).id("stash-apply-card"),
        spec,
        disabled_reason.is_none().then(|| confirm.clone()),
        cancel.clone(),
    )
    .child(title_row);
    let mut body = modal_scroll_body()
        // ── Current → Predicted ─────────────────────────────
        .child(div().flex_shrink_0().child(render_current_predicted(&plan)));

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{2717} {}",
                        plan_note_text(b)
                    ))),
            );
        }
        body = body.child(block_col.flex_shrink_0());
    }

    if plan_ready(&plan) {
        if let Some(commands) = offered_recovery_commands(plan.recovery.as_ref()) {
            body = body.child(render_recovery_commands(
                commands,
                "stash-apply-recovery",
                "stash-apply-recovery-copy",
                "stash-apply-recovery-body",
                overrides,
                cx,
            ));
        }
    }

    // ── Error message ────────────────────────────────────
    if let Some(ref err) = modal.error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(modal_button(
            "stash-apply-cancel",
            Msg::PlanCancel.t(),
            ModalButtonKind::Cancel,
            None,
            move |_, w, a| cancel(w, a),
            cx,
        ))
        .child(crate::ui::e2e::measure_confirm(modal_button(
            "stash-apply-confirm",
            Msg::InputApply.t(),
            ModalButtonKind::Primary,
            disabled_reason,
            move |_, w, a| confirm(w, a),
            cx,
        )));
    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    // ── Full-screen overlay wrapper ─────────────────────────
    modal_overlay(card).into_any_element()
}
