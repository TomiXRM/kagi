//! Plan-confirmation modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). These are the thin per-modal
//! wrappers that build a cancel/confirm listener pair and delegate the card to
//! the shared `render_plan_modal_card` / `render_input_plan_modal` helpers (which
//! stay in `modal_renderers.rs`). Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::i18n::Msg;
use super::modal_renderers::{render_input_plan_modal, render_plan_modal_wrapper};
use super::modals::*;
use super::KagiApp;
use gpui::{Context, SharedString};
use kagi_git::{MergeKind, OperationPlan};

pub(crate) fn render_plan_modal(
    modal: CheckoutPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let create_branch_target = match &modal.target {
        CheckoutPlanTarget::Commit(commit_id) => Some(commit_id.clone()),
        CheckoutPlanTarget::Branch(_) => None,
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Checkout",
        create_branch_target,
        |this, _cx| this.cancel_modal(),
        |this, cx| this.start_checkout(cx),
        cx,
    )
}

/// Pull plan confirmation overlay (T-HT-003) — same card as the checkout
/// plan modal, wired to `confirm_pull`.
pub(crate) fn render_pull_modal(
    modal: PullPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // W3-NOTIFY: confirm runs on a background thread (start/finish toasts).
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Pull",
        None,
        |this, _cx| this.cancel_pull_modal(),
        |this, cx| this.start_pull(cx),
        cx,
    )
}

/// Undo-commit confirmation overlay (T-HT-009).
pub(crate) fn render_undo_modal(
    modal: UndoPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Undo",
        None,
        |this, _cx| this.cancel_undo_modal(),
        |this, cx| this.confirm_undo(cx),
        cx,
    )
}

/// Operation-history Undo / Redo confirmation overlay (T-UNDOREDO-001,
/// ADR-0081). Confirming runs the safe ref move through the standard pipeline
/// and advances/retreats the history cursor.
pub(crate) fn render_history_modal(
    modal: HistoryPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let confirm_label = if modal.is_undo {
        Msg::Undo.t()
    } else {
        Msg::Redo.t()
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        confirm_label,
        None,
        |this, _cx| this.clear_history_modal(),
        |this, cx| this.confirm_history(cx),
        cx,
    )
}

/// Sequencer `<op> --continue` confirmation overlay (ADR-0068 /
/// T-CONFLICT-FLOW-032).  Shown when Continue routes a rebase / cherry-pick /
/// revert; confirming advances the sequencer.
pub(crate) fn render_conflict_continue_modal(
    modal: ConflictContinuePlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        Msg::ConflictContinue.t(),
        None,
        |this, _cx| this.cancel_conflict_continue(),
        |this, cx| this.confirm_conflict_continue(cx),
        cx,
    )
}

/// Stash-pop confirmation overlay (T-HT-007).
pub(crate) fn render_pop_modal(modal: PopPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Pop",
        None,
        |this, _cx| this.cancel_pop_modal(),
        |this, cx| this.start_pop(cx),
        cx,
    )
}

/// Stash drop confirmation overlay (ADR-0087) — Destructive: deletes the
/// stash entry without touching the working tree. Same card as Pop, wired to
/// `start_stash_drop`.
pub(crate) fn render_stash_drop_modal(
    modal: StashDropModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Drop",
        None,
        |this, _cx| this.cancel_stash_drop_modal(),
        |this, cx| this.start_stash_drop(cx),
        cx,
    )
}

/// Unlock-worktree confirmation overlay — the plan warning carries the
/// recorded lock reason; confirming removes the lock (admin-only, never
/// touches any working tree).
pub(crate) fn render_unlock_worktree_modal(
    modal: UnlockWorktreeModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Unlock",
        None,
        |this, _cx| this.cancel_unlock_worktree_modal(),
        |this, cx| this.confirm_unlock_worktree(cx),
        cx,
    )
}

/// Push plan confirmation overlay (T-HT-004) — same card as the pull
/// plan modal, wired to `confirm_push`.
pub(crate) fn render_push_modal(
    modal: PushPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // W3-NOTIFY: confirm runs on a background thread (start/finish toasts).
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Push",
        None,
        |this, _cx| this.cancel_push_modal(),
        |this, cx| this.start_push(cx),
        cx,
    )
}

pub(crate) fn render_branch_plan_modal(
    modal: BranchPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let label = match modal.kind {
        BranchPlanKind::PullFfOnly => "Pull",
        BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => "Push",
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        label,
        None,
        |this, _cx| this.cancel_branch_plan_modal(),
        |this, cx| this.start_branch_plan(cx),
        cx,
    )
}

