//! History-rewriting / destructive modal renderers split out of
//! `modal_renderers.rs` (T-SPLIT-MODALS-001 / ADR-0116 Wave 3): amend (two-stage
//! rewrite-history confirm) and discard (two-stage permanent-discard confirm).
//! These build bespoke cards rather than delegating to `render_plan_modal_card`.
//! Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::modal_renderers::{
    modal_overlay, render_current_predicted, render_modal_title_row, render_recovery_box, ModalIcon,
};
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, KeyDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

/// Every fully-bespoke destructive modal in this file badges itself with
/// `trash-2` (no upstream `IconName` variant — raw asset path, same as the
/// toolbar's Editor/Graph glyphs) in `color_blocker`, matching every other
/// destructive plan-confirmation modal (user request 2026-07-23).
const DESTRUCTIVE_ICON: ModalIcon = ModalIcon::Path("icons/trash-2.svg");

/// Amend confirmation overlay (T-COMMIT-011, ADR-0040 / 0023).
///
/// History-rewriting → **two-stage confirm**.  The first Confirm click arms the
/// action (`confirm_armed` flips to true); the button then turns into an
/// explicit, red final-confirm that lists what is lost (the old SHA).  No typed
/// confirmation is required (ADR-0023).
pub(crate) fn render_amend_modal(
    modal: AmendPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let armed = modal.confirm_armed;
    let has_blockers = !modal.plan.blockers.is_empty();
    let plan = modal.plan.clone();
    let error = modal.error.clone();

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_amend_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        // First click arms; second click executes (handled in start_amend).
        this.start_amend(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // Build the standard plan card body (title / current→predicted / warnings /
    // blockers / recovery / error) and append a two-stage confirm row.
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(render_modal_title_row(
            SharedString::from(plan_title_text(&plan.title)),
            Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
        ))
        .child(render_current_predicted(
            &plan,
            Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
        ));

    // Warnings.
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{26a0} {}",
                        plan_note_text(w)
                    ))),
            );
        }
        card = card.child(warn_col);
    }

    // Staged files folded in (preview_files), if any.
    if !plan.preview_files.is_empty() {
        let total = plan.preview_files.len();
        let mut col = div().flex().flex_col().gap_1().child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(format!(
                    "Staged changes folded in ({})",
                    total
                ))),
        );
        for f in plan.preview_files.iter().take(10) {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_sub))
                    .overflow_hidden()
                    .child(SharedString::from(f.path.display().to_string())),
            );
        }
        card = card.child(col);
    }

    // Blockers.
    if has_blockers {
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
        card = card.child(block_col);
    }

    // Recovery.
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        card = card.child(render_recovery_box(
            &recovery_text,
            current_theme().color_blocker,
        ));
    }

    // When armed: explicit "what is lost" second-stage notice (ADR-0023).
    if armed && !has_blockers {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div().text_sm().text_color(rgb(current_theme().color_blocker))
                        .child(SharedString::from("\u{26a0} This rewrites history. Click \u{201c}Rewrite history\u{201d} to confirm.")),
                )
                .child(
                    div().text_xs().text_color(rgb(current_theme().text_sub)).overflow_hidden()
                        .child(SharedString::from(
                            "The current commit's SHA will be replaced. The old commit becomes unreachable from the branch (recoverable via git reflog / reset --hard <old>).",
                        )),
                ),
        );
    }

    // Error.
    if let Some(err) = &error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // Buttons.
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("amend-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        // Stage 1 label = "Amend\u{2026}", stage 2 (armed) = red "Rewrite history".
        let label = if armed {
            "Rewrite history"
        } else {
            "Amend\u{2026}"
        };
        let confirm = if armed {
            KagiButton::accent("amend-confirm", label, current_theme().color_blocker, cx)
        } else {
            Button::new("amend-confirm").label(label).primary()
        };
        button_row = button_row.child(confirm.small().on_click(confirm_handler));
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(card).into_any_element()
}

