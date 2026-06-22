//! Modal renderer functions extracted from modals.rs (ADR-0114 / Phase D).
//!
//! These are the per-modal `render_*` functions that build GPUI elements from
//! modal state structs. Extracted from modals.rs to bring it under the 800-LOC
//! target (AGENTS.md). modals.rs retains the modal state structs + ActiveModal enum.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::commit_panel::{status_badge, CommitPlanModal};
use super::i18n::Msg;
use super::modals::*;
use super::theme::{self, theme as current_theme};
use super::*;
use super::{file_tree, smart_commit, KagiApp};
use gpui::{
    div, prelude::*, px, rgb, App, Context, Entity, FocusHandle, KeyDownEvent, SharedString, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::{Disableable as _, Sizable as _};
use kagi_git::BranchRenameValidation;
use kagi_git::{ChangeKind, CommitId, MergeKind};

pub(crate) fn render_plan_modal(
    modal: CheckoutPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let create_branch_target = match &modal.target {
        CheckoutPlanTarget::Commit(commit_id) => Some(commit_id.clone()),
        CheckoutPlanTarget::Branch(_) => None,
    };
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_checkout(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Checkout",
        cancel_handler,
        confirm_handler,
        create_branch_target,
        cx,
    )
    .into_any_element()
}

/// Pull plan confirmation overlay (T-HT-003) — same card as the checkout
/// plan modal, wired to `confirm_pull`.
pub(crate) fn render_pull_modal(
    modal: PullPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_pull_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        // W3-NOTIFY: run on a background thread (start/finish toasts).
        this.start_pull(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Pull",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Undo-commit confirmation overlay (T-HT-009).
pub(crate) fn render_undo_modal(
    modal: UndoPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_undo_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_undo(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Undo",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Operation-history Undo / Redo confirmation overlay (T-UNDOREDO-001,
/// ADR-0081). Confirming runs the safe ref move through the standard pipeline
/// and advances/retreats the history cursor.
pub(crate) fn render_history_modal(
    modal: HistoryPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.clear_history_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_history(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_label = if modal.is_undo {
        Msg::Undo.t()
    } else {
        Msg::Redo.t()
    };
    render_plan_modal_card(
        modal.plan,
        modal.error,
        confirm_label,
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Sequencer `<op> --continue` confirmation overlay (ADR-0068 /
/// T-CONFLICT-FLOW-032).  Shown when Continue routes a rebase / cherry-pick /
/// revert; confirming advances the sequencer.
pub(crate) fn render_conflict_continue_modal(
    modal: ConflictContinuePlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_conflict_continue();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_conflict_continue(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        Msg::ConflictContinue.t(),
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

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
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        // First click arms; second click executes (handled in start_amend).
        this.start_amend(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
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
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
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
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
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
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.predicted.dirty))),
                        ),
                ),
        );

    // Warnings.
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
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
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // Recovery.
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

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

/// Stash-pop confirmation overlay (T-HT-007).
pub(crate) fn render_pop_modal(modal: PopPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_pop_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_pop(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Pop",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Stash drop confirmation overlay (ADR-0087) — Destructive: deletes the
/// stash entry without touching the working tree. Same card as Pop, wired to
/// `start_stash_drop`.
pub(crate) fn render_stash_drop_modal(
    modal: StashDropModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_drop_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_stash_drop(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Drop",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Push plan confirmation overlay (T-HT-004) — same card as the pull
/// plan modal, wired to `confirm_push`.
pub(crate) fn render_push_modal(
    modal: PushPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_push_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        // W3-NOTIFY: run on a background thread (start/finish toasts).
        this.start_push(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Push",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

pub(crate) fn render_branch_plan_modal(
    modal: BranchPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = match modal.kind {
        BranchPlanKind::PullFfOnly => "Pull",
        BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => "Push",
    };
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_branch_plan_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_branch_plan(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        label,
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

pub(crate) fn render_input_plan_modal(
    title: String,
    label: &'static str,
    input_state: Option<Entity<InputState>>,
    plan: Option<std::sync::Arc<OperationPlan>>,
    validation: Option<BranchRenameValidation>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
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
                .child(SharedString::from(title)),
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
                        .child(SharedString::from(label)),
                )
                .children(input_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(BranchRenameValidation::Invalid(reason)) = validation {
        // W29-I18N-WAVE2: localize the keyed branch-name reason.
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(SharedString::from(crate::ui::i18n::branch_name_error(
                    &reason,
                ))),
        );
    }

    if let Some(plan) = plan {
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
                        .text_sm()
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from(plan.current.head.clone())),
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
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from(plan.predicted.head.clone())),
                ),
        );

        if !plan.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for warning in &plan.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{26a0} {}", warning))),
                );
            }
            card = card.child(warn_col);
        }
        if !plan.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for blocker in &plan.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", blocker))),
                );
            }
            card = card.child(block_col);
        }
    }

    if let Some(err) = error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err),
        );
    }

    let mut buttons = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("branch-input-cancel")
            .label("Cancel")
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
    card = card.child(buttons);

    modal_overlay(card).into_any_element()
}