pub(crate) fn render_set_upstream_modal(
    modal: SetUpstreamModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_set_upstream_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_set_upstream(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
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
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_rename_branch(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
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
    // W31-MERGE-INTO-CONFLICT: a conflict-producing merge gets a localized
    // Confirm-button label. The full "Merge <source> into <current>" context is
    // already the modal's title (the plan title), so the button stays short —
    // just "Merge" — so a long branch name (e.g. origin/FW/MainBoard_V26.1) can't
    // overflow the button width. The conflict case keeps its short localized
    // hint + a prominent warning banner prepended to the plan's warnings.
    let (confirm_label, plan): (SharedString, std::sync::Arc<OperationPlan>) =
        if matches!(modal.kind, MergeKind::Conflicts(_)) {
            let mut plan = (*modal.plan).clone();
            plan.warnings.insert(
                0,
                kagi_git::ops::PlanNote::Common(kagi_git::ops::CommonNote::MergeConflictWarning),
            );
            (
                SharedString::from(Msg::MergeAndResolveConflicts.t()),
                std::sync::Arc::new(plan),
            )
        } else {
            (SharedString::from("Merge"), modal.plan)
        };
    render_plan_modal_wrapper(
        plan,
        modal.error,
        confirm_label,
        None,
        |this, _cx| this.cancel_merge_modal(),
        |this, cx| this.start_merge(cx),
        cx,
    )
}

pub(crate) fn render_tracking_checkout_modal(
    modal: TrackingCheckoutPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Checkout",
        None,
        |this, _cx| this.cancel_tracking_checkout_modal(),
        |this, cx| this.start_tracking_checkout(cx),
        cx,
    )
}

/// Switch-to-latest confirmation overlay (ADR-0101).
pub(crate) fn render_switch_to_latest_modal(
    modal: SwitchToLatestPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Switch",
        None,
        |this, _cx| this.cancel_switch_to_latest_modal(),
        |this, cx| this.start_switch_to_latest(cx),
        cx,
    )
}

/// Delete-branch confirmation overlay (W2-DELETE).
pub(crate) fn render_delete_branch_modal(
    modal: DeleteBranchModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Delete",
        None,
        |this, _cx| this.cancel_delete_branch_modal(),
        |this, cx| this.start_delete_branch(cx),
        cx,
    )
}

/// Delete-remote-branch confirmation overlay (branch-menu "Advanced /
/// Dangerous" group). Two-stage confirm: the first click only arms the
/// button (label switches to an explicit "really delete" warning); the
/// second click executes. Mirrors `DiscardModal`'s `confirm_armed` pattern
/// via the shared plan-modal card rather than a bespoke renderer.
pub(crate) fn render_delete_remote_branch_modal(
    modal: DeleteRemoteBranchModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let confirm_label: SharedString = if modal.confirm_armed {
        SharedString::from("\u{26a0} Really delete — cannot be undone")
    } else {
        SharedString::from("Delete remote branch")
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        confirm_label,
        None,
        |this, _cx| this.cancel_delete_remote_branch_modal(),
        |this, cx| this.start_delete_remote_branch(cx),
        cx,
    )
}

/// Reset-current-to-HEAD confirmation overlay (branch-menu "Advanced /
/// Dangerous" group). Two-stage confirm, same shape as
/// `render_delete_remote_branch_modal`.
pub(crate) fn render_reset_current_modal(
    modal: ResetCurrentModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let confirm_label: SharedString = if modal.confirm_armed {
        SharedString::from("\u{26a0} Really reset — cannot be undone")
    } else {
        SharedString::from("Reset current branch")
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        confirm_label,
        None,
        |this, _cx| this.cancel_reset_current_modal(),
        |this, cx| this.start_reset_current(cx),
        cx,
    )
}

/// Force-with-lease push confirmation overlay (branch-menu "Advanced /
/// Dangerous" group). Two-stage confirm, same shape as
/// `render_reset_current_modal`.
pub(crate) fn render_force_lease_push_modal(
    modal: ForceLeasePushModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let confirm_label: SharedString = if modal.confirm_armed {
        SharedString::from("\u{26a0} Really force-push — cannot be undone")
    } else {
        SharedString::from("Force-with-lease push")
    };
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        confirm_label,
        None,
        |this, _cx| this.cancel_force_lease_push_modal(),
        |this, cx| this.start_force_lease_push(cx),
        cx,
    )
}

/// Branch-cleanup confirmation overlay (ADR-0128). Reuses the shared plan
/// card: the plan's `preview_commits` already lists the branches (with their
/// local/origin tips), and blockers hide the confirm button.
pub(crate) fn render_branch_cleanup_modal(
    modal: BranchCleanupModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan.clone(),
        modal.error.clone(),
        "Delete",
        None,
        |this, _cx| this.cancel_branch_cleanup_modal(),
        |this, cx| this.confirm_branch_cleanup(cx),
        cx,
    )
}

/// Revert confirmation overlay (T-CM-034).
pub(crate) fn render_revert_modal(
    modal: RevertModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        "Revert",
        None,
        |this, _cx| this.cancel_revert_modal(),
        |this, cx| this.start_revert(cx),
        cx,
    )
}
