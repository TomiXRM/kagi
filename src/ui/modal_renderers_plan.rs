//! Plan-confirmation modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). These are the thin per-modal
//! wrappers that build a cancel/confirm listener pair and delegate the card to
//! the shared `render_plan_modal_card` / `render_input_plan_modal` helpers (which
//! stay in `modal_renderers.rs`). Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::i18n::Msg;
use super::modal_renderers::{render_input_plan_modal, render_plan_modal_card};
use super::modals::*;
use super::KagiApp;
use gpui::{prelude::*, Context, SharedString};
use kagi_git::{MergeKind, OperationPlan};

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