pub(crate) fn render_set_upstream_modal(
    modal: SetUpstreamModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_set_upstream_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_set_upstream(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_input_plan_modal(
        format!("Set upstream for {}", modal.branch_name),
        "Upstream",
        modal.input_state,
        modal.plan,
        None,
        modal.error,
        "Set upstream",
        cancel_handler,
        confirm_handler,
    )
}

pub(crate) fn render_rename_branch_modal(
    modal: RenameBranchModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_rename_branch_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_rename_branch(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_input_plan_modal(
        format!("Rename {}", modal.old_name),
        "New branch name",
        modal.input_state,
        modal.plan,
        Some(modal.validation),
        modal.error,
        "Rename",
        cancel_handler,
        confirm_handler,
    )
}

pub(crate) fn render_merge_modal(
    modal: MergePlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_merge_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_merge(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    // W31-MERGE-INTO-CONFLICT: a conflict-producing merge gets a localized
    // Confirm-button label. The full "Merge <source> into <current>" context is
    // already the modal's title (the plan title), so the button stays short —
    // just "Merge" — so a long branch name (e.g. origin/FW/MainBoard_V26.1) can't
    // overflow the button width. The conflict case keeps its short localized
    // hint + a prominent warning banner prepended to the plan's warnings.
    let (confirm_label, plan): (SharedString, std::sync::Arc<OperationPlan>) =
        if matches!(modal.kind, MergeKind::Conflicts(_)) {
            let mut plan = (*modal.plan).clone();
            plan.warnings
                .insert(0, Msg::MergeConflictWarning.t().to_string());
            (
                SharedString::from(Msg::MergeAndResolveConflicts.t()),
                std::sync::Arc::new(plan),
            )
        } else {
            (SharedString::from("Merge"), modal.plan)
        };
    render_plan_modal_card(
        plan,
        modal.error,
        confirm_label,
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

pub(crate) fn render_tracking_checkout_modal(
    modal: TrackingCheckoutPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_tracking_checkout_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_tracking_checkout(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Checkout",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Switch-to-latest confirmation overlay (ADR-0101).
pub(crate) fn render_switch_to_latest_modal(
    modal: SwitchToLatestPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_switch_to_latest_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_switch_to_latest(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Switch",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Delete-branch confirmation overlay (W2-DELETE).
pub(crate) fn render_delete_branch_modal(
    modal: DeleteBranchModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_delete_branch_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_delete_branch(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Delete",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
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
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_discard(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_discard_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });

    let title = if modal.is_all {
        format!("Discard all changes ({})", target_count)
    } else {
        plan.title.clone()
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
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .border_1()
        .border_color(rgb(current_theme().color_blocker))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(current_theme().color_blocker))
                .text_xl()
                .child(SharedString::from(title)),
        )
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
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
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
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // ── Recovery note ───────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

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

/// Revert confirmation overlay (T-CM-034).
pub(crate) fn render_revert_modal(
    modal: RevertModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_revert_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_revert(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Revert",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Shared full-screen modal overlay chrome (T-SPLIT-HELPERS-001 / ADR-0116
/// Wave 3). Every modal renderer wrapped its card in the same two-layer
/// structure: a semi-transparent, occluding backdrop + a centred flex column
/// holding the card. This factors that DOM into one place so each renderer
/// only builds its card and calls `modal_overlay(card)`.
///
/// Produces exactly the tree the renderers built inline:
/// ```text
/// div.size_full.absolute.top_0.left_0
///   ├─ div.size_full.absolute.top_0.left_0.occlude.bg(modal_overlay).opacity(0.65)   // backdrop
///   └─ div.size_full.absolute.top_0.left_0.flex.flex_col.justify_center.items_center  // centring
///        └─ {card}
/// ```
/// Returns a `Div` (not `impl IntoElement`) so callers that additionally
/// attached a root-level `.on_key_down(..)` (the discard modal's ESC handler)
/// can keep chaining it — event handlers are stored independently of children,
/// so chaining order does not change the rendered tree. Callers that occluded
/// the card itself pass `card.occlude()` in the `card` slot.
pub(crate) fn modal_overlay(card: impl IntoElement) -> gpui::Div {
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Backdrop (dark, semi-transparent). `.occlude()` blocks mouse events
        // from reaching the UI beneath the modal (click-through bug).
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
        // Card centred on top of the backdrop.
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )
}

/// Shared plan-confirmation card: title / current→predicted / warnings /
/// blockers / recovery / error / Cancel + confirm buttons.  The confirm
/// button is hidden whenever the plan has blockers.
pub(crate) fn render_plan_modal_card(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: impl Into<SharedString>,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    create_branch_target: Option<CommitId>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Accept either a `&'static str` (most modals) or a dynamic `String`/
    // `SharedString` (merge: `Merge <source> into <target>`, T-DNDMERGE-001).
    let confirm_label: SharedString = confirm_label.into();
    let has_blockers = !plan.blockers.is_empty();

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ───────────────────────────
        .child(
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
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
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
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.predicted.dirty))),
                        ),
                ),
        );

    // ── Warnings ─────────────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // ── Commits to push (T-HT-004) ────────────────────────
    // Shown only when preview_commits is non-empty (push plans).
    if !plan.preview_commits.is_empty() {
        let total = plan.preview_commits.len();
        let show_count = total.min(10);
        let label = format!("Commits to push ({})", total);
        let mut commit_col = div().flex().flex_col().gap_1().child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label)),
        );
        for entry in plan.preview_commits.iter().take(show_count) {
            let line: String = entry.chars().take(72).collect();
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_sub))
                    .overflow_hidden()
                    .child(SharedString::from(line)),
            );
        }
        if total > 10 {
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_muted))
                    .child(SharedString::from(format!(
                        "\u{2026} and {} more",
                        total - 10
                    ))),
            );
        }
        card = card.child(commit_col);
    }

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
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

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message (preflight / execute failure) ───────
    if let Some(err) = &error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        // Cancel button (always present — safe default)
        .child(
            Button::new("plan-cancel")
                .label("Cancel")
                .ghost()
                .small()
                .on_click(cancel_handler),
        );

    if let Some(commit_id) = create_branch_target {
        let create_handler = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
            this.cancel_modal();
            this.open_create_branch_modal(commit_id.clone(), cx);
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.notify();
        });
        button_row = button_row.child(
            Button::new("plan-create-branch")
                .label("Create branch here...")
                .small()
                .on_click(create_handler),
        );
    }

    // Checkout button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            Button::new("plan-confirm")
                .label(confirm_label)
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(card)
}

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
            window.focus(&fh);
        }
        cx.notify();
    });

    // ── Confirm handler (only created when no blockers) ─────
    // T-BP-003: return focus to root_focus after confirm.
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_create_branch(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
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
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create branch @ {}  {}",
                    modal.at.short(),
                    modal.start_title
                ))),
        )
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

    // ── Plan state (current → predicted) ─────────────────
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
                        .child(SharedString::from(p.title.clone())),
                ),
        );

        // ── Blockers (localized — W29-I18N-WAVE2) ─────────
        if !p.blockers.is_empty() {
            let lines: Vec<SharedString> = if modal.localized_blockers.is_empty() {
                p.blockers
                    .iter()
                    .map(|b| SharedString::from(b.clone()))
                    .collect()
            } else {
                modal.localized_blockers.clone()
            };
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

        // ── Recovery ──────────────────────────────────────
        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
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
                window.focus(&fh);
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
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_create_worktree(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
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
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create worktree @ {}  {}",
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
                        .child(SharedString::from(p.title.clone())),
                ),
        );

        if !p.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for w in &p.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("! {}", w))),
                );
            }
            card = card.child(warn_col);
        }

        // ── Blockers (localized — W29-I18N-WAVE2) ─────────
        if !p.blockers.is_empty() {
            let lines: Vec<SharedString> = if modal.localized_blockers.is_empty() {
                p.blockers
                    .iter()
                    .map(|b| SharedString::from(b.clone()))
                    .collect()
            } else {
                modal.localized_blockers.clone()
            };
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
                .child(SharedString::from(p.recovery.clone())),
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
                window.focus(&fh);
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
///   - `[Cancel]` always; `[Stash]` only when no blockers
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
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_push(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
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
                .child(SharedString::from("Stash push — save local modifications")),
        )
        // ── Message input ──────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("Message (optional)")),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        );

    // ── Plan state (current → predicted) ─────────────────
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
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(p.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", p.predicted.dirty))),
                        ),
                ),
        );

        // ── Warnings ──────────────────────────────────────
        if !p.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for w in &p.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{26a0} {}", w))),
                );
            }
            card = card.child(warn_col);
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
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        // ── Recovery ──────────────────────────────────────
        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    // ── Error message ──────────────────────────────────
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
        Button::new("stash-push-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(
            KagiButton::accent(
                "stash-push-confirm",
                "Stash",
                current_theme().color_warning,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_stash_push_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
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
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_apply_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_apply(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
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
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ─────────────────────────────
        .child(
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
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
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
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.predicted.dirty))),
                        ),
                ),
        );

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
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

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message ────────────────────────────────────
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
        Button::new("stash-apply-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(
            KagiButton::accent(
                "stash-apply-confirm",
                "Apply",
                current_theme().color_success,
                cx,
            )
            .small()
            .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    modal_overlay(card)
}

