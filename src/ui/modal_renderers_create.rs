//! Create-branch / create-worktree modal renderers split out of
//! `modal_renderers.rs` (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). Both build a
//! name-input card with a live plan preview and a focusable, ESC-cancellable
//! wrapper. Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::modal_renderers::{
    modal_overlay, render_current_predicted, render_modal_title_row, render_recovery_box,
};
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, App, Context, FocusHandle, KeyDownEvent, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{IconName, Sizable as _};
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

// ──────────────────────────────────────────────────────────────
// Create-branch modal renderer (T014)
// ──────────────────────────────────────────────────────────────

/// Render the create-branch confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Branch name text input (live KeyDown handler)
///   - Live plan: Current → Predicted state
///   - Blockers (red) if any
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always; `[Create]` only when no blockers and name is non-empty
pub(crate) fn render_create_branch_modal(
    modal: CreateBranchModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);

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
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title (icon-badge header, matching Pull/Push's richer card —
        // user request 2026-07-23; `Plus` mirrors the toolbar "Branch"
        // button's own icon, `color_success` matches this modal's own
        // Create button accent below) ──────────────────────────
        .child(render_modal_title_row(
            SharedString::from(format!(
                "Create branch @ {}  {}",
                modal.at.short(),
                modal.start_title
            )),
            Some((IconName::Plus, current_theme().color_success)),
        ))
        // ── Name input ────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Branch name")),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        )
        .child(
            div().px_2().py_1().child(
                Checkbox::new("create-branch-checkout-after")
                    .label("Checkout after create")
                    .checked(modal.checkout_after)
                    .on_click(toggle_checkout),
            ),
        );

    if let Some(ref p) = plan {
        // ── Plan state (current → predicted), same boxed treatment as
        // Pull/Push (user request 2026-07-23) — creating a branch never
        // moves HEAD or touches the working tree, so current and predicted
        // are identical here; showing them side by side is the same
        // reassurance the recovery text gives, just at a glance.
        card = card.child(render_current_predicted(
            p,
            Some((IconName::Plus, current_theme().color_success)),
        ));

        // ── Blockers (localized) ──────────────────────────
        if !p.blockers.is_empty() {
            let lines: Vec<SharedString> = p
                .blockers
                .iter()
                .map(|b| SharedString::from(plan_note_text(b)))
                .collect();
            let mut block_col = div().flex().flex_col().gap_1();
            for b in lines {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        // ── Recovery (same grouped/monospace treatment as Pull/Push —
        // user request 2026-07-23) ─────────────────────────
        let recovery_text = plan_recovery_text(p.recovery.as_ref());
        if !recovery_text.is_empty() {
            card = card.child(render_recovery_box(
                &recovery_text,
                current_theme().color_success,
            ));
        }
    }

    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("create-branch-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    // Create button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            KagiButton::accent(
                "create-branch-confirm",
                "Create",
                current_theme().color_success,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

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
    let plan = modal.plan.clone();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);

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

    let mut card = div()
        .w(theme::scaled_px(540.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(render_modal_title_row(
            SharedString::from(format!(
                "Create worktree @ {}  {}",
                modal.at.short(),
                modal.start_title
            )),
            Some((IconName::Plus, current_theme().color_success)),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Branch name")),
                )
                .children(modal.branch_state.as_ref().map(|st| Input::new(st).small())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Path")),
                )
                .children(modal.path_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(ref p) = plan {
        // Same boxed current/predicted treatment as Pull/Push/Create-Branch
        // (user request 2026-07-23: reuse this display everywhere instead of
        // a one-off layout per modal).
        card = card.child(render_current_predicted(
            p,
            Some((IconName::Plus, current_theme().color_success)),
        ));

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
            card = card.child(warn_col);
        }

        // ── Blockers (localized) ──────────────────────────
        if !p.blockers.is_empty() {
            let lines: Vec<SharedString> = p
                .blockers
                .iter()
                .map(|b| SharedString::from(plan_note_text(b)))
                .collect();
            let mut block_col = div().flex().flex_col().gap_1();
            for b in lines {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        let recovery_text = plan_recovery_text(p.recovery.as_ref());
        if !recovery_text.is_empty() {
            card = card.child(render_recovery_box(
                &recovery_text,
                current_theme().color_success,
            ));
        }
    }

    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("create-worktree-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if !has_blockers {
        button_row = button_row.child(
            KagiButton::accent(
                "create-worktree-confirm",
                "Create",
                current_theme().color_success,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    }
    card = card.child(button_row);

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
    let plan = modal.plan.clone();
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);

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

    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create tag @ {}  {}",
                    modal.at.short(),
                    modal.start_title
                ))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Tag name")),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(ref p) = plan {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(p.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", p.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_muted))
                        .child(SharedString::from(plan_title_text(&p.title))),
                ),
        );

        if !p.blockers.is_empty() {
            let lines: Vec<SharedString> = p
                .blockers
                .iter()
                .map(|b| SharedString::from(plan_note_text(b)))
                .collect();
            let mut block_col = div().flex().flex_col().gap_1();
            for b in lines {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(plan_recovery_text(p.recovery.as_ref()))),
        );
    }

    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("create-tag-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(
            KagiButton::accent(
                "create-tag-confirm",
                "Create",
                current_theme().color_success,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

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
