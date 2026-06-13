use super::commit_panel::{status_badge, CommitPlanModal};
use super::i18n::Msg;
use super::theme::{self, theme as current_theme};
use super::{file_tree, smart_commit, KagiApp};

use gpui::{
    div, prelude::*, rgb, App, Context, Entity, FocusHandle, KeyDownEvent, SharedString, Window,
};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::{Input, InputState};
use gpui_component::Sizable as _;
use kagi::git::{
    ops::{AmendMode, BranchRenameValidation, MergeKind, OperationPlan},
    ChangeKind, CommitId,
};

// ──────────────────────────────────────────────────────────────
// CheckoutPlanModal — state for the plan confirmation overlay (T013)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress checkout plan confirmation.
#[derive(Clone)]
pub struct CheckoutPlanModal {
    /// Branch or commit target captured when the plan was opened.
    pub target: CheckoutPlanTarget,
    /// When `true` (Enter-checkout on a dirty tree), confirm stashes the
    /// working-tree changes first, then checks out.
    pub stash_first: bool,
    /// The computed plan (title, current, predicted, warnings, blockers, recovery).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed (replaces normal buttons).
    pub error: Option<SharedString>,
}

/// Execution target for the shared checkout plan modal.
#[derive(Clone, Debug)]
pub enum CheckoutPlanTarget {
    Branch(String),
    Commit(CommitId),
}

/// State for an in-progress pull confirmation (T-HT-003).  Same shape as
/// [`CheckoutPlanModal`] but kept separate so the confirm path can't be mixed up.
#[derive(Clone)]
pub struct PullPlanModal {
    /// The computed pull plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

/// State for an in-progress undo-commit confirmation (T-HT-009).
#[derive(Clone)]
pub struct UndoPlanModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

/// State for a sequencer (rebase / cherry-pick / revert) conflict-continue
/// confirmation (ADR-0068 / T-CONFLICT-FLOW-032).  A `git <op> --continue` plan
/// shown before the sequencer is advanced.  Merge does NOT use this modal — it
/// routes to the commit panel instead.
#[derive(Clone)]
pub struct ConflictContinuePlanModal {
    /// The computed `<op> --continue` plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute failed (replaces the confirm button).
    pub error: Option<SharedString>,
}

/// State for an in-progress amend confirmation (T-COMMIT-011, ADR-0040).
///
/// Amend is history-rewriting (ADR-0023) so the modal requires a **two-stage
/// confirmation**: the first Confirm click *arms* the action (`confirm_armed`
/// flips to `true` and the button text changes to a final, explicit confirm),
/// and only the second click executes.
#[derive(Clone)]
pub struct AmendPlanModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Which amend mode this plan was built for.
    pub mode: AmendMode,
    /// The new message (for MessageOnly / Both); ignored for Staged.
    pub message: String,
    /// Two-stage confirm gate: `false` = first click pending, `true` = armed.
    pub confirm_armed: bool,
}

/// State for an in-progress stash-pop confirmation (T-HT-007).
#[derive(Clone)]
pub struct PopPlanModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Stash index the plan was built for.
    pub stash_index: usize,
}

