//! Stash push / apply modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). Pure physical move — behaviour
//! unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::i18n::Msg;
use super::modal_renderers::{
    modal_overlay, plan_status_chips, render_current_predicted, render_modal_title_row,
    render_recovery_box,
};
use super::modal_renderers_input::{
    render_input_modal_action_with_size, render_input_modal_heading, InputActionSize,
};
use super::modal_renderers_plan::render_input_recovery_commands;
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_MD};
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::{KagiApp, MONO_FONT};
use gpui::{div, prelude::*, rgb, Context, FocusHandle, KeyDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_domain::plan_note::{PlanNote, StashNote};
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

/// Stash-only status pills: keep the plan text intact while using quiet fills.
fn stash_status_chip(text: &str, kind: &str) -> gpui::AnyElement {
    let t = current_theme();
    let (color, fill_alpha, text_alpha) = match kind {
        "staged" | "clean" => (t.color_success, 0x33, 0xff),
        "modified" => (t.change_modified, 0x22, if t.dark { 0xe0 } else { 0xff }),
        "conflicted" => (t.color_blocker, 0x33, 0xff),
        _ => (t.text_main, 0x13, 0xcc),
    };
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .h(theme::scaled_px(24.))
        .px_2()
        .rounded(theme::scaled_px(6.))
        .bg(gpui::rgba((color << 8) | fill_alpha))
        .font_family(MONO_FONT)
        .text_xs()
        .text_color(gpui::rgba((color << 8) | text_alpha))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

fn stash_status_chips(dirty: &str) -> Vec<gpui::AnyElement> {
    if dirty == "clean" {
        return vec![stash_status_chip("clean", "clean")];
    }
    let mut chips = Vec::new();
    for part in dirty.split(", ") {
        let Some((count, kind)) = part.split_once(' ') else {
            return plan_status_chips(dirty);
        };
        if count.parse::<usize>().is_err()
            || !matches!(kind, "staged" | "modified" | "untracked" | "conflicted")
        {
            // An operation's explanatory text is not a count-only status.
            return plan_status_chips(dirty);
        }
        chips.push(stash_status_chip(part, kind));
    }
    chips
}

/// The Stash Push preview renders the live plan in two full-width rows. Every
/// branch detail remains visible (or horizontally scrollable) at small sizes.
fn stash_preview_state(
    head: &str,
    dirty: &str,
    label: &'static str,
    id: &'static str,
) -> gpui::AnyElement {
    let full = SharedString::from(format!("{label}: {head} [{dirty}]"));
    div()
        .id(id)
        .role(gpui::Role::Group)
        .aria_label(full.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
        })
        .relative()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .gap_3()
        .px_3()
        .py_2()
        .child(
            div()
                .w(theme::scaled_px(44.))
                .flex_shrink_0()
                .pt(theme::scaled_px(3.))
                .text_xs()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(theme::scaled_px(6.))
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(theme::scaled_px(13.))
                        .line_height(gpui::relative(1.5))
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from(head.to_owned())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(theme::scaled_px(6.))
                        .children(stash_status_chips(dirty)),
                ),
        )
        .when(cfg!(feature = "gui-e2e"), |row| {
            row.child(super::e2e::measure_inside(id))
        })
        .into_any_element()
}

fn render_stash_push_preview(plan: &kagi_git::OperationPlan) -> gpui::AnyElement {
    div()
        .id("plan-state-comparison")
        .w_full()
        .min_w(gpui::px(0.))
        .rounded(theme::scaled_px(10.))
        .bg(rgb(current_theme().panel))
        .overflow_x_scroll()
        .flex()
        .flex_col()
        .child(stash_preview_state(
            &plan.current.head,
            &plan.current.dirty,
            Msg::InputStashCurrent.t(),
            "plan-state-current",
        ))
        .child(
            div()
                .border_t_1()
                .border_color(rgb(current_theme().surface))
                .child(stash_preview_state(
                    &plan.predicted.head,
                    &plan.predicted.dirty,
                    Msg::InputStashAfter.t(),
                    "plan-state-predicted",
                )),
        )
        .into_any_element()
}

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
    let card = modal_card(MODAL_W_MD - 16.)
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
                        .h(theme::scaled_px(38.))
                        .rounded(theme::scaled_px(8.))
                        .bg(rgb(current_theme().panel))
                        .text_size(theme::scaled_px(14.))
                })),
        );

    // ── Plan state (current → predicted) ─────────────────
    if let Some(ref p) = plan {
        body = body.child(div().flex_shrink_0().child(render_stash_push_preview(p)));

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
        if message_filled && !has_blockers {
            if let Some(recovery) = p.recovery.as_ref().filter(|r| !r.commands.is_empty()) {
                body = body.child(div().flex_shrink_0().child(render_input_recovery_commands(
                    &recovery.commands,
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
        .child(
            Button::new("stash-push-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .small()
                .on_click(cancel_handler),
        )
        .child(crate::ui::e2e::measure_confirm(
            render_input_modal_action_with_size(
                || {
                    KagiButton::accent(
                        "stash-push-confirm",
                        Msg::InputStash.t(),
                        current_theme().color_warning,
                        cx,
                    )
                    .small()
                    .on_click(confirm_handler)
                },
                Msg::InputStash.t(),
                current_theme().color_warning,
                disabled_reason,
                InputActionSize::Small,
                cx,
            ),
        ));

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
        .child(
            Button::new("stash-planning-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cancel_active_modal(cx);
                    cx.notify();
                })),
        );
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
///   - Recovery text
///   - Error message (if execute failed)
///   - `[Cancel]` always; `[Apply]` only when no blockers
pub(crate) fn render_stash_apply_modal(
    modal: StashApplyModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let Some(plan) = modal.plan.clone() else {
        return render_stash_planning(modal.error, cx);
    };
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_apply_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_apply(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // #454: blockers and the recovery prose grow with the plan and this card
    // has no inner scroller, so the body is its single scroll region.
    let card = modal_card(MODAL_W_MD).child(div().flex_shrink_0().child(render_modal_title_row(
        SharedString::from(plan_title_text(&plan.title)),
        Some((IconName::Inbox.into(), current_theme().color_success)),
    )));
    let mut body = modal_scroll_body()
        // ── Current → Predicted ─────────────────────────────
        .child(div().flex_shrink_0().child(render_current_predicted(
            &plan,
            Some((IconName::Inbox.into(), current_theme().color_success)),
        )));

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

    // ── Recovery ──────────────────────────────────────────
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        body = body.child(div().flex_shrink_0().child(render_recovery_box(
            &recovery_text,
            current_theme().color_success,
        )));
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

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("stash-apply-cancel")
            .label(Msg::PlanCancel.t())
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(crate::ui::e2e::measure_confirm(
            KagiButton::accent(
                "stash-apply-confirm",
                "Apply",
                current_theme().color_success,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        ));
    }

    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    // ── Full-screen overlay wrapper ─────────────────────────
    modal_overlay(card).into_any_element()
}