// ──────────────────────────────────────────────────────────────
// Cherry-pick modal renderer (T016)
// ──────────────────────────────────────────────────────────────

/// Render the cherry-pick plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title (commit short sha + summary onto HEAD branch)
///   - Current → Predicted state
///   - Preview files section (file tree, reusing T018 build_file_tree)
///   - Blockers (red) if any — includes conflict file names
///   - Recovery text
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always; `[Cherry-pick]` only when no blockers
pub(crate) fn render_cherry_pick_modal(
    modal: CherryPickModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_cherry_pick_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_cherry_pick(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // Change-kind colours come from the active theme (W9-THEME).

    // ── Build preview file tree rows ────────────────────────
    let tree_rows = file_tree::build_file_tree(&plan.preview_files);
    let tree_element_rows: Vec<_> = tree_rows
        .iter()
        .map(|row| {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    div()
                        .id(SharedString::from(format!("cpk-dir-{}", name.as_ref())))
                        .flex()
                        .flex_row()
                        .items_center()
                        .pl(theme::scaled_px(indent))
                        .mb_px()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(current_theme().change_dir))
                                .child(name.clone()),
                        )
                        .into_any()
                }
                file_tree::TreeRow::File {
                    depth,
                    name,
                    file_index,
                    change,
                } => {
                    let indent = (*depth as f32) * 12.0;
                    let (badge_char, badge_color) = match change {
                        ChangeKind::Added => ("A", current_theme().change_added),
                        ChangeKind::Modified => ("M", current_theme().change_modified),
                        ChangeKind::Deleted => ("D", current_theme().change_deleted),
                        ChangeKind::Renamed { .. } => ("R", current_theme().change_renamed),
                        ChangeKind::TypeChange => ("T", current_theme().change_typechange),
                    };
                    let _ = file_index; // not clickable in preview
                    div()
                        .id(("cpk-file", *file_index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .pl(theme::scaled_px(indent))
                        .mb_px()
                        .child(
                            div()
                                .w(theme::scaled_px(14.))
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(rgb(badge_color))
                                .child(SharedString::from(badge_char)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .text_color(rgb(current_theme().text_main))
                                .overflow_hidden()
                                .child(name.clone()),
                        )
                        .into_any()
                }
            }
        })
        .collect();

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(theme::scaled_px(520.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(current_theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ───────────────────────────
        .child(
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
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div().flex().flex_row().gap_2().text_sm().child(
                        div()
                            .text_color(rgb(current_theme().text_main))
                            .child(SharedString::from(plan.predicted.head.clone())),
                    ),
                ),
        );

    // ── Preview files section ─────────────────────────────
    if !plan.preview_files.is_empty() {
        let mut preview_col = div().flex().flex_col().gap_px().child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .mb_1()
                .child(SharedString::from(format!(
                    "Preview ({} file{})",
                    plan.preview_files.len(),
                    if plan.preview_files.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ))),
        );
        for row in tree_element_rows {
            preview_col = preview_col.child(row);
        }
        card = card.child(preview_col);
    }

    // ── Warnings ──────────────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
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

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(current_theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

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
        Button::new("cherry-pick-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(
            Button::new("cherry-pick-confirm")
                .label("Cherry-pick")
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    modal_overlay(card)
}

// ──────────────────────────────────────────────────────────────
// Commit Plan modal renderer (T025)
// ──────────────────────────────────────────────────────────────

/// Render the commit plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Preview files (staged files)
///   - Warnings (unstaged remain)
///   - Error message (if execute failed)
///   - `[Cancel]` always; `[Commit]` when no blockers
pub(crate) fn render_commit_plan_modal(
    modal: CommitPlanModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_commit_plan_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.start_commit(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // ── Preview file tree ────────────────────────────────────
    let tree_rows = file_tree::build_file_tree(&plan.preview_files);
    let mut preview_col = div().flex().flex_col().gap_px().child(
        div()
            .text_sm()
            .text_color(rgb(current_theme().text_label))
            .mb_1()
            .child(SharedString::from(format!(
                "Staging ({} file{})",
                plan.preview_files.len(),
                if plan.preview_files.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ))),
    );

    for row in &tree_rows {
        match row {
            file_tree::TreeRow::Dir { depth, name } => {
                let indent = (*depth as f32) * 12.0;
                preview_col = preview_col.child(
                    div()
                        .id(SharedString::from(format!("cpk-dir-{}", name.as_ref())))
                        .pl(theme::scaled_px(indent))
                        .text_xs()
                        .text_color(rgb(current_theme().change_dir))
                        .child(name.clone()),
                );
            }
            file_tree::TreeRow::File {
                depth,
                name,
                file_index,
                change,
            } => {
                let indent = (*depth as f32) * 12.0;
                let (badge, badge_color, _) = status_badge(change, false);
                let _ = file_index;
                preview_col = preview_col.child(
                    div()
                        .id(("cpk-file", *file_index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .pl(theme::scaled_px(indent))
                        .child(
                            div()
                                .w(theme::scaled_px(14.))
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(badge_color))
                                .child(SharedString::from(badge)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(current_theme().text_main))
                                .overflow_hidden()
                                .child(name.clone()),
                        ),
                );
            }
        }
    }

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
                .child(SharedString::from(plan.title.clone())),
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
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
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
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from(plan.predicted.head.clone())),
                ),
        )
        // Preview files
        .child(preview_col);

    // Warnings
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // Blockers
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
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

    // Error
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
        Button::new("commit-plan-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        button_row = button_row.child(
            Button::new("commit-plan-confirm")
                .label("Commit")
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    modal_overlay(card)
}

// ──────────────────────────────────────────────────────────────
// Smart Commit modal renderer (T-COMMIT-016, ADR-0044)
// ──────────────────────────────────────────────────────────────

/// Render the Smart Commit consent / model-picker overlay.
///
/// * `Consent` — the first-time opt-in dialog carrying the four mandated
///   statements ([`smart_commit::CONSENT_LINES`]).  Confirm enables LLM
///   generation and proceeds to model selection.
/// * `ModelPicker` — choose one installed model; the choice is persisted.
pub(crate) fn render_smart_commit_modal(
    modal: smart_commit::SmartCommitModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let card = match modal {
        smart_commit::SmartCommitModal::Consent => {
            let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.cancel_smart_modal(cx);
            });
            let confirm = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.confirm_smart_consent(cx);
            });
            let mut lines_col = div().flex().flex_col().gap_1();
            for line in smart_commit::CONSENT_LINES {
                lines_col = lines_col.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(current_theme().color_branch))
                                .child(SharedString::from("•")),
                        )
                        .child(
                            div()
                                .text_color(rgb(current_theme().text_main))
                                .child(SharedString::from(line)),
                        ),
                );
            }
            div()
                .w(theme::scaled_px(460.))
                .bg(rgb(current_theme().modal))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xl()
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from("Enable Local LLM generation?")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_sub))
                        .child(SharedString::from(
                            "Pressing Generate sends your staged diff to a local Ollama \
                             model on this machine. Please review:",
                        )),
                )
                .child(lines_col)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("smart-consent-cancel")
                                .label("Cancel")
                                .ghost()
                                .small()
                                .on_click(cancel),
                        )
                        .child(
                            KagiButton::accent(
                                "smart-consent-confirm",
                                "Enable & continue",
                                current_theme().color_success,
                                cx,
                            )
                            .small()
                            .on_click(confirm),
                        ),
                )
        }
        smart_commit::SmartCommitModal::ModelPicker { models } => {
            let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.cancel_smart_modal(cx);
            });
            let mut list = div().flex().flex_col().gap_1();
            for (i, m) in models.iter().enumerate() {
                let model_name = m.clone();
                let pick = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    this.choose_smart_model(model_name.clone(), window, cx);
                });
                list = list.child(
                    div()
                        .id(("smart-model", i))
                        .px_3()
                        .py_1()
                        .rounded_sm()
                        .bg(rgb(current_theme().surface))
                        .text_sm()
                        .text_color(rgb(current_theme().text_main))
                        .on_click(pick)
                        .hover(|s| s.bg(rgb(current_theme().selected)).cursor_pointer())
                        .child(SharedString::from(m.clone())),
                );
            }
            div()
                .w(theme::scaled_px(420.))
                .bg(rgb(current_theme().modal))
                .rounded_lg()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xl()
                        .text_color(rgb(current_theme().text_main))
                        .child(SharedString::from("Select a local model")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_sub))
                        .child(SharedString::from(
                            "Choose which installed Ollama model to use. \
                             Your choice is remembered.",
                        )),
                )
                .child(list)
                .child(
                    div().flex().flex_row().justify_end().child(
                        Button::new("smart-model-cancel")
                            .label("Cancel")
                            .ghost()
                            .small()
                            .on_click(cancel),
                    ),
                )
        }
    };

    modal_overlay(card)
}

