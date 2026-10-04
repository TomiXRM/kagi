//! Create-branch / create-worktree modal renderers split out of
//! `modal_renderers.rs` (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). Both build a
//! name-input card with a live plan preview and a focusable, ESC-cancellable
//! wrapper. Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::i18n::Msg;
use super::modal_renderers::{modal_overlay, render_current_predicted, ModalIcon};
use super::modal_renderers_input::{
    render_input_modal_action, render_input_modal_field, render_input_modal_heading,
};
use super::modal_renderers_plan::{offered_recovery_commands, render_input_recovery_commands};
use super::modal_shell::{modal_card, modal_scroll_body, MODAL_W_LG, MODAL_W_MD};
use super::modals::worktree::CreateWorktreeModal;
use super::modals::*;
use super::theme::theme as current_theme;
use super::KagiApp;
use gpui::{div, prelude::*, rgb, App, Context, FocusHandle, KeyDownEvent, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::IconName;
use kagi_domain::plan_note::{CommonNote, PlanNote, TagNote};
use kagi_ui_core::i18n::plan_note_text;

// ──────────────────────────────────────────────────────────────
// Create-branch modal renderer (T014)
// ──────────────────────────────────────────────────────────────

/// Render the create-branch confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Short title and selected commit on a second line
///   - Branch name input, with its own validation reason
///   - Live plan: Current → Predicted state and non-field blockers
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` and an always-visible, disabled-until-ready `[Create]`
pub(crate) fn render_create_branch_modal(
    modal: CreateBranchModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // A pending or failed plan cannot confirm; the action remains visible so
    // its disabled reason is reachable by pointer and assistive technology.
    let plan = modal.plan.plan().cloned();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
    let error = plan_or_exec_error(&modal.plan, modal.error.clone());

    // ── Cancel handler ──────────────────────────────────────
    // T-BP-003: return focus to root_focus so cmd-j keeps working.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_create_branch_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // ── Confirm handler (only created when no blockers) ─────
    // T-BP-003: return focus to root_focus after confirm.
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_create_branch(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // W12-GCADOPT (§2.7): replace the old `[ ]`/`[x]` pseudo-checkbox text with a
    // real `gpui_component::checkbox::Checkbox`.  Its `on_click` hands us the new
    // checked state; we route it through the same toggle + replan logic via the
    // KagiApp entity (Checkbox callbacks take `&mut App`, not `&mut Context`).
    let app_entity = cx.entity();
    let toggle_checkout = move |new_checked: &bool, _window: &mut Window, cx: &mut App| {
        let new_checked = *new_checked;
        app_entity.update(cx, |this, cx| {
            if let Some(modal) = this.create_branch_modal_mut() {
                modal.checkout_after = new_checked;
                modal.error = None;
            }
            this.replan_create_branch();
            cx.notify();
        });
    };

    // ── Build modal card ────────────────────────────────────
    // The card's scrollable body holds the plan details; title and buttons
    // remain pinned outside it.
    let card = modal_card(MODAL_W_MD).child(render_input_modal_heading(
        Msg::InputBranchTitle.t(),
        Some((modal.at.short(), &modal.start_title)),
        IconName::Plus.into(),
        current_theme().color_success,
    ));
    let mut body = modal_scroll_body()
        // ── Name input ────────────────────────────────────
        .child(render_input_modal_field(
            Msg::InputBranchName.t(),
            modal.input_state.as_ref(),
            plan.as_ref()
                .and_then(|p| {
                    p.blockers.iter().find(|b| {
                        matches!(b, PlanNote::Common(CommonNote::BranchNameErrorKeyed(_)))
                    })
                })
                .map(|b| SharedString::from(plan_note_text(b))),
        ))
        .child(
            div().flex_shrink_0().px_2().py_1().child(
                Checkbox::new("create-branch-checkout-after")
                    .label(Msg::InputCheckoutAfterCreate.t())
                    .checked(modal.checkout_after)
                    .on_click(toggle_checkout),
            ),
        );

    if let Some(ref p) = plan {
        // Branch creation leaves HEAD and the working tree unchanged; show
        // both states using the same stacked comparison as every plan card.
        body = body.child(div().flex_shrink_0().child(render_current_predicted(p)));

        // Field validation belongs beside the input; every other safety
        // blocker remains in the plan area and still blocks confirmation.
        let mut block_col = div().flex().flex_col().gap_1();
        let mut nonfield_blocker = false;
        for b in p
            .blockers
            .iter()
            .filter(|b| !matches!(b, PlanNote::Common(CommonNote::BranchNameErrorKeyed(_))))
        {
            nonfield_blocker = true;
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
        if nonfield_blocker {
            body = body.child(block_col.flex_shrink_0());
        }

        // Creating a branch never needs recovery in this card: the complete
        // guidance remains available in the operation log.
    }
    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // Keep the primary in place while validation or planning blocks it.
    let disabled_reason = if has_blockers {
        error.clone().or_else(|| {
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
        .child(crate::ui::e2e::measure_control(
            "create-branch-cancel",
            Button::new("create-branch-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .on_click(cancel_handler),
        ))
        .child(crate::ui::e2e::measure_confirm(render_input_modal_action(
            || {
                KagiButton::accent(
                    "create-branch-confirm",
                    Msg::InputCreate.t(),
                    current_theme().color_success,
                    cx,
                )
                .on_click(confirm_handler)
            },
            Msg::InputCreate.t(),
            current_theme().color_success,
            disabled_reason,
            cx,
        )));

    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    // Real text inputs handle their own focus/keys now. Escape bubbles up
    // from the focused input to this wrapper and cancels (user request).
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_create_branch_modal();
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

pub(crate) fn render_create_worktree_modal(
    modal: CreateWorktreeModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Pending/failed plans retain a visible, disabled primary action.
    let plan = modal.plan.plan().cloned();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
    let error = plan_or_exec_error(&modal.plan, modal.error.clone());

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_create_worktree_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_create_worktree(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // Keep the two fields together above the safety preview. Only keyed
    // validation for a field moves out of the blocker area.
    let card = modal_card(MODAL_W_LG).child(render_input_modal_heading(
        Msg::InputWorktreeTitle.t(),
        Some((modal.at.short(), &modal.start_title)),
        IconName::FolderOpen.into(),
        current_theme().color_success,
    ));
    let mut body = modal_scroll_body()
        .child(render_input_modal_field(
            Msg::InputBranchName.t(),
            modal.branch_state.as_ref(),
            plan.as_ref()
                .and_then(|p| {
                    p.blockers.iter().find(|b| {
                        matches!(b, PlanNote::Common(CommonNote::BranchNameErrorKeyed(_)))
                    })
                })
                .map(|b| SharedString::from(plan_note_text(b))),
        ))
        .child(render_input_modal_field(
            Msg::InputWorktreePath.t(),
            modal.path_state.as_ref(),
            plan.as_ref()
                .and_then(|p| {
                    p.blockers.iter().find(|b| {
                        matches!(
                            b,
                            PlanNote::Common(
                                CommonNote::WorktreePathErrorKeyed(_)
                                    | CommonNote::GitErrorPassthrough { .. }
                            )
                        )
                    })
                })
                .map(|b| SharedString::from(plan_note_text(b))),
        ));

    if let Some(ref p) = plan {
        // Reuse the shared comparison rather than a worktree-specific layout.
        body = body.child(div().flex_shrink_0().child(render_current_predicted(p)));

        if !p.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for w in &p.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("! {}", plan_note_text(w)))),
                );
            }
            body = body.child(warn_col.flex_shrink_0());
        }

        let mut block_col = div().flex().flex_col().gap_1();
        let mut nonfield_blocker = false;
        for b in p.blockers.iter().filter(|b| {
            !matches!(
                b,
                PlanNote::Common(
                    CommonNote::BranchNameErrorKeyed(_)
                        | CommonNote::WorktreePathErrorKeyed(_)
                        | CommonNote::GitErrorPassthrough { .. }
                )
            )
        }) {
            nonfield_blocker = true;
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
        if nonfield_blocker {
            body = body.child(block_col.flex_shrink_0());
        }
        let fields_filled = modal
            .branch_state
            .as_ref()
            .is_some_and(|state| !state.read(cx).value().trim().is_empty())
            && modal
                .path_state
                .as_ref()
                .is_some_and(|state| !state.read(cx).value().trim().is_empty());
        if fields_filled && !has_blockers {
            if let Some(commands) = offered_recovery_commands(p.recovery.as_ref()) {
                body = body.child(div().flex_shrink_0().child(render_input_recovery_commands(
                    commands,
                    current_theme().color_success,
                )));
            }
        }
    }
    if let Some(ref err) = error {
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
        error.clone().or_else(|| {
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
            Button::new("create-worktree-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .on_click(cancel_handler),
        )
        .child(crate::ui::e2e::measure_confirm(render_input_modal_action(
            || {
                KagiButton::accent(
                    "create-worktree-confirm",
                    Msg::InputCreate.t(),
                    current_theme().color_success,
                    cx,
                )
                .on_click(confirm_handler)
            },
            Msg::InputCreate.t(),
            current_theme().color_success,
            disabled_reason,
            cx,
        )));
    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_create_worktree_modal();
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

// ──────────────────────────────────────────────────────────────
// Create-tag modal renderer (branch-menu "Create tag here...")
// ──────────────────────────────────────────────────────────────

/// Render the create-tag confirmation overlay. Mirrors
/// [`render_create_branch_modal`] minus the checkout-after checkbox — a tag
/// is never checked out.
pub(crate) fn render_create_tag_modal(
    modal: CreateTagModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // A pending or failed plan cannot confirm, but the disabled Create action
    // remains visible with a reason.
    let plan = modal.plan.plan().cloned();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
    let error = plan_or_exec_error(&modal.plan, modal.error.clone());

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_create_tag_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_create_tag(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // Keep the title concise; the commit remains visible but secondary.
    let card = modal_card(MODAL_W_MD).child(render_input_modal_heading(
        Msg::InputTagTitle.t(),
        Some((modal.at.short(), &modal.start_title)),
        ModalIcon::Path("icons/tag.svg"),
        current_theme().color_tag,
    ));
    let mut body = modal_scroll_body().child(render_input_modal_field(
        Msg::InputTagName.t(),
        modal.input_state.as_ref(),
        plan.as_ref()
            .and_then(|p| {
                p.blockers
                    .iter()
                    .find(|b| matches!(b, PlanNote::Tag(TagNote::NameError(_))))
            })
            .map(|b| SharedString::from(plan_note_text(b))),
    ));

    if let Some(ref p) = plan {
        body = body.child(div().flex_shrink_0().child(render_current_predicted(p)));

        let mut block_col = div().flex().flex_col().gap_1();
        let mut nonfield_blocker = false;
        for b in p
            .blockers
            .iter()
            .filter(|b| !matches!(b, PlanNote::Tag(TagNote::NameError(_))))
        {
            nonfield_blocker = true;
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
        if nonfield_blocker {
            body = body.child(block_col.flex_shrink_0());
        }
        let input_filled = modal
            .input_state
            .as_ref()
            .is_some_and(|state| !state.read(cx).value().trim().is_empty());
        if input_filled && !has_blockers {
            if let Some(commands) = offered_recovery_commands(p.recovery.as_ref()) {
                body = body.child(div().flex_shrink_0().child(render_input_recovery_commands(
                    commands,
                    current_theme().color_tag,
                )));
            }
        }
    }

    if let Some(ref err) = error {
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
        error.clone().or_else(|| {
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
            Button::new("create-tag-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .on_click(cancel_handler),
        )
        .child(crate::ui::e2e::measure_confirm(render_input_modal_action(
            || {
                KagiButton::accent(
                    "create-tag-confirm",
                    Msg::InputCreate.t(),
                    current_theme().color_success,
                    cx,
                )
                .on_click(confirm_handler)
            },
            Msg::InputCreate.t(),
            current_theme().color_success,
            disabled_reason,
            cx,
        )));

    let card = card
        .child(body)
        .child(div().flex_shrink_0().child(button_row));

    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_create_tag_modal();
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

    modal_overlay(focusable_card)
}