/// State for an in-progress push confirmation (T-HT-004).  Same shape as
/// [`PullPlanModal`] but kept separate so the confirm path can't be mixed up.
#[derive(Clone)]
pub struct PushPlanModal {
    /// The computed push plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

/// State for an in-progress branch merge confirmation (T-BCM-030).
#[derive(Clone)]
pub struct MergePlanModal {
    /// The branch merged INTO HEAD (the "source" in user terms; the argument to
    /// `git merge`).
    pub target: String,
    /// The current/checked-out branch the source is merged into (destination).
    /// Drives the explicit confirm-button label `Merge <target> into
    /// <into_branch>` (ADR-0079 / T-DNDMERGE-001).
    pub into_branch: String,
    pub plan: std::sync::Arc<OperationPlan>,
    /// W31-MERGE-INTO-CONFLICT: what executing this merge will do. When
    /// [`MergeKind::Conflicts`] the modal shows a "resolve conflicts" confirm
    /// and `start_merge` drives the real merge into Conflict Mode.
    pub kind: MergeKind,
    pub error: Option<SharedString>,
}

/// State for creating a local tracking branch from a remote branch and checking
/// it out as one operation (T-BCM-061).
#[derive(Clone)]
pub struct TrackingCheckoutPlanModal {
    pub remote_branch: String,
    pub local_branch: String,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CreateBranchModal — state for the create-branch overlay (T014)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress create-branch confirmation.
///
/// The user types a branch name; the plan is regenerated live on each keystroke.
#[derive(Clone)]
pub struct CreateBranchModal {
    /// The commit at which the branch will be created.
    pub at: CommitId,
    /// First line of the start commit message, used to identify menu origin.
    pub start_title: String,
    /// Current text in the branch-name input field (synced from `input_state`).
    pub input: String,
    /// Real text-input entity (gpui-component). Created lazily on first
    /// render (needs a Window); `None` in headless paths.
    pub input_state: Option<Entity<InputState>>,
    /// Whether to check out the new branch after creating it.
    pub checkout_after: bool,
    /// Live plan (re-generated each keystroke from `input` and `at`).
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
    /// Localized blocker texts for display (W29-I18N-WAVE2). The keyed
    /// branch-name reasons are localized; non-keyed plan blockers pass through
    /// in English. Recomputed each replan. The execute-guard still uses
    /// `plan.blockers` (English) so behaviour is unchanged.
    pub localized_blockers: Vec<SharedString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // legacy hand-rolled input era; kept for struct compat
pub enum WorktreeModalField {
    Branch,
    Path,
}

/// State for an in-progress create-worktree confirmation.
#[derive(Clone)]
pub struct CreateWorktreeModal {
    /// The commit used as the start point for the new branch.
    pub at: CommitId,
    /// First line of the start commit message.
    pub start_title: String,
    /// New branch name (synced from `branch_state`).
    pub branch_input: String,
    /// Real branch-name input entity (lazy; None headless).
    pub branch_state: Option<Entity<InputState>>,
    /// Target worktree path (synced from `path_state`).
    pub path_input: String,
    /// Real path input entity (lazy; None headless).
    pub path_state: Option<Entity<InputState>>,
    /// True once the user has manually edited the path.
    pub path_touched: bool,
    /// True when this modal attaches an existing local branch to a worktree
    /// instead of creating a new branch first.
    pub allow_existing_branch: bool,
    /// Which field receives key input (legacy hand-rolled input era; the
    /// real `InputState`s manage their own focus now).
    #[allow(dead_code)]
    pub active_field: WorktreeModalField,
    /// Live plan regenerated from branch/path/start.
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
    /// Localized blocker texts for display (W29-I18N-WAVE2). The keyed
    /// branch-name and worktree-path reasons are localized; non-keyed plan
    /// blockers pass through in English. Recomputed each replan.
    pub localized_blockers: Vec<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// StashPushModal — state for the stash push confirmation overlay (T015)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress stash push confirmation.
///
/// The user may optionally type a stash message; the live plan is regenerated
/// on each keystroke.
#[derive(Clone)]
pub struct StashPushModal {
    /// Optional stash message (empty string → None passed to stash_save2).
    /// Synced from `input_state`.
    pub input: String,
    /// Real text-input entity (lazy; None headless).
    pub input_state: Option<Entity<InputState>>,
    /// Live plan (re-generated each keystroke from `input`).
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// StashApplyModal — state for the stash apply confirmation overlay (T015)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress stash apply confirmation.
#[derive(Clone)]
pub struct StashApplyModal {
    /// The stash index to apply.
    pub index: usize,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CherryPickModal — state for the cherry-pick plan overlay (T016)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress cherry-pick plan confirmation.
///
/// The modal shows a preview of affected files and any blockers before
/// the user confirms execution.
#[derive(Clone)]
pub struct CherryPickModal {
    /// The commit id that will be cherry-picked.
    pub commit_id: CommitId,
    /// The computed plan (title, current, predicted, preview_files, blockers, recovery).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// RevertModal — state for the revert plan overlay (T-CM-034)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress revert plan confirmation.
#[derive(Clone)]
pub struct RevertModal {
    /// The commit id that will be reverted.
    pub commit_id: CommitId,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// DeleteBranchModal — state for the delete-branch confirmation overlay (W2-DELETE)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress delete-branch confirmation (W2-DELETE).
///
/// The modal shows blockers (unmerged / current branch) and the recovery
/// `git branch <name> <sha>` string before the user confirms.
#[derive(Clone)]
pub struct DeleteBranchModal {
    /// The local branch name to delete.
    pub branch_name: String,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if preflight or execute failed.
    pub error: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchPlanKind {
    PullFfOnly,
    Push,
    PushSetUpstream,
}

#[derive(Clone)]
pub struct BranchPlanModal {
    pub kind: BranchPlanKind,
    pub branch_name: String,
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

#[derive(Clone)]
pub struct SetUpstreamModal {
    pub branch_name: String,
    pub input: String,
    pub input_state: Option<Entity<InputState>>,
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    pub error: Option<SharedString>,
}

#[derive(Clone)]
pub struct RenameBranchModal {
    pub old_name: String,
    pub input: String,
    pub input_state: Option<Entity<InputState>>,
    pub validation: BranchRenameValidation,
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    pub error: Option<SharedString>,
}

/// State for an in-progress discard confirmation (W17-DISCARD, ADR-0046).
///
/// Danger modal: shows the target file list, any skipped (untracked/conflicted)
/// files, the recovery note, and a red Discard button. `paths` is the exact set
/// passed to `execute_discard` (untracked/conflicted already excluded).
#[derive(Clone)]
pub struct DiscardModal {
    /// The computed plan (`destructive: true`).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Repo-relative paths that will be discarded (one operation).
    pub paths: Vec<String>,
    /// Repo-relative paths shown as "skipped" (untracked / conflicted).
    pub skipped: Vec<String>,
    /// Whether this was launched from the "Discard all" header button.
    pub is_all: bool,
    /// Error message to show if preflight or execute failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// Plan modal renderer (T013)
// ──────────────────────────────────────────────────────────────

/// Render the plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Current → Predicted state
///   - Warnings (yellow) if any
///   - Blockers (red) if any
///   - Recovery text
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always present; `[Checkout]` only when no blockers
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
        this.confirm_undo();
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
        div()
            .id("amend-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    if !has_blockers {
        // Stage 1 label = "Amend\u{2026}", stage 2 (armed) = red "Rewrite history".
        let (label, bg) = if armed {
            ("Rewrite history", current_theme().color_blocker)
        } else {
            ("Amend\u{2026}", current_theme().color_branch)
        };
        button_row = button_row.child(
            div()
                .id("amend-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(bg))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from(label)),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper (matches render_plan_modal_card) ──
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
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
        div()
            .id("branch-input-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );
    if !has_blockers {
        buttons = buttons.child(
            div()
                .id("branch-input-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_branch))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from(confirm_label)),
        );
    }
    card = card.child(buttons);

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
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
    // "resolve conflicts" confirm label and a prominent localized warning banner
    // prepended to the plan's (English, git-layer) per-file warning.
    // T-DNDMERGE-001 / ADR-0079: the confirm button must be explicit, not vague —
    // `Merge <source> into <current>` (domain words / branch names stay English).
    let (confirm_label, plan): (SharedString, std::sync::Arc<OperationPlan>) =
        if matches!(modal.kind, MergeKind::Conflicts(_)) {
            let mut plan = (*modal.plan).clone();
            plan.warnings
                .insert(0, Msg::MergeConflictWarning.t().to_string());
            let label = SharedString::from(format!(
                "Merge {} into {} ({})",
                modal.target,
                modal.into_branch,
                Msg::MergeAndResolveConflicts.t()
            ));
            (label, std::sync::Arc::new(plan))
        } else {
            let label =
                SharedString::from(format!("Merge {} into {}", modal.target, modal.into_branch));
            (label, modal.plan)
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

    // ── Buttons ─────────────────────────────────────────────
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        div()
            .id("discard-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );
    if can_discard {
        button_row = button_row.child(
            div()
                .id("discard-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_blocker))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Discard")),
        );
    }
    card = card.child(button_row);

    // ── Full-screen overlay: backdrop + card, BOTH occluded ──
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .on_key_down(esc_cancel)
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
                // ADR-0046 / W17: the card itself must also occlude, else clicks
                // fall through to the UI beneath (known click-through bug).
                .child(card.occlude()),
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
            div()
                .id("plan-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().surface))
                .text_sm()
                .text_color(rgb(current_theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(current_theme().selected)))
                .child(SharedString::from("Cancel")),
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
            div()
                .id("plan-create-branch")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().surface))
                .text_sm()
                .text_color(rgb(current_theme().text_main))
                .on_click(create_handler)
                .hover(|style| style.bg(rgb(current_theme().selected)))
                .child(SharedString::from("Create branch here...")),
        );
    }

    // Checkout button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_branch))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from(confirm_label)),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────────────────
    // Two layers: backdrop (semi-transparent) + centred card.
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Backdrop (dark, semi-transparent).
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
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
        this.confirm_create_branch();
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
            if let Some(ref mut modal) = this.create_branch_modal {
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
        div()
            .id("create-branch-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    // Create button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("create-branch-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_success))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Create")),
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

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
                .child(focusable_card),
        )
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
        div()
            .id("create-worktree-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("create-worktree-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_success))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Create")),
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

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
                .child(focusable_card),
        )
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
        div()
            .id("stash-push-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-push-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_warning))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Stash")),
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

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
                .child(focusable_card),
        )
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
        this.confirm_stash_apply();
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
        div()
            .id("stash-apply-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-apply-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_success))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Apply")),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
        div()
            .id("cherry-pick-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("cherry-pick-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().accent)) // mauve accent
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Cherry-pick")),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
        div()
            .id("commit-plan-cancel")
            .px_3()
            .py_1()
            .rounded_sm()
            .bg(rgb(current_theme().surface))
            .text_sm()
            .text_color(rgb(current_theme().text_main))
            .on_click(cancel_handler)
            .hover(|style| style.bg(rgb(current_theme().selected)))
            .child(SharedString::from("Cancel")),
    );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("commit-plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(current_theme().color_branch))
                .text_sm()
                .text_color(rgb(current_theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Commit")),
        );
    }

    card = card.child(button_row);

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
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
                            div()
                                .id("smart-consent-cancel")
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .bg(rgb(current_theme().surface))
                                .text_sm()
                                .text_color(rgb(current_theme().text_main))
                                .on_click(cancel)
                                .hover(|s| s.bg(rgb(current_theme().selected)))
                                .child(SharedString::from("Cancel")),
                        )
                        .child(
                            div()
                                .id("smart-consent-confirm")
                                .px_3()
                                .py_1()
                                .rounded_sm()
                                .bg(rgb(current_theme().color_success))
                                .text_sm()
                                .text_color(rgb(current_theme().bg_base))
                                .on_click(confirm)
                                .hover(|s| s.opacity(0.85))
                                .child(SharedString::from("Enable & continue")),
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
                        div()
                            .id("smart-model-cancel")
                            .px_3()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(current_theme().surface))
                            .text_sm()
                            .text_color(rgb(current_theme().text_main))
                            .on_click(cancel)
                            .hover(|s| s.bg(rgb(current_theme().selected)))
                            .child(SharedString::from("Cancel")),
                    ),
                )
        }
    };

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
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
