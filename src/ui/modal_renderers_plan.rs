//! Plan-confirmation modal renderers split out of `modal_renderers.rs`
//! (T-SPLIT-MODALS-001 / ADR-0116 Wave 3). These are the thin per-modal
//! wrappers that build a cancel/confirm listener pair and delegate the card to
//! the shared `render_plan_modal_card` / `render_input_plan_modal` helpers (which
//! stay in `modal_renderers.rs`). Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::i18n::Msg;
use super::modal_renderers::{
    render_input_plan_modal, render_plan_modal_wrapper, render_plan_modal_wrapper_styled,
};
use super::modals::*;
use super::theme::{self as theme_mod, theme};
use super::{KagiApp, MONO_FONT};
use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::IconName;
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

/// Pull plan confirmation overlay (T-HT-003) — richer icon-badge card
/// (user request 2026-07-22: "make the popup cards richer, starting with
/// Pull/Push"), wired to `confirm_pull`. Accent colour matches the toolbar's
/// Pull button (`color_branch`, same as the ↓N chip).
pub(crate) fn render_pull_modal(
    modal: PullPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // W3-NOTIFY: confirm runs on a background thread (start/finish toasts).
    render_plan_modal_wrapper_styled(
        modal.plan,
        modal.error,
        "Pull",
        None,
        Some((IconName::ArrowDown, theme().color_branch)),
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

/// Push plan confirmation overlay (T-HT-004) — richer icon-badge card (see
/// `render_pull_modal`), wired to `confirm_push`. Accent colour is
/// `color_success` (green "sending" feel), distinct from Pull's blue so the
/// two are scannable at a glance.
pub(crate) fn render_push_modal(
    modal: PushPlanModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // W3-NOTIFY: confirm runs on a background thread (start/finish toasts).
    render_plan_modal_wrapper_styled(
        modal.plan,
        modal.error,
        "Push",
        None,
        Some((IconName::ArrowUp, theme().color_success)),
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

/// Rebase-current-onto confirmation overlay (branch-menu "Integrate" group).
/// Single confirm, no armed second stage — see `operations/rebase.rs`'s
/// module doc for why (Guarded, not Destructive; a conflict routes into the
/// existing conflict editor rather than losing anything).
pub(crate) fn render_rebase_modal(
    modal: RebaseCurrentOntoModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper(
        modal.plan,
        modal.error,
        format!("Rebase {}", modal.branch),
        None,
        |this, _cx| this.cancel_rebase_modal(),
        |this, cx| this.start_rebase(cx),
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

/// Recovery box (Pull/Push only). Two earlier passes boxed this section —
/// first a bordered card with bordered "chip" pills around each `git ...`
/// command, then a further tinted "NOTE" callout around the prose — but
/// chrome kept compounding: a card inside a card, pills that looked tappable
/// for text that wasn't, and boxes-within-boxes read as busier, not more
/// readable (user follow-ups 2026-07-22). A cross-model design review (codex
/// and a Claude planning pass, prompted by the user asking for "Appleらしい"
/// taste) converged independently on the same fix: delete the chrome and let
/// typography carry the hierarchy instead. So: no outer card, no pill
/// borders. Prose recedes (small, muted, proportional); `git ...` commands
/// step forward in monospace against a single hairline accent rule that ties
/// them back to the modal's colour without adding another filled box.
///
/// Lines are grouped by consecutive kind — prose vs. `git `-prefixed command
/// — purely by shape, so a caption ("if the push is rejected...") stays
/// visually attached to the command(s) right after it, with more space
/// between groups than within one. This scans the ALREADY-LOCALIZED display
/// text for lines that *look like* a command — a per-line cosmetic choice,
/// never a behavioural one, same category as the commit-row sha/summary
/// split in `modal_renderers.rs` (ADR-0129 keeps string-sniffing out of
/// *decisions*, not out of *display shape*). A recovery whose prose has no
/// command lines (or none at all) just shows as plain paragraphs.
pub(crate) fn render_recovery_box(text: &str, color: u32) -> gpui::AnyElement {
    let (_, rule_color, _) = theme_mod::badge_style(color);

    let mut groups: Vec<(bool, Vec<&str>)> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let is_command = line.starts_with("git ");
        match groups.last_mut() {
            Some((last_is_command, lines)) if *last_is_command == is_command => lines.push(line),
            _ => groups.push((is_command, vec![line])),
        }
    }

    let mut col = div()
        .flex()
        .flex_col()
        .pt(theme_mod::scaled_px(6.))
        .gap(theme_mod::scaled_px(10.));
    for (is_command, lines) in groups {
        col = col.child(if is_command {
            render_recovery_commands(&lines, rule_color)
        } else {
            render_recovery_prose(&lines)
        });
    }
    col.into_any_element()
}

/// A consecutive run of non-command recovery lines: small, proportional, and
/// de-emphasized only through type family/rule (not through crushing
/// contrast) — `text_muted` measures under 2:1 against this modal's
/// background, well below legible-body-text range, so hierarchy here comes
/// from `text_sub` (same colour as the commands below) plus the surrounding
/// spacing, never from a hard-to-read grey (user follow-up 2026-07-22).
fn render_recovery_prose(lines: &[&str]) -> gpui::AnyElement {
    let mut block = div().flex().flex_col().gap(theme_mod::scaled_px(3.));
    for line in lines {
        block = block.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(line.to_string())),
        );
    }
    block.into_any_element()
}

/// A consecutive run of `git ...` command lines: monospace, set off from the
/// prose by a single hairline accent rule — no fill, no border-all-round, so
/// it can't be mistaken for a button.
fn render_recovery_commands(lines: &[&str], rule_color: u32) -> gpui::AnyElement {
    let mut block = div().flex().flex_col().gap(theme_mod::scaled_px(2.));
    for line in lines {
        block = block.child(
            div()
                .font_family(MONO_FONT)
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(line.to_string())),
        );
    }
    div()
        .pl(theme_mod::scaled_px(8.))
        .border_l_1()
        .border_color(gpui::rgba(rule_color))
        .child(block)
        .into_any_element()
}
