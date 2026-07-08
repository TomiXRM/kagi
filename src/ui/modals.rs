//! Modal state structs and the ActiveModal enum (ADR-0076 / ADR-0114).
//!
//! Renderer functions extracted to modal_renderers.rs.

use gpui::{Entity, SharedString};
use gpui_component::input::InputState;
use kagi_git::{
    ops::{AmendMode, BranchRenameValidation, MergeKind, OperationPlan},
    CommitId,
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

/// State for an in-progress operation-history Undo/Redo confirmation
/// (T-UNDOREDO-001, ADR-0081). Carries the previewed plan plus the
/// [`HistoryEntry`] being moved and whether the move is an undo or a redo.
#[derive(Clone)]
pub struct HistoryPlanModal {
    /// The computed undo/redo plan (current → target, blockers, warnings).
    pub plan: std::sync::Arc<OperationPlan>,
    /// The history entry this modal acts on.
    pub entry: kagi_git::HistoryEntry,
    /// `true` for an undo move, `false` for a redo move.
    pub is_undo: bool,
    /// Error message to show if execute or preflight failed.
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

/// State for a standalone stash **drop** confirmation (ADR-0087, Destructive).
#[derive(Clone)]
pub struct StashDropModal {
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

/// State for "Switch to latest `<branch>`" (ADR-0101): fetch + switch + ff-only.
#[derive(Clone)]
pub struct SwitchToLatestPlanModal {
    pub branch_name: String,
    pub remote_branch: String,
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
///
/// Two-stage confirm (T-REARCH-014, mirrors `AmendPlanModal::confirm_armed`):
/// the first click arms the red Discard button (label becomes the explicit
/// "Permanently discard N files"); only the second click executes. Discard is
/// the most destructive working-tree op (checkout -- + untracked deletion).
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
    /// Two-stage confirm gate: `false` = first click pending, `true` = armed.
    pub confirm_armed: bool,
}

// ──────────────────────────────────────────────────────────────
// Plan modal renderer (T013)
// ──────────────────────────────────────────────────────────────
// Modal renderer functions (render_plan_modal, render_pull_modal, etc.)
// have been extracted to modal_renderers.rs (ADR-0114).

/// Pending action to run once the user confirms discarding a dirty Editor
/// Workspace buffer (T-WS-EDITOR-002 §5, unsaved-changes guard).
///
/// Spec changes (user): switching the tree source or FILE is a view/tab
/// action, NOT destructive — a dirty buffer survives as its tab, so there
/// is no SwitchSource/SelectFile intent. The guard covers the genuinely
/// destructive paths only: replacing an edit with disk text (Reload),
/// closing a dirty tab (CloseTab), and dropping the whole workspace (Close
/// or repo-context changes).
#[derive(Clone, Debug)]
pub enum EditorPendingIntent {
    /// Discard the buffer and re-read the open file from disk (the
    /// external-change banner's Reload button).
    Reload,
    /// Close one editor tab (the tab's × button), discarding its edits.
    CloseTab(std::path::PathBuf),
    /// Close the Editor Workspace (← Graph, toolbar, Cmd-Shift-E).
    Close,
    /// Switch repository tabs after discarding the whole editor workspace.
    SwitchRepo(std::path::PathBuf),
    /// Close a repository tab after discarding the whole editor workspace.
    CloseRepoTab(std::path::PathBuf),
    /// Enter a remote read-only repo after discarding the local editor workspace.
    EnterRemoteView {
        host: kagi_domain::remote::RemoteHost,
        root: String,
        snap: std::sync::Arc<kagi_git::RepoSnapshot>,
    },
}

/// State for the Editor Workspace "unsaved changes" confirmation
/// (T-WS-EDITOR-002 §5). Not a Git write — no `OperationPlan` here, just a
/// discard-the-buffer-or-cancel gate before switching file/source or closing
/// the workspace while its buffer is dirty.
#[derive(Clone, Debug)]
pub struct EditorDirtyGuardModal {
    pub intent: EditorPendingIntent,
}

// ──────────────────────────────────────────────────────────────
// EditorFsPromptModal / EditorDeleteConfirmModal — Editor Workspace tree
// context-menu fs operations (T-WS-EDITOR-007)
// ──────────────────────────────────────────────────────────────

/// Which fs-prompt the single [`EditorFsPromptModal`] is showing. Rename
/// pre-fills `input` with the current name; New File / New Folder start
/// empty. Not a Git write (plain `std::fs` — same ADR-0120 §4 scoping as
/// Editor Workspace save) — no `OperationPlan` here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorFsPromptKind {
    Rename,
    NewFile,
    NewDir,
}

/// State for the Rename / New File / New Folder text-input prompt. One
/// modal variant for all three (ticket's call): they share the same
/// name-input + validate + `std::fs` op shape, only the target path and the
/// title/verb differ by `kind`.
#[derive(Clone)]
pub struct EditorFsPromptModal {
    pub kind: EditorFsPromptKind,
    /// `Rename`: the full repo-relative path being renamed. `NewFile` /
    /// `NewDir`: the repo-relative parent directory to create inside
    /// (empty `PathBuf` = repo root).
    pub base: std::path::PathBuf,
    /// Current text in the name input field (synced from `input_state`).
    pub input: String,
    /// Real text-input entity (gpui-component). Created lazily on first
    /// render (needs a Window); `None` in headless paths — the
    /// `KAGI_EDITOR_WS_NEWFILE` hook drives `input` directly instead.
    pub input_state: Option<Entity<InputState>>,
    /// Validation / fs-op error message, shown in place of a live plan (there
    /// is none — this isn't a Git write).
    pub error: Option<SharedString>,
}

/// State for the Delete (Trash) confirmation. Not a two-stage arm like
/// `DiscardModal` — a Trash move is recoverable (`~/.Trash`), unlike
/// `git checkout --`/untracked deletion, so a single explicit confirm click
/// is enough (ponytail: matches the risk level instead of copying the
/// heavier discard gate everywhere).
#[derive(Clone)]
pub struct EditorDeleteConfirmModal {
    /// Repo-relative path to trash.
    pub path: std::path::PathBuf,
    pub is_dir: bool,
    /// Recursive entry count under a directory target (see
    /// `editor_fs_ops::count_dir_entries_capped`); `None` for a file target.
    pub file_count: Option<usize>,
    /// `true` when `file_count` stopped short of the real total (capped —
    /// the modal shows "N+ files").
    pub truncated: bool,
    /// True when an open editor tab at/under `path` has unsaved edits; the
    /// Trash move is recoverable, but the in-memory delta is not.
    pub has_dirty_buffers: bool,
    pub error: Option<SharedString>,
}

pub enum ActiveModal {
    Checkout(CheckoutPlanModal),
    Pull(PullPlanModal),
    Undo(UndoPlanModal),
    Amend(AmendPlanModal),
    Pop(PopPlanModal),
    StashDrop(StashDropModal),
    Push(PushPlanModal),
    BranchPlan(BranchPlanModal),
    SetUpstream(SetUpstreamModal),
    RenameBranch(RenameBranchModal),
    Merge(MergePlanModal),
    TrackingCheckout(TrackingCheckoutPlanModal),
    SwitchToLatest(SwitchToLatestPlanModal),
    CreateBranch(CreateBranchModal),
    CreateWorktree(CreateWorktreeModal),
    StashPush(StashPushModal),
    StashApply(StashApplyModal),
    CherryPick(CherryPickModal),
    Revert(RevertModal),
    History(HistoryPlanModal),
    DeleteBranch(DeleteBranchModal),
    Discard(DiscardModal),
    ConflictContinue(ConflictContinuePlanModal),
    EditorDirtyGuard(EditorDirtyGuardModal),
    EditorFsPrompt(EditorFsPromptModal),
    EditorDeleteConfirm(EditorDeleteConfirmModal),
}