/// Auto-update detail modal (ADR-0082, T-AUTOUPDATE-001).
///
/// Shows current → latest, the chosen asset, release notes, and — Phase 1 — a
/// "Update now" button that downloads + verifies + installs + relaunches. Phase-0
/// fallbacks ("Open release page", "Skip this version") are always present.
pub(crate) fn render_update_modal(
    plan: kagi_domain::update::UpdatePlan,
    installing: bool,
    status: Option<SharedString>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.update_modal_open = false;
        cx.notify();
    });
    let update_now = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.start_update_install(cx);
    });
    let skip = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.skip_this_update(cx);
    });
    let open_page = cx.listener(|this, _e: &gpui::ClickEvent, _w, _cx| {
        this.open_release_page();
    });

    // Release notes can be long, so size the card to a fraction of the window
    // (issue #29 comment): 0.8× viewport width, and the notes pane scrolls within
    // ~half the viewport height. The notes text is rendered at ~0.7× the current
    // UI zoom so more fits.
    let viewport = window.viewport_size();
    let card_w = px((f32::from(viewport.width) * 0.8).max(360.0));
    let notes_h = px((f32::from(viewport.height) * 0.5).max(160.0));
    let notes_font = px((theme::rem_size_px() * 0.7).max(9.0));

    let mut card =
        div()
            .w(card_w)
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
                    .child(SharedString::from("Update available")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .text_sm()
                    .child(div().text_color(rgb(current_theme().text_sub)).child(
                        SharedString::from(format!(
                            "v{}.{}.{}",
                            plan.current.major, plan.current.minor, plan.current.patch
                        )),
                    ))
                    .child(
                        div()
                            .text_color(rgb(current_theme().text_label))
                            .child(SharedString::from("\u{2192}")),
                    )
                    .child(
                        div()
                            .text_color(rgb(current_theme().text_main))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(SharedString::from(plan.tag.clone())),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_sub))
                    .child(SharedString::from(format!(
                        "{}  ({:.1} MB)",
                        plan.asset.name,
                        plan.asset.size as f64 / 1_048_576.0
                    ))),
            );

    // Release notes — rendered as Markdown (gpui-component TextView). The
    // TextView scrolls itself (`.scrollable(true)`) inside a fixed-height pane;
    // the earlier `overflow_y_scroll` wrapper did not actually scroll the async
    // TextView. Heading/body sizes are scaled to ~0.7× the UI zoom, and the
    // markdown style follows the active (dark) theme so code blocks render right.
    if !plan.notes.trim().is_empty() {
        use gpui_component::text::TextViewStyle;
        use gpui_component::ActiveTheme as _;

        let highlight_theme = cx.theme().highlight_theme.clone();
        let is_dark = cx.theme().mode.is_dark();
        let tv_style = TextViewStyle {
            heading_base_font_size: notes_font,
            highlight_theme,
            is_dark,
            ..Default::default()
        };

        card = card.child(
            div()
                .id("update-notes")
                .h(notes_h)
                .w_full()
                .p_2()
                .rounded_md()
                .bg(rgb(current_theme().bg_base))
                .text_color(rgb(current_theme().text_main))
                .text_size(notes_font)
                .child(
                    gpui_component::text::TextView::markdown(
                        "update-notes-md",
                        SharedString::from(plan.notes.clone()),
                        window,
                        cx,
                    )
                    .scrollable(true)
                    .style(tv_style),
                ),
        );
    }

    if let Some(s) = status {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_warning))
                .child(s),
        );
    }

    // Buttons row.
    let mut actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            Button::new("update-skip")
                .label("Skip this version")
                .ghost()
                .small()
                .on_click(skip),
        )
        .child(
            Button::new("update-page")
                .label("Release page")
                .ghost()
                .small()
                .on_click(open_page),
        )
        .child(div().flex_grow())
        .child(
            Button::new("update-cancel")
                .label("Later")
                .ghost()
                .small()
                .on_click(cancel),
        );
    if installing {
        actions = actions.child(
            Button::new("update-now")
                .label("Updating…")
                .primary()
                .small()
                .loading(true)
                .disabled(true)
                .on_click(|_, _, _| {}),
        );
    } else {
        actions = actions.child(
            Button::new("update-now")
                .label("Update now")
                .primary()
                .small()
                .on_click(update_now),
        );
    }
    card = card.child(actions);

    modal_overlay(card).into_any_element()
}

// ──────────────────────────────────────────────────────────────
// ActiveModal — single active-modal enum (ADR-0076 / issue #13 P7)
// ──────────────────────────────────────────────────────────────

// ActiveModal moved back to modals.rs (state, not rendering).