/// Discard confirmation overlay (W17-DISCARD, ADR-0046).
///
/// Danger (red) card: target file list (scrollable), any skipped
/// untracked/conflicted files, recovery note, Cancel + red Discard.
/// ESC cancels. Both the backdrop AND the card call `.occlude()` to defeat the
/// known click-through bug. The Discard button is hidden when there are blockers
/// or zero targets.
pub(crate) fn render_discard_modal(
    modal: DiscardModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();
    let target_count = modal.paths.len();
    let can_discard = !has_blockers && target_count > 0;
    // Two-stage confirm (T-REARCH-014): first click arms, second executes.
    let armed = modal.confirm_armed;

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_discard_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_discard(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_discard_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });

    let title = if modal.is_all {
        format!("Discard all changes ({})", target_count)
    } else {
        plan_title_text(&plan.title)
    };

    // ── Target file list (scrollable) ───────────────────────
    let mut file_list = div()
        .id("discard-file-list")
        .flex()
        .flex_col()
        .gap_px()
        .max_h(theme::scaled_px(180.))
        .overflow_y_scroll();
    for p in &modal.paths {
        let line: String = p.chars().take(80).collect();
        file_list = file_list.child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_main))
                .overflow_hidden()
                .child(SharedString::from(line)),
        );
    }

    // ── Card ─────────────────────────────────────────────────
    // Icon badge (trash-2 / color_blocker) now carries the danger signal that
    // the full-card red border used to — matches every other destructive
    // plan-confirmation modal (user request 2026-07-23), one less box.
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(render_modal_title_row(
            SharedString::from(title),
            Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
        ))
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(format!(
                    "{} file(s) to discard:",
                    target_count
                ))),
        )
        .child(file_list);

    // ── Skipped (untracked / conflicted) ────────────────────
    if !modal.skipped.is_empty() {
        let mut skip_col = div().flex().flex_col().gap_px().child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(format!(
                    "Skipped ({}):",
                    modal.skipped.len()
                ))),
        );
        for p in modal.skipped.iter().take(20) {
            let line: String = p.chars().take(80).collect();
            skip_col = skip_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_muted))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{2014} {} (untracked/conflicted)",
                        line
                    ))),
            );
        }
        card = card.child(skip_col);
    }

    // ── Warnings / Blockers ─────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_px();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{26a0} {}",
                        plan_note_text(w)
                    ))),
            );
        }
        card = card.child(warn_col);
    }
    if has_blockers {
        let mut block_col = div().flex().flex_col().gap_px();
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
        card = card.child(block_col);
    }

    // ── Recovery note ───────────────────────────────────────
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        card = card.child(render_recovery_box(
            &recovery_text,
            current_theme().color_blocker,
        ));
    }

    // ── Error (preflight / execute failure) ─────────────────
    if let Some(err) = &modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Two-stage "what is lost" warning (armed second stage) ──
    // Mirrors amend's armed notice (ADR-0023). Only shown after the first
    // click armed the action, so the user sees an explicit final warning.
    if armed && can_discard {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .child(SharedString::from(
                    "\u{26a0} Working-tree changes will be lost. Click \u{201c}Permanently discard\u{201d} to confirm.",
                )),
        );
    }

    // ── Buttons ─────────────────────────────────────────────
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("discard-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if can_discard {
        // Two-stage confirm (T-REARCH-014): first click arms the red Discard
        // button (label becomes the explicit "Permanently discard N files");
        // the second click executes. Mirrors amend's confirm_armed pattern.
        // Both stages stay red — discard is always a destructive op.
        let label = if armed {
            format!("Permanently discard {} file(s)", target_count)
        } else {
            format!("Discard {} file(s)", target_count)
        };
        button_row = button_row.child(
            KagiButton::accent("discard-confirm", label, current_theme().color_blocker, cx)
                .small()
                .on_click(confirm_handler),
        );
    }
    card = card.child(button_row);

    // ── Full-screen overlay (shared chrome, T-SPLIT-HELPERS-001) ──
    // ESC cancels via the root key handler; the card itself also occludes
    // (ADR-0046 / W17), else clicks fall through to the UI beneath. Chaining
    // `.on_key_down` onto the shared overlay is DOM-equivalent — event handlers
    // are stored independently of the child element list.
    modal_overlay(card.occlude())
        .on_key_down(esc_cancel)
        .into_any_element()
}
