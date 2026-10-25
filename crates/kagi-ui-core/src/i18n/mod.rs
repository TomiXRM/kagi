//! W22-I18N / ADR-0048: dependency-free UI localization (English / Japanese).
//! Covers modal explanatory / confirmation / recovery prose, toasts, Busy-footer
//! texts, guard messages, WIP notes, and empty states.
//! Typed plan notes keep their English renderer in `kagi-domain`; their JA
//! renderer lives in `plan`. Advice Msg keys share canonical domain EN templates
//! and family-local JA templates without changing CLI/oplog/klog text.
//! # Design (same shape as [`super::theme`])
//! * [`Lang`] is `En` / `Ja`; the active language is an [`AtomicUsize`] index
//!   (`0 = En`, `1 = Ja`), exactly like `theme::ACTIVE`.
//! * [`lang()`] reads it (called from every render path that shows prose);
//!   [`set_lang()`] updates **and persists** it to `settings.json` key `"lang"`.
//! * [`Msg`] is an enum of message keys; [`Msg::t`] matches on `(lang(), self)`
//!   and returns a `&'static str`.  Because the match is exhaustive, a missing
//!   translation is a **compile error** — no fluent / gettext crate is added
//!   (dependency-purity rule).
//! * Parameterized strings get plain helper `fn`s in this module (e.g.
//!   [`wip_row_note`]) so `format!` lives here, not at the call sites.
//! # Domain words stay English
//! Per ADR-0048, domain words (Pull / Push / Branch / Stash / Pop / Undo /
//! Terminal / Commit / amend / checkout / cherry-pick / revert / discard /
//! worktree / tag …), single-word action buttons, column headers, SHAs and
//! branch names are **not** translated; they appear verbatim inside both the
//! `En` and `Ja` arms below.
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::settings::{read_setting, write_setting};

pub fn settings_zoom_action(increase: bool, value: &str) -> String {
    match (lang(), increase) {
        (Lang::En, false) => format!("Decrease UI zoom ({value})"),
        (Lang::En, true) => format!("Increase UI zoom ({value})"),
        (Lang::Ja, false) => format!("UI ズームを下げる ({value})"),
        (Lang::Ja, true) => format!("UI ズームを上げる ({value})"),
    }
}

pub fn settings_zoom_value(value: &str) -> String {
    match lang() {
        Lang::En => format!("UI zoom {value}"),
        Lang::Ja => format!("UI ズーム {value}"),
    }
}

pub fn settings_compact_threshold_action(increase: bool, value: u32) -> String {
    match (lang(), increase) {
        (Lang::En, false) => format!("Decrease compact layout threshold ({value}rem)"),
        (Lang::En, true) => format!("Increase compact layout threshold ({value}rem)"),
        (Lang::Ja, false) => format!("コンパクト表示への切替幅を下げる ({value}rem)"),
        (Lang::Ja, true) => format!("コンパクト表示への切替幅を上げる ({value}rem)"),
    }
}

pub fn settings_compact_threshold_value(value: u32, logical_px: f32) -> String {
    match lang() {
        Lang::En => format!("{value}rem ≈ {logical_px:.0} logical px at current UI zoom"),
        Lang::Ja => format!("{value}rem ≈ 現在の UI ズームで {logical_px:.0} logical px"),
    }
}

// Lang + active-language atomic
pub mod activity;
pub mod analyze;
/// UI language.  `En` is index 0 (the default), `Ja` is index 1.
pub mod busy;
mod conflict;
pub use busy::{busy_label, slow_read_advice, slow_read_label, slow_read_skip, slow_write_advice};
pub mod op;
pub mod oplog_panel;
pub mod plan;
pub mod pr_threads;
pub mod pr_viewed;
pub mod queue;
pub mod status_chips;
pub mod time;
pub use op::{
    auto_stash_identity_unverified, auto_stash_missing, auto_stash_restore_conflicted,
    auto_stash_restore_failed, op_failed, op_plan_failed, op_refused, oplog_write_failed,
    plan_not_shown_retry, pull_failed_stash_restored, rebase_repository_settings_may_block_start,
    recorded_outcome_notice, terminal_nonconcurrent_blocked, terminal_ports_exhausted, Op,
};

/// Accessibility action for a document tab's localized display name.
pub fn tab_close_name(name: &str) -> String {
    match lang() {
        Lang::En => format!("Close {name}"),
        Lang::Ja => format!("{name} を閉じる"),
    }
}
pub use plan::{plan_note_text, plan_recovery_text, plan_title_text};
pub use queue::{queue_text, QueueText};

pub fn terminal_exited_title(code: u32) -> String {
    match lang() {
        Lang::En => format!("{} (code {code})", Msg::TerminalExitedTitle.t_for(Lang::En)),
        Lang::Ja => format!(
            "{}（終了コード {code}）",
            Msg::TerminalExitedTitle.t_for(Lang::Ja)
        ),
    }
}

pub fn terminal_failed_hint(reason: &str) -> String {
    format!("{}\n{reason}", Msg::TerminalRestartHint.t())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
    /// Every selectable UI language, in menu order. Source of truth for the
    /// Language submenu and the command palette (issue #373).
    pub const ALL: &'static [Lang] = &[Lang::En, Lang::Ja];

    /// The name shown in the Language submenu / picker (an endonym — not
    /// translated per active language).
    pub fn display_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ja => "日本語",
        }
    }

    /// Stable lowercase slug used in `settings.json` and `KAGI_LANG`.
    pub fn slug(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
        }
    }

    /// Parse a slug (`"en"` / `"ja"`, case-insensitive) into a [`Lang`].
    pub fn from_slug(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            _ => None,
        }
    }

    fn from_index(i: usize) -> Lang {
        if i == 1 {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Ja => 1,
        }
    }
}

/// Active language index (`0 = En`, `1 = Ja`).  Defaults to English.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The currently-active [`Lang`].  Cheap — called from every prose render path.
#[inline]
pub fn lang() -> Lang {
    Lang::from_index(ACTIVE.load(Ordering::Relaxed))
}

/// Set the active language and persist it to `settings.json` (key `"lang"`).
pub fn set_lang(l: Lang) {
    ACTIVE.store(l.index(), Ordering::Relaxed);
    write_setting("lang", Some(l.slug()));
}

/// Set the active language **without** persisting (test helper — keeps the
/// unit tests off the real `settings.json`).
#[cfg(test)]
pub fn set_lang_no_persist(l: Lang) {
    ACTIVE.store(l.index(), Ordering::Relaxed);
}

// ──────────────────────────────────────────────────────────────────────────
// Startup resolution
// ──────────────────────────────────────────────────────────────────────────

/// Resolve the startup language **without** mutating global state.
///
/// Priority (ADR-0048):
/// 1. `KAGI_LANG=en|ja` env override (headless-test determinism),
/// 2. persisted `settings.json` `"lang"`,
/// 3. `LANG` / `LC_ALL` starting with `"ja"` → [`Lang::Ja`],
/// 4. otherwise [`Lang::En`].
pub fn resolve_lang() -> Lang {
    if let Ok(v) = std::env::var("KAGI_LANG") {
        if let Some(l) = Lang::from_slug(&v) {
            return l;
        }
    }
    if let Some(l) = read_setting("lang").and_then(|s| Lang::from_slug(&s)) {
        return l;
    }
    let locale = std::env::var("LC_ALL")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default();
    if locale.to_ascii_lowercase().starts_with("ja") {
        Lang::Ja
    } else {
        Lang::En
    }
}

/// Initialise the active language at startup (called once from `main`).
/// Logs `[kagi] lang: <slug>`.
pub fn init_lang() {
    let l = resolve_lang();
    ACTIVE.store(l.index(), Ordering::Relaxed);
    klog!("lang: {}", l.slug());
}

// ──────────────────────────────────────────────────────────────────────────
// Message keys
// ──────────────────────────────────────────────────────────────────────────

/// Every translatable UI-layer string key (wave 1).  Domain words stay English
/// inside both arms; only the surrounding explanatory prose is localized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    AboutTitle,
    InspectorOpenLink,
    InspectorCopyLink,
    A11yCommandPaletteQuery,
    A11yCommitSummary,
    A11yCommitDescription,
    A11yConflictResult,
    A11yIssueTitle,
    A11yIssueBody,
    A11yCommentBody,
    A11yHomeFilter,
    A11yListTitleFilter,
    A11ySidebarFilter,
    A11ySidebarClearSearch,
    SidebarGroupFilterExpanded,
    A11yPrReviewersFilter,
    A11yPrAssigneesFilter,
    A11yPrLabelsFilter,
    A11yStashMessage,
    A11yAnalyzeIgnorePatterns,
    A11yEditorFileContents,
    AboutDescription,
    MenuHideSidebar,
    MenuShowSidebar,
    MenuEnterFullScreen,
    MenuExitFullScreen,
    MenuPreviousTab,
    MenuNextTab,
    MenuBringAllToFront,
    NoOtherTabs,
    MenuHideApp,
    MenuHideOthers,
    MenuShowAll,
    MenuServices,
    ShortcutsRemapHelp,
    MenuUndo,
    MenuRedo,
    MenuCut,
    MenuCopy,
    MenuPaste,
    MenuSelectAll,
    MenuSave,
    MenuOpenRecent,
    MenuClearRecent,
    MenuFind,
    MenuFindOpen,
    MenuWorkspaceGraph,
    MenuWorkspacePrs,
    MenuWorkspaceEditor,
    NothingToSave,
    AppNoticeDismiss,
    ToastDismiss,
    BusyHideProgress,
    ToastOpenRepository,
    ToastMore,
    ToastMoreOne,
    ToastCollapse,
    /// Title row of the shared app-notice card (#792). One neutral title for
    /// every notice: `AppNotice` carries no outcome kind, so a card that named
    /// one ("refused", "aborted") would be guessing.
    AppNoticeTitle,
    /// The card's closing line: where the operation's detail can be read and
    /// copied. Says where to look, never that the entry was durably saved —
    /// an append failure is one of the notices this card carries.
    AppNoticeDetailsInOpLog,
    AppReconcileRequired,
    AppReconcileAcknowledge,
    AppReconcileInspect,
    AppReconcileConfirm,
    AppReconcileUnobservable,
    AppReconcileArmRelease,
    AppReconcileReleaseUnobservable,
    AppReconcileReleaseArmed,
    // ── Generic guards / footers ────────────────────────────────────
    /// "another operation is in progress" (was "別の操作が実行中です").
    OpInProgress,
    CheckoutStashCannotQueue,
    SlowWriteNetwork,
    SlowWriteRebase,
    SlowWriteCheckout,
    SlowWriteRefs,
    SlowWriteMerge,
    SlowWriteCommit,
    SlowWriteStash,
    SlowWriteWorktree,
    SlowWriteConflict,
    SlowWriteLocal,
    SlowWriteGeneric,
    RemoteOpAwaitingCompletion,
    TransportRetryHeld,
    NoRepoOpen,
    NoCommitSelected,
    DiffNotOpen,
    /// ADR-0124: diff-header selected mode segments.
    DiffViewSplit,
    DiffViewUnified,
    /// Line-ending-only replacement captions; existing sigils/columns identify sides.
    DiffLineEndingLf,
    DiffLineEndingCrLf,
    DiffLineEndingNone,
    DiffBinaryNoText,
    DiffImageAdded,
    DiffImageModified,
    DiffImageDeleted,
    DiffImageBefore,
    DiffImageAfter,
    DiffImagePreviewUnavailable,
    DiffPdfPreview,
    DiffPdfPrevious,
    DiffPdfCurrent,
    DiffPdfNoPrevious,
    DiffPdfNoCurrent,
    DiffPdfReadFailed,
    DiffPdfScrollIndependent,
    DiffPdfScrollSynchronized,
    ImageFileSize,
    ImageDimensions,
    /// `kagi_ui_core::commit_header::render_commit_header`'s placeholder
    /// before any commit is selected — shared by the Editor Workspace's
    /// History pane and File History's detail pane.
    CommitHeaderSelectPrompt,

    // ── Agent provenance (issue #337) ───────────────────────────────
    /// Neutral qualifier appended to an agent badge when a `Reviewed-by:`
    /// trailer is present. Non-judgmental.
    AgentReviewed,
    /// Tooltip for an agent-provenance badge (no human-review qualifier).
    AgentCreatedTooltip,
    /// Tooltip for an agent-provenance badge that also carries a review.
    AgentReviewedTooltip,

    // ── Placeholder / unimplemented menu reasons ────────────────────
    MultiWindowUnsupported,
    ResetUnimplemented,
    FinishDialogFirst,

    // ── Busy footers (op in flight) ─────────────────────────────────
    BusyCheckout,
    BusySwitchToLatest,
    BusyPull,
    AutoStashRestored,
    PullAutoStashConfirm,
    PullReviewConflicts,
    PullReviewChanges,
    BusyPush,
    BusyStash,
    BusyStashPop,
    /// issue #280: pop applied with conflicts; the stash entry was kept.
    StashPopConflictedKept,
    BusyStashDrop,
    BusyCherryPick,
    BusyRevert,
    BusyAmend,
    BusyDeleteBranch,
    BusyDeleteBranchPlan,
    BusyDiscard,
    BusyCommit,
    BusyCreateWorktree,
    /// Footer label while a worktree removal runs off the main thread (issue
    /// #404): a `pre_remove` command can take minutes, so removal is async.
    BusyRemoveWorktree,
    WorktreeMeasuring,
    WorktreeNotMeasured,
    WorktreeInspectionRefresh,
    WorktreeInspectionStale,
    WorktreeSafeMerged,
    WorktreeSafePushed,
    WorktreeMainExcluded,
    WorktreeDirty,
    WorktreeLocked,
    WorktreeUnpublished,
    WorktreeUnborn,
    WorktreeUpstreamUnavailable,
    WorktreeObservationFailed,
    WorktreeSafeShort,
    WorktreeKeepShort,
    WorktreeUnknownShort,
    WorktreeDirtyShort,
    WorktreeLockedShort,
    WorktreeStaleShort,
    WorktreeErrorShort,
    WorktreeDetachedShort,
    /// Worktree context menu: unlock action label.
    MenuUnlockWorktree,
    /// Worktree context menu: disabled reason when the worktree has no lock.
    MenuWorktreeNotLocked,
    /// Worktree context menu: lock action label (issue #340).
    MenuLockWorktree,
    /// Worktree context menu: disabled reason when the worktree is already locked.
    MenuWorktreeAlreadyLocked,
    /// Worktree context menu: remove keeping the branch (issue #340).
    MenuRemoveWorktreeKeepBranch,
    /// Worktree context menu: remove and also delete the branch (issue #340).
    MenuRemoveWorktreeAndBranch,
    /// Worktree context menu: prune stale worktrees (issue #340).
    MenuPruneWorktrees,
    /// Worktree context menu: repair worktree links (issue #340).
    MenuRepairWorktrees,
    /// WIP-row worktree menu: open that worktree in a new tab (issue #473 —
    /// the click itself now shows the worktree's changes in place).
    MenuOpenWorktreeInNewTab,
    /// Branch menu: open the worktree this branch is checked out in (#473).
    MenuOpenWorktreeDir,
    /// Graph badge label for a detached worktree (#595).
    GraphDetachedWorktree,
    /// Connector in a destructive modal's title when the op runs in a linked
    /// worktree rather than the open tab: "Discard all changes (3) — in
    /// worktree wt-a" (#476 slice 3, `worktree_wip::worktree_modal_title`).
    InWorktree,
    /// Default lock reason kagi records for a manual lock (issue #340).
    WorktreeLockDefaultReason,
    WorktreeLockReason,
    WorktreeLockReview,
    /// Confirm or manually resolve a terminal-owned lock before restarting.
    TerminalAutoLockPendingRelease,
    /// An auto lock/unlock offer can no longer be attributed to its owner.
    TerminalAutoLockStaleOwner,
    TerminalNotStartedTitle,
    TerminalExitedTitle,
    TerminalFailedTitle,
    TerminalNotStartedHint,
    TerminalRestartHint,
    TerminalRestart,
    TerminalPasteTitle,
    TerminalPasteBody,
    TerminalPasteControlBody,
    TerminalPasteConfirm,
    TerminalFocusDescription,
    UnbornEmptyTitle,
    UnbornEmptyHint,
    BusyMerge,
    MergeDestinationChanged,

    // ── Operation no-op toasts ──────────────────────────────────────
    // (Per-op "started" toasts were removed: the unified busy snackbar —
    // driven by `busy_op` with a spinning sync icon — now signals progress.)
    AlreadyUpToDatePull,
    AlreadyUpToDatePush,
    PullUpstreamChangedDuringFetch,
    PullFilesChangedDuringPlanning,
    PullAutoStashIdentityChanged,
    PullAutoStashPlanStale,
    PullAutoStashRestoreChanged,

    // ── Toolbar guard reasons (domain words kept English) ───────────
    PullBusy,
    PullDetached,
    PullUnborn,
    PullNoUpstream,
    PullNothing,
    PushBusy,
    PushDetached,
    PushUnborn,
    PushNoRemote,
    PushNothing,
    StashClean,
    StashBusy,
    PopEmpty,
    PopBusy,
    // Legacy undo-commit disabled-reason strings (UndoDetached / UndoUnborn /
    // UndoAhead0) were removed when the toolbar Undo button was generalised to
    // operation-history undo (T-UNDOREDO-001); the headless undo-commit path in
    // main.rs no longer surfaces a disabled reason.

    // ── Operation Undo / Redo (T-UNDOREDO-001, ADR-0081) ────────────
    /// Toolbar "Undo" button label (domain word — English in both langs).
    Undo,
    /// Toolbar "Redo" button label (domain word — English in both langs).
    Redo,
    /// Footer / tooltip shown when there is nothing to undo.
    NothingToUndo,
    /// Footer / tooltip shown when there is nothing to redo.
    NothingToRedo,

    // ── Checkout / compare prose & recovery ─────────────────────────
    CheckoutSelectFirst,
    AlreadyHead,
    NoLocalChanges,
    DirtyStashFirst,
    AmendNeedMessageOrStaged,
    AmendStatusLoading,

    // ── Context-menu disabled reasons ───────────────────────────────
    CmDetachedHead,
    CmSameAsHead,
    CmMergeUnsupported,
    CmAlreadyInBranch,
    CmNotInBranch,
    CmAlreadyHead,
    CmIdentical,
    CmNoLocalChanges,
    CmResetUnneeded,
    CmNoCurrentBranch,
    CmNoSelectedBranch,
    CmResetUnimplemented,
    BcmBusy,
    BcmNotImplementedYet,
    BcmCurrentBranch,
    BcmLocalOnly,
    BcmCurrentBranchOnly,
    BcmOnlyFromCurrentBranch,
    BcmNoUpstream,
    BcmDetachedHead,
    BcmDeleteBranch,
    BcmDeleteUnmerged,
    BcmCheckedOutElsewhere,
    BcmConflictMode,
    BcmNothingToPush,
    BcmIntegrateMenu,
    BcmAdvancedMenu,
    BcmWorktreeMenu,

    // ── Empty states ────────────────────────────────────────────────
    NoLocalBranches,
    BranchPickerCheckout,
    BranchPickerDelete,
    BranchPickerRename,
    A11yIssueOpenRow,
    A11yRecentOpenRow,
    NoOperationsYet,
    /// Operation Log row: tooltip on the per-row copy button (issue #468).
    OpLogCopyEntry,
    /// Operation Log row: shown above a huge expanded detail block, whose text
    /// selection is switched off to keep the draw cheap (issue #548).
    OpLogDetailSelectionOff,
    // #1025 — Operation Log plan-recovery section.
    OpLogRecovery,
    OpLogRecoveryNotRecorded,
    OpLogRecoveryCommand,

    // ── Command palette (issue #352) ─────────────────────────────────
    /// Placeholder text in the palette's search box.
    CommandPalettePlaceholder,
    /// Shown when the query matches no command.
    CommandPaletteNoResults,
    /// Empty-query palette's command section.
    CommandPaletteActions,

    // ── Misc footers ────────────────────────────────────────────────
    Refreshed,
    /// Toast after a manual working-tree snapshot (ADR-0154 / #335).
    SnapshotCreated,
    /// Toast prefix when a manual snapshot fails (ADR-0154 / #335).
    SnapshotFailed,
    /// Fetch authentication failure; `{}` is the sanitized remote identity.
    FetchAuthenticationRequired,
    OpenedInFinder,
    /// Label on the "load more commits" row at the bottom of the commit list.
    LoadMoreCommits,
    /// Computed plan file preview; `{}` is the full count.
    ModalFilesToChange,
    /// Bounded preview remainder; `{}` is the omitted count.
    ModalFilesMore,
    /// Preview producer reached its cap; the true omitted count is unknown.
    ModalFilesMoreUnknown,

    // ── W30-CONFLICT-UI: Conflict Mode (banner / list / choose / preview) ──
    // Operation headings (domain words rebase/merge/cherry-pick/revert kept
    // English per ADR-0048; only the surrounding prose is localized).
    // Banner buttons + progress.
    ConflictContinue,
    ConflictAbort,
    ConflictSkip,
    /// Full armed Skip label, shared by the strip and sidebar tooltip.
    ConflictConfirmSkip,
    /// Short armed sidebar button label; the full action stays in its tooltip.
    ConflictConfirmSkipButton,
    /// Repository menu titles (distinct from sentence-case buttons).
    MenuConfirmSkip,
    MenuAbortOperation,
    MenuAbortMerge,
    MenuAbortRebase,
    MenuAbortCherryPick,
    MenuAbortRevert,
    MenuAbortStashApply,
    /// Conflict dashboard: what the armed Skip is about to discard.
    ConflictConfirmSkipHint,
    ConflictResolved,
    // File list.
    ConflictUnresolved,
    ConflictResolvedShort,
    ConflictNeedsReview,
    ConflictKindContent,
    ConflictKindRenameDelete,
    ConflictKindModifyDelete,
    ConflictKindAddAdd,
    ConflictKindSubmodule,
    ConflictKindSymlink,
    ConflictKindBinary,
    ConflictKindDirFile,
    // Detail pane / choose buttons (role names appended at the call site).
    ConflictSelectFile,
    ConflictKeepCurrent,
    ConflictTakeIncoming,
    ConflictKeepBoth,
    // #320: directory/file conflict — keep-directory vs keep-file.
    ConflictKeepDirectory,
    ConflictKeepFile,
    ConflictConfirmDeletion,
    ConflictUseDeletedSide,
    ConflictUseModifiedSide,
    ConflictResolutionPlan,
    ConflictResolutionConfirm,
    ConflictResolutionConsequence,
    ConflictRemoveIndex,
    ConflictRemoveWorktree,
    ConflictKeepPaths,
    ConflictRestorePaths,
    ConflictDirFileHint,
    ConflictResultPreview,
    ConflictPreviewHint,
    ConflictBinaryNoPreview,
    // ── #321: binary / symlink / submodule side viewer ──
    ConflictSymlinkTarget,
    ConflictOpenBothExternal,
    ConflictImageTooLarge,
    ConflictSubmoduleCommit,
    // ── W32-CONFLICT-EDITOR: hunk-level Conflict Editor ──────
    ConflictInProgress,
    ConflictSaved,
    ConflictSavedStaged,
    ConflictStagedContents,
    ConflictReviewContinue,
    ConflictUnsaved,
    ConflictSaveRefused,
    ConflictWorktreeChanged,
    ConflictWorktreeChangedHelp,
    ConflictReload,
    ConflictSaveAgain,
    ConflictNextFile,
    ConflictNotCommitted,
    ConflictReviewNeeded,
    ConflictShowSidebar,
    ConflictHideSidebar,
    ConflictReview,
    ConflictBackToHunk,
    ConflictTools,
    ConflictDraft,
    ConflictDraftReady,
    ConflictStaged,
    ConflictHunkResult,
    ConflictEmptyResult,
    ConflictManualResult,
    ConflictUseThisSide,
    ConflictUseThisSideAccessible,
    ConflictBothCurrentFirst,
    ConflictBothIncomingFirst,
    ConflictUsed,
    ConflictNotUsed,
    ConflictPartlyUsed,
    ConflictChange,
    ConflictChangeAccessible,
    ConflictResetFocused,
    ConflictResetFocusedAccessible,
    ConflictEditResult,
    ConflictRemainingFiles,
    ConflictToCommit,
    EditorCurrentSide,
    EditorIncomingSide,
    EditorConflictNofM,
    EditorPrevHunk,
    EditorNextHunk,
    EditorOpenExternal,
    EditorReset,
    EditorSave,
    EditorResultOutput,
    EditorAllResolved,
    EditorUnresolvedHunks,
    EditorSavedResolved,
    EditorNoTextMerge,
    // ── T-CONFLICT-UI/UX: 3-pane editor controls ──
    EditorResetAllConfirm,
    // ── T-CONFLICT-UX-010/012: per-hunk accept controls ──
    EditorHunkLabel,
    EditorCurrentFirst,
    EditorIncomingFirst,
    // ── W33-CONFLICT-DASHBOARD: Right-panel dashboard + escape hatch ──
    ConflictDashHeader,
    ConflictRebaseApplying,
    ConflictRebaseCommits,
    ConflictRoleCurrent,
    ConflictRoleIncoming,
    ConflictGitTermHint,
    ConflictConflictedCount,
    ConflictResolvedCount,
    ConflictSectionConflicted,
    ConflictSectionResolved,
    ConflictConfirmAbort,
    /// #704: abort success toast, and the strip prose after the operation's
    /// own name (`merge · in progress`, `rebase 2/5 · 進行中`).
    ConflictAborted,
    OperationInProgress,
    ConflictBackToConflicts,
    ConflictExternalTool,
    ConflictExternalToolUnset,
    ConflictOpenTerminal,
    ConflictCopyPath,
    ConflictCopyGitCommand,
    ConflictBlockerUnresolved,
    ConflictBlockerMarker,
    ConflictBlockerBinary,
    ConflictBlockerDeletion,
    ConflictBlockerIndex,
    ConflictBlockerMessage,
    ConflictBlockerChecklist,
    ConflictContinueReady,
    ConflictMore,
    ConflictNextConflict,
    ConflictNoConflictedFiles,
    ConflictNoResolvedFiles,
    // ── Branch-name / worktree-path validation (W29-I18N-WAVE2) ──────
    // The keyed git-layer validation reasons (src/git/ops.rs). Domain words
    // (branch / worktree / git ref / HEAD) and the user-entered name/path stay
    // verbatim; only the surrounding prose is localized. Parameterized variants
    // (carrying a name/path) use the `*_fmt` helpers below, not these arms.
    /// create-branch: name is empty.
    BranchNameEmpty,
    /// rename-branch: name is required (blank).
    BranchNameRequired,
    /// rename-branch: leading/trailing whitespace.
    BranchNameWhitespace,
    /// rename-branch: new name equals the old name.
    BranchNameSame,
    /// worktree path is empty.
    WorktreePathEmpty,

    // ── Misc UI prose sweep (W29-I18N-WAVE2, task 3) ─────────────────
    /// Inspector counts row when nothing changed in the commit.
    NoFileChanges,
    /// Inspector files list when the diff could not be computed.
    DiffUnavailable,
    /// Inspector co-author section caption.
    CoAuthoredBy,
    /// Inspector trailers section caption (issue #336).
    Trailers,
    /// Commit panel: subject-line input placeholder.
    CommitTitle,
    /// Commit panel: body input placeholder.
    CommitBody,
    /// Compact WIP row's authoring affordance and pinned composer.
    CompactCommitAffordance,
    CompactCommitComposer,
    CompactCommitClose,
    /// Commit draft persistence failure, followed by the I/O detail.
    CommitDraftSaveFailed,
    CommitDraftSaveRetry,
    /// Commit panel: tooltip on the sparkles (AI message) icon.
    GenerateMessage,
    /// Smart Commit capabilities are still being discovered in the background.
    SmartDetecting,
    /// Commit panel: tooltip on the person+ (co-author) icon.
    AddCoAuthor,
    /// Commit panel: tooltip on the amend icon.
    AmendLastCommit,
    /// Commit panel: co-author picker with nothing to offer.
    NoRecentAuthors,
    /// Conflict editor: pane-header pill that takes the whole file's side.
    EditorUseAll,
    EditorSelectionNone,
    EditorSelectionPartial,
    EditorSelectionAll,
    /// Inspector file context menu: open the file in the Editor Workspace.
    MenuOpenInEditor,
    /// Inspector file context menu: show this file's history.
    MenuShowFileHistory,
    A11yInspectorFiles,
    A11yFileHistory,
    /// Context menus: open the file in the user's external editor.
    OpenInExternalEditor,
    DiffStageHunk,
    DiffUnstageHunk,
    /// PR menu: the head branch is not fetched locally.
    PrBranchNotFetched,
    /// PR context menu.
    PrOpenOnGitHub,
    PrCopyUrl,
    PrPeek,
    PrJumpToBranch,
    /// PR draft label.
    PrDraft,
    /// PR pane (takeover).
    PrPaneTitle,
    PrPaneEmpty,
    PrReviewApproved,
    PrReviewChanges,
    PrReviewRequired,
    /// PR mode.
    PrModeExit,
    PrModeSelectHint,
    PrModeNoFile,
    PrModeFiles,
    PrModeShowDescription,
    PrModeDescription,
    PrModeCommits,
    /// Focus Queue buckets.
    PrQueueNeedsYou,
    PrQueuePending,
    PrQueueInProgress,
    PrQueueReady,
    PrQueueWaiting,
    PrQueueDormant,
    /// Navigator sections (ADR-0200) — filters over the open PRs.
    PrSectionInbox,
    PrSectionMine,
    PrSectionReview,
    PrSectionAssigned,
    /// Swimlane pane (ADR-0200).
    PrLaneTitle,
    PrLaneLoaded,
    PrLaneCommit,
    PrLaneContext,
    PrSwimlane,
    PrShowNavigator,
    PrHideNavigator,
    PrShowGraph,
    PrHideGraph,
    NoPrOpen,
    /// Detail rail facts (ADR-0200).
    PrRailReviewers,
    PrRailAssignees,
    PrRailLabels,
    PrRailWorktree,
    PrRailWorktreeClean,
    PrRailNone,
    /// Home list: header, chips, sort and column names (ADR-0200).
    PrHomeOpen,
    PrHomeDraft,
    PrHomeSortUpdated,
    PrHomeSortCreated,
    PrHomeSortNumber,
    PrColNo,
    PrColTitle,
    PrColState,
    PrColAuthor,
    PrColChecks,
    PrColFiles,
    PrColAge,
    /// Focus Queue reasons.
    PrWhyChangesRequested,
    PrWhyConflicting,
    PrWhyCiRunning,
    PrWhyReadyToMerge,
    PrWhyReviewRequested,
    PrWhyAwaitingReview,
    PrWhyPending,
    /// Right rail + tabs.
    PrModeChecks,
    PrModeReview,
    PrModeOverview,
    PrModeNoReview,
    PrModeMerge,
    PrModeMergeDone,
    PrChecksPassed,
    PrChecksFailed,
    PrChecksRunning,
    PrChecksOpenInBrowser,
    PrReviewApprove,
    PrReviewSubmit,
    PrReviewRequestChanges,
    PrEditApply,
    PrFieldsApply,
    PrFieldsFilterPlaceholder,
    PrCommentPlaceholder,
    PrCommentPost,
    IssueCompose,
    IssueComposeEmpty,
    IssueTitleOptional,
    IssueCreate,
    HomeTabTitle,
    HomeTitle,
    HomeRecent,
    HomeNoRecent,
    HomeRemoveRecent,
    HomeOpenFolder,
    HomeConnectRemote,
    HomeGithubMine,
    /// `{}` = why the organization could not be read.
    HomeGithubOwnerFailed,
    HomeGithubFilter,
    HomeGithubRefresh,
    HomeGithubLoading,
    HomeGithubOrgsLoading,
    HomeGithubUpdating,
    HomeGithubOrgs,
    /// `{}` = why the organizations could not be listed.
    HomeGithubOrgsFailed,
    /// `{}` = why the read failed.
    HomeGithubRefreshFailed,
    HomeGithubFailedHint,
    /// `{owner}` = section heading; `{host}` = host this list was read from.
    HomeGithubSectionEmpty,
    /// `{host}` = host the user's successfully empty list was read from.
    HomeGithubMineEmpty,
    HomeGithubNoMatch,
    /// `{}` = how many were read.
    HomeGithubTruncated,
    HomeGithubOpen,
    HomeGithubClone,
    HomeGithubCloning,
    /// Home's switch (#928): the repository list.
    HomePaneRepos,
    /// Home's switch: the user's open pull requests.
    HomePanePrs,
    /// Home's switch: the issues assigned to the user.
    HomePaneIssues,
    HomeWorkMine,
    HomeWorkReview,
    HomeWorkNoPrs,
    HomeWorkNoReview,
    HomeWorkNoIssues,
    HomeWorkNoMatch,
    HomeWorkTruncated,
    HomeWorkOpenOnGithub,
    HomeWorkLoading,
    /// `{}` = why the list could not be read.
    HomeWorkFailed,
    /// `{}` = the PR number, then why it could not be opened.
    HomeWorkOpenFailed,
    /// `{}` = the issue number, its repository, the repository the clone's
    /// Issues mode addresses.
    HomeWorkOtherRepo,
    HomeGithubPrivate,
    HomeGithubFork,
    /// `{}` = the repository's name.
    CloneTitle,
    CloneLocation,
    CloneNoLocation,
    /// `{}` = the folder that will be created.
    CloneWillCreate,
    CloneChooseFolder,
    CloneChangeFolder,
    CloneCopyLocation,
    CloneConfirm,
    CloneBusy,
    /// `{}` = elapsed time, `m:ss`.
    CloneRunning,
    /// `{}` = the host it downloads from.
    CloneRunningHint,
    CloneHide,
    /// `{}` = the full source of the single running clone.
    CloneHiddenRunning,
    CloneShowProgress,
    IssuePostedAs,
    IssueNewFieldsTitle,
    IssueReply,
    ComposerWrite,
    ComposerPreview,
    IssueFocusEditor,
    IssueExitFocus,
    ComposerDraftSaved,
    ComposerDraftSaving,
    IssueRepoUnavailable,
    PrSuggestion,
    PrApplySuggestion,
    PrHunkCopy,
    PrHunkCopied,
    /// PR pane: the Conflicts tab label.
    PrModeConflicts,
    /// PR pane: the Conflicts tab, when nothing conflicts after all.
    PrConflictsNone,
    /// PR pane: the Conflicts tab's one-line explanation.
    PrConflictsHint,
    /// PR pane: a file deleted on one side and changed on the other.
    PrConflictDeleteModify,
    /// PR pane: both sides added a file at the same path.
    PrConflictBothAdded,
    /// PR pane: a binary file changed on both sides — nothing to show as text.
    PrConflictBinary,
    /// PR pane: the conflict is too large to render.
    PrConflictTooLarge,
    /// PR pane: the word in a conflict hunk header ("conflict 2/7").
    PrConflictHunk,
    PrModeNoDescription,
    /// PR merge-status card (#347): heading + one line per merge state's action.
    PrMergeStatusHeading,
    PrMergeActionConflict,
    PrMergeActionUpdateBranch,
    PrMergeActionBlocked,
    PrMergeActionUnstable,
    PrMergeActionReady,
    PrMergeActionDraft,
    PrMergeActionWait,
    /// PR merge-status card: the "still missing" list labels.
    PrMergeMissingApprovals,
    PrMergeMissingCodeowner,
    PrMergeUnresolvedThreads,
    /// PR merge-status card: merge-queue position line.
    PrMergeQueueLabel,
    /// PR mode: dashboard "back to the home screen" button.
    PrAllPrs,
    /// PR mode: manual PR-list fetch button.
    PrRefresh,
    /// PR mode: the PR fetch failed, shown instead of the empty inbox.
    PrFetchFailed,
    /// #506: the shown list is the last successful fetch, not this one.
    PrFetchStale,
    /// #506: the fetch failed because `gh` is not authenticated.
    PrFetchAuth,
    /// #506: the fetch never reached GitHub (DNS/TLS/timeout/refused).
    PrFetchNetwork,
    /// #506: `gh` answered with something that did not parse.
    PrFetchInvalid,
    /// #506: the fetch failed for a reason kagi could not classify.
    PrFetchUnknown,
    /// #506: this repository has no GitHub remote — nothing to show.
    PrGithubUnavailable,
    /// #506: Branch Cleanup's PR column is last-known data.
    CleanupPrEvidenceStale,
    /// PR mode: header of a grouped stack of dependent PRs.
    PrStack,
    /// PR mode: toast shown while that fetch runs.
    PrRefreshing,
    /// Read-only GitHub Issues workspace.
    WorkspaceGraph,
    WorkspacePrs,
    WorkspaceIssues,
    IssuesLoadingList,
    IssuesLoadingMore,
    IssuesRetryLoadMore,
    ListFilterAll,
    ListFilterTitle,
    ListFilterReady,
    ListFilterPassing,
    ListFilterFailing,
    ListFilterSort,
    ListSortComments,
    ListSortAscending,
    ListSortDescending,
    ListFilterClear,
    ListLoadMore,
    PrLoadingMore,
    /// `{}` = the number of locally loaded rows, with `+` when unfinished.
    ListLoadedCount,
    ListMatchesCount,
    IssuesEmpty,
    IssuesFilterEmpty,
    IssuesViewerUnknown,
    IssuesRefreshing,
    IssuesAssignedToMe,
    IssuesCreatedByMe,
    IssuesMentioningMe,
    IssuesRecentlyUpdated,
    IssueStateOpen,
    IssueStateClosed,
    IssueStateUnknown,
    IssueSelect,
    IssueBackToList,
    IssueLoading,
    IssueDetailsUnavailable,
    IssueNoDescription,
    /// Commit panel: tooltip on the commit.template toggle.
    ToggleCommitTemplate,
    /// Commit panel: label on the commit.template toggle.
    Template,
    /// Footer idle status.
    Ready,
    /// Branch menu Sync item when no upstream is configured.
    NoUpstreamSet,

    // ── Merge-into-conflict (W31-MERGE-INTO-CONFLICT) ────────────────
    /// Confirm-button label on a merge plan that will produce conflicts.
    MergeAndResolveConflicts,
    /// Prominent warning shown on the merge modal when conflicts are predicted.
    MergeConflictWarning,

    // ── T-SETTINGS-001 / ADR-0080: Settings window (prose localized; the
    //    domain word "graph" stays English per ADR-0048) ──────────────────
    /// Settings window title.
    SettingsTitle,
    /// Settings pane containing appearance and general application preferences.
    SettingsGeneral,
    /// The native settings host could not be opened.
    SettingsWindowFailed,
    /// Settings sidebar: Appearance page title.
    SettingsAppearance,
    /// Semantic grouped-form headings (#1200).
    SettingsGraph,
    SettingsNetwork,
    SettingsBehaviour,
    /// Compact visible supporting text; full guidance remains in help.
    SettingsLaneCompactSummary,
    SettingsTerminalAutoLockSummary,
    SettingsCustomThemesSummary,
    SettingsAnalyzeIgnoreSummary,
    SettingsGraphCopySummary,
    SettingsSmartProviderSummary,
    SettingsAnalyze,
    SettingsAutoFetchSummary,
    SettingsReduceMotionSummary,
    SettingsSmartEnableSummary,
    /// Settings sidebar: Language page title.
    SettingsLanguage,
    /// Appearance → Theme row title.
    SettingsTheme,
    /// Appearance → Theme row description.
    SettingsThemeDesc,
    /// Apple theme chrome accent preference and detected macOS colour.
    SettingsAccent,
    SettingsAccentSystem,
    SettingsAccentTheme,
    SettingsAccentGraphite,
    SettingsAccentRed,
    SettingsAccentOrange,
    SettingsAccentYellow,
    SettingsAccentGreen,
    SettingsAccentBlue,
    SettingsAccentPurple,
    SettingsAccentPink,
    SettingsAccentDefault,
    /// Appearance → Custom themes row title (#922).
    SettingsCustomThemes,
    /// Appearance → Custom themes row description (#922).
    SettingsCustomThemesDesc,
    /// Appearance → Custom themes row: link to `docs/themes.md` (#922).
    SettingsCustomThemesGuide,
    /// Appearance → Custom themes: open its folder in the OS file manager.
    SettingsCustomThemesOpen,
    /// Appearance → Custom themes: reload JSON files without closing Settings.
    SettingsCustomThemesReload,
    /// Appearance → Custom themes: displayed if there is no home directory.
    SettingsThemesFolderUnavailable,
    /// Appearance → UI Zoom row title.
    SettingsZoom,
    /// Appearance → UI Zoom row description.
    SettingsZoomDesc,
    /// Appearance → relative compact layout threshold.
    SettingsCompactThreshold,
    SettingsCompactThresholdDesc,
    /// Appearance → Compact graph row title.
    SettingsCompact,
    /// Appearance → Compact graph row description.
    SettingsCompactDesc,
    /// Appearance → Lane-compaction (swimlane) row title.
    SettingsLaneCompact,
    /// Appearance → Lane-compaction (swimlane) row description.
    SettingsLaneCompactDesc,
    /// Appearance → Auto-fetch row title.
    SettingsAutoFetch,
    /// Appearance → Auto-fetch row description.
    SettingsAutoFetchDesc,
    /// Appearance → Graph Cmd+C copy-target row title.
    SettingsGraphCopy,
    /// Appearance → Graph Cmd+C copy-target row description.
    SettingsGraphCopyDesc,
    SettingsMarkdownDefaultView,
    SettingsMarkdownDefaultViewDesc,
    MarkdownDefaultPreview,
    MarkdownDefaultSource,
    SettingsToolbarDisplay,
    SettingsToolbarDisplayDesc,
    ToolbarIconOnly,
    ToolbarIconAndText,
    ToolbarBranch,
    ToolbarStash,
    ToolbarPop,
    ToolbarEditor,
    ToolbarAnalyze,
    ToolbarTerminal,
    BottomTabOperationLog,
    BottomTabActivity,
    BottomTabChanges,
    ActivityPeriod,
    ActivityChart,
    ActivityDay,
    ActivityWeek,
    ActivityMonth,
    ActivityYear,
    ActivityAll,
    StatusShowTerminal,
    StatusHideTerminal,
    StatusShowOperationLog,
    StatusHideOperationLog,
    ToolbarOverflow,
    TabOverflow,
    /// Appearance → Reduce-motion row title (issue #354 / ADR-0173).
    SettingsReduceMotion,
    /// Appearance → Reduce-motion row description.
    SettingsReduceMotionDesc,
    /// Appearance → Terminal auto-lock row title (#772 / ADR-0208).
    SettingsTerminalAutoLock,
    /// Appearance → Terminal auto-lock row description.
    SettingsTerminalAutoLockDesc,
    /// Language → Interface language row title.
    SettingsInterfaceLang,
    /// Language → Interface language row description.
    SettingsInterfaceLangDesc,

    // ── ADR-0119: Analyze (Code Ecosystem) ignore section in Settings ─────
    /// Analyze-ignore section header.
    SettingsAnalyzeIgnore,
    /// Analyze-ignore section description (gitignore-syntax editor).
    SettingsAnalyzeIgnoreDesc,
    /// Analyze-ignore → Save button.
    SettingsAnalyzeIgnoreSave,
    /// Analyze-ignore → Reset-to-defaults button.
    SettingsAnalyzeIgnoreReset,
    SettingsAnalyzeIgnoreUnsaved,
    SettingsAnalyzeIgnoreSaved,
    SettingsAnalyzeIgnoreFailed,
    SettingsAnalyzeIgnoreClean,
    SettingsAnalyzeResetTitle,
    SettingsAnalyzeResetDetail,

    // ── ADR-0090 / ADR-0099: Smart Commit section (prose localized; the
    //    domain words "commit" / "LLM" / "Ollama" / "CLI" stay English) ──────
    /// Smart Commit section header (product feature name — English in both arms).
    SettingsSmartCommit,
    /// Smart Commit → Enable toggle row title.
    SettingsSmartEnable,
    /// Smart Commit → Enable toggle row description.
    SettingsSmartEnableDesc,
    /// Smart Commit → Provider row title.
    SettingsSmartProvider,
    /// Smart Commit → Provider row description.
    SettingsSmartProviderDesc,
    /// Smart Commit → LLM model row title.
    SettingsSmartModel,
    /// Smart Commit → LLM model row description.
    SettingsSmartModelDesc,
    /// Smart Commit → model picker note when no local models are detected.
    SettingsSmartNoModels,

    // ── Branch Cleanup (ADR-0128) ───────────────────────────────────
    /// Pane title / sidebar entry ("Merged branches").
    CleanupTitle,
    /// Bulk-delete button label prefix; count is appended at render.
    CleanupDeleteMerged,
    /// "Copy all branch names" header button.
    CleanupCopyAll,
    /// Toast confirming branch name(s) were copied.
    CleanupNamesCopied,
    /// Table column: branch name.
    CleanupColBranch,
    /// Table column: where the branch exists (local / origin chips).
    CleanupColWhere,
    /// Table column: merge date.
    CleanupColMergedAt,
    /// Table column: classification status.
    CleanupColStatus,
    /// Badge: fully merged (tip is an ancestor of the default branch).
    CleanupBadgeMerged,
    /// Badge: probably squash-merged (upstream gone) — no local proof.
    CleanupBadgeSquash,
    /// Badge: merged but the branch grew new commits since (WARN).
    CleanupBadgeGrown,
    /// Badge: stale (no commits for 90 days).
    CleanupBadgeStale,
    /// Tooltip/hint for WARN rows (prefix; ahead count appended).
    CleanupGrownHint,
    /// Empty-table body message.
    // ── Plan-modal confirm labels (destructive ops get an armed variant) ──
    /// Heading for the structured recovery commands in a plan card / Copy all.
    ModalRecoveryCommands,
    // Advice catalog (#353): contextual variants retain their original wording.
    AdviceUntrackedRemain(kagi_domain::plan_note::UntrackedCtx),
    AdviceSuggestStashPush,
    AdviceNoForceUsed(kagi_domain::plan_note::push::PushPunct),
    /// The Operation Log panel's fixed strings (#334, keys in `oplog_panel`).
    OplogPanel(oplog_panel::OplogPanelMsg),
    /// Restore planning and malformed canonical ref-row failures (#993).
    RestoreRepoSessionUnavailable,
    RestorePlanRowsInvalidSeeLog,
    RestorePlanRowsInvalidNoRepository,
    AdviceWillDetachHead,
    AdviceRecommendCreateBranchHereFirst,
    AdviceDirtyStashFirst,
    AdvicePullAutoStash,
    AdviceDivergedSwitchOnly,
    AdviceDeleteUnmerged,
    AdviceUntrackedExcluded,
    AdviceCommonBranchInvalidRef,
    AdviceCommonConflictedFiles,
    AdviceCommonDirtyBlocksOp,
    AdviceCommonDirtyRollbackHint,
    AdviceCommonPartialCloneObjectMissing,
    AdviceCommonSparseExcludedPath,
    AdviceCommonExternalFilter,
    AdviceCommonExternalFilterSync,
    AdviceCommonSparseCheckoutUnsupported,
    AdviceCommonSubmoduleCheckoutUnsupported,
    AdviceCommonHunkChanged,
    AdviceCommonMergeConflictWarning,
    AdviceCommitNothingStaged,
    AdviceCommitStagedContentChanged,
    AdviceCommitIdentityUnavailable,
    AdviceGitIdentityUnavailable,
    AdviceCommitConflictedFiles,
    AdviceCommitLeftoverNotIncluded,
    AdviceDiscardTargetConflicted,
    AdviceDiscardTargetSubmodule,
    AdviceDiscardUntrackedWillBeDeleted,
    AdviceSnapshotSavepointFirst,
    AdviceSnapshotRewritesWorkingTree,
    AdviceRulesetPatternViolation,
    AdviceRulesetPatternUncheckable,
    AdviceRulesetFileTooLarge,
    AdviceRulesetRestrictedExtension,
    AdviceRulesetRestrictedPath,
    AdviceRulesetPathTooLong,
    AdviceRulesetSignatureRequired,
    AdviceRulesetLinearHistoryRequired,
    AdviceRulesetNonFastForward,
    AdviceRulesetCreationBlocked,
    AdviceRulesetConstraintsUnknown,
    AdviceChecklistPossibleSecretFileStaged,
    AdviceChecklistLargeBinaryStaged,
    AdviceChecklistConflictMarkerFound,
    AdviceChecklistPossibleSecretContentStaged,
    AdviceMergeWillConflict,
    AdviceMergeUnrelatedHistories,
    AdviceMergeOperationInProgress,
    AdviceMergeUntrackedWouldBeOverwritten,
    AdviceMergeIntoUnrelatedHistories,
    AdviceMergeIntoCheckedOutElsewhere,
    AdviceMergeIntoWouldConflict,
    AdviceMergeIntoFastForward,
    AdviceMergeIntoWorkingTreeUntouched,
    AdviceMergeIntoCreatesLocalBranch,
    AdviceMergeIntoRemoteSource,
    AdviceMergeIntoLocalDiffersFromRemote,
    AdviceRebaseDirtyWorkingTree,
    AdviceRebaseMayConflict,
    AdviceCherryRevertWouldConflictCherryPick,
    AdviceCherryRevertWouldConflictRevert,
    AdviceCherryRevertDirtyMayRefuse,
    AdviceResetRefOnlySoftReset,
    AdviceResetAbandonsCommits,
    AdviceResetTargetNotAncestor,
    AdviceHistoryPushedHistoryRewriteUndo,
    AdviceHistoryPushedHistoryRewriteAmend,
    AdviceHistoryAmendDivergesFromRemote,
    AdviceHistoryNothingStagedForAmend,
    AdviceHistoryWrongBranch,
    AdviceHistoryHeadNotOnBranch,
    AdviceHistoryEntryStaleBranchMoved,
    AdviceHistoryEntryStaleUnreachable,
    AdviceHistorySoftMovePreservesChanges,
    AdviceConflictsObservationChanged,
    AdviceConflictsSkipStepChanged,
    AdviceConflictsPlanChanged,
    AdviceConflictsRepositoryIdentityChanged,
    AdviceConflictsConflictGone,
    AdviceConflictsAbortStartUnavailable,
    AdviceConflictsAbortHeadMoved,
    AdviceConflictsDraftPreservationFailed,
    AdviceConflictsResolutionMarkers,
    AdviceConflictsUnresolvedFiles,
    AdviceConflictsMarkerResidue,
    AdviceConflictsIndexUnmerged,
    AdviceConflictsBinaryUnresolved,
    AdviceConflictsDeletionUndecided,
    AdviceConflictsEmptyMergeMessage,
    AdviceConflictsNoConflictingFilesDetected,
    AdviceConflictsPartialResolutionsPreserved,
    AdviceConflictsSkipDiscardsStep,
    AdviceBranchRenameRefOnlyDirty,
    AdviceBranchRenameRemoteNotRenamed,
    AdviceBranchRenameConfig,
    AdviceBranchRenameConfigChanged,
    AdviceBranchRenameConfigExternal,
    AdviceBranchRenameConfigConditional,
    AdviceBranchRenameConfigHeader,
    AdviceBranchDeleteCurrentBranch,
    AdviceBranchDeleteReachabilityChanged,
    AdviceBranchDeleteBranchCheckedOut,
    AdviceBranchDeleteBranchInLockedWorktree,
    AdviceBranchDeleteBranchInDirtyWorktree,
    AdviceBranchDeleteRemovesPinningWorktree,
    AdviceBranchDeleteSquashMerged,
    AdviceBranchDeleteKeepsRemote,
    AdviceCheckoutCheckoutOverlap,
    AdviceCheckoutDirtyMayFail,
    AdviceSwitchWillCreateTracking,
    AdviceSwitchFfLocalKnowledge,
    AdviceSwitchAheadSwitchOnly,
    AdviceWorktreeDirtyBlocksCheckoutAfterCreate,
    AdviceWorktreeCreatesLinkedWorktree,
    AdviceWorktreeLockedWithReason,
    AdviceWorktreeIncludeCopy,
    AdviceWorktreeIncludeSkippedSymlinks,
    AdviceWorktreeIncludeOverCap,
    AdviceWorktreeRemoveDirty,
    AdviceWorktreeRemoveLocked,
    AdviceWorktreeRemoveLiveShell,
    AdviceWorktreeRemoveLeftoverProcesses,
    AdviceWorktreeRemoveIgnoredFiles,
    AdviceWorktreeRemoveIgnoredFilesAndFolders,
    AdviceWorktreeRemovesWorktreeDeleteBranch,
    AdviceWorktreeRemovesWorktreeKeepBranch,
    AdviceWorktreeLocksWorktree,
    AdviceWorktreePrunePreview,
    AdviceWorktreeRepairsWorktrees,
    AdviceWorktreePostCreateSteps,
    AdviceWorktreePostCreateStepsTrustRequired,
    AdviceWorktreePreRemoveSteps,
    AdviceWorktreePreRemoveStepsTrustRequired,
    AdviceTagNameErrorEmpty,
    AdviceTagNameErrorLeadingDash,
    AdviceTagPushRemoteSideEffect,
    AdviceTagPushRejectedIfMoved,
    AdviceCleanupNoLongerCandidate,
    AdviceCleanupNotSafelyDeletable,
    AdviceCleanupTipMoved,
    AdviceCleanupSquashHeuristicOnly,
    AdviceCleanupRemoteDeleteNetwork,
    AdvicePushNoUpstreamNoRemotes,
    AdvicePushUpstreamFormatInvalid,
    AdvicePushUpstreamNotPresentLocally,
    AdvicePullNoUpstreamWithHint,
    AdvicePullStrictFfLocalChangesOverlap,
    AdvicePullMergePrediction,
    AdvicePullRestoreConflict,
    AdvicePullRestoreConflictPossible,
    AdvicePullRestoreConflictMore,
    AdvicePullCannotFastForward,
    AdvicePullRemoteDiverged,
    AdvicePullRemoteDirty,
    AdvicePullRemotePreviewStale,
    AdviceStashHeadRequired,
    AdviceStashUntrackedIncluded,
    AdviceStashDirtyBlocksApply,
    AdviceStashConflictUnknownFiles,
    AdviceStashPopWouldConflict,
    AdviceStashApplyWouldConflict,
    AdviceStashApplyPredictionUnavailable,
    AdviceStashPopPredictionUnavailable,
    AdviceStashRemoteDropIrreversible,
    AdviceStashTargetChanged,
    AdviceStashListChanged,
    AdviceRemoteBranchLocalBranchUntouched,
    AdviceForceLeaseNoUpstream,
    AdviceForceLeaseRewritesRemoteHistory,
    AdviceForceLeaseLeaseValue,
    AdviceGithubHeadUnavailable,
    AdviceGithubNotMergeable,
    AdviceGithubIsDraft,
    AdviceGithubChecksFailing,
    AdviceGithubChecksPending,
    AdviceGithubChangesRequested,
    AdviceGithubRemoteSideEffect,
    AdviceGithubDeletesBranch,
    AdviceGithubForkKeepsRemoteBranch,
    AdviceGithubDeletesLocalBranchPresent,
    AdviceGithubDeletesLocalBranchAbsent,
    AdviceGithubKeepsLocalBranch,
    AdviceGithubLocalBranchKept,
    AdviceGithubLocalBranchNotDeleted,
    AdviceGithubSuggestionRangeGone,
    AdviceGithubSuggestionStale,
    AdviceGithubSuggestionNotPrHead,
    AdviceGithubSuggestionHeadUnavailable,
    AdviceGithubSuggestionWorkingTreeOnly,
    AdviceGithubCommentBodyEmpty,
    AdviceGithubIssueTitleEmpty,
    AdviceGithubIssueCreateFields,
    AdviceGithubIssueUnknownLabels,
    AdviceGithubIssueUnassignableUsers,
    AdviceGithubReviewBodyEmpty,
    AdviceGithubFieldEditEmpty,
    AdviceGithubLocalBranchNotDeletedDeletionUnauthorized,
    /// #454: header of the always-visible list of files an amend folds into the
    /// commit. `{}` is appended by the caller as `" (N)"`.
    AmendFoldedFiles,
    /// #454: tooltip on a popup's copy button (copies the whole dialog).
    ModalCopyAll,
    // #1016 modal polish: confirm labels, disabled reasons and maintenance state.
    ModalCherryPickConfirm,
    ModalCommitConfirm,
    ModalNoDiscardTargets,
    ModalPlanNotReady,
    /// Repository-health plan AFTER chips, full descriptions, and confirm labels.
    MaintenanceCommitGraphAfter,
    MaintenanceFsmonitorAfter,
    MaintenanceCommitGraphDetail,
    MaintenanceFsmonitorDetail,
    MaintenanceWriteCommitGraph,
    MaintenanceEnableFsmonitor,
    PlanCreateBranchHere,
    InputApply,
    /// Header of the distinct equivalent CLI command in Copy all.
    ModalEquivalentCommand,
    /// #454: tooltip on a list panel's copy button (copies every row).
    ModalCopyList,
    /// #454: toast after a popup copy.
    ModalCopied,
    /// #454: header chip on a card whose plan is `destructive` — the one
    /// piece of the mock that states the stakes before the button does.
    ModalCannotUndo,
    /// #454: section header for the files an operation acts on.
    ModalTargetFiles,
    /// #454: section header for a plan's warnings (what will NOT be touched).
    ModalWarningsSection,
    /// #454: collapsible-section header for the paths discard skips
    /// (untracked / conflicted). The count is rendered by `modal_section`.
    DiscardSkippedSection,
    /// #454: checkout-overlap blocker summary, `{}` = overlap count. The
    /// paths themselves are rendered as a list under this line, not joined
    /// into the sentence (40 paths used to fill the whole card).
    PlanOverlapSummary,
    /// #625: the summary line above the list of paths whose auto-stash restore
    /// would conflict, so the paths render as rows instead of a comma wall.
    PlanRestoreConflictSummary,
    /// #625: same, for the paths kagi could **not** decide in advance — the
    /// wording says *may* conflict and must stay distinct.
    PlanRestoreConflictMaybeSummary,
    /// Short operation labels and count templates used in plan heading chips.
    PlanHeadingSetUpstream,
    PlanHeadingApprove,
    PlanHeadingComment,
    PlanHeadingRequestChanges,
    PlanHeadingBehind,
    PlanHeadingFiles,
    PlanHeadingStashes,
    PlanHeadingBranches,
    /// Confirm button on the set-upstream modal, `{}` = branch.
    PlanSetUpstreamFor,
    /// Confirm button on the rename-branch modal, `{}` = old name.
    PlanRenameBranch,
    /// Confirm button on the merge modal.
    PlanMerge,
    /// Confirm button on the rebase modal, `{}` = branch.
    PlanRebaseOnto,
    PlanRebaseCandidates,
    PlanRebaseMore,
    PlanRebaseResultUnknown,
    PlanBranchesToDelete,
    PlanCommitsToPush,
    PlanTagPublishScope,
    PlanTagName,
    PlanTagKind,
    PlanTagObject,
    PlanTagTarget,
    PlanTagRemote,
    PlanTagPushDestination,
    PlanTagLocalUnchanged,
    PlanReplayOnto,
    PlanReplayOntoArmed,
    ThemeColorVision,
    A11yCommitList,
    A11yCommitRow,
    A11yCommitRefs,
    A11yWipRow,
    A11yStashRow,
    A11ySidebarLocal,
    A11ySidebarRemote,
    A11ySidebarWorktrees,
    A11ySidebarTags,
    A11ySidebarStashes,
    A11ySidebarGroup,
    A11ySidebarBranch,
    A11ySidebarCurrentBranch,
    A11ySidebarRemoteBranch,
    A11ySidebarTag,
    SidebarTagCommitNotLoaded,
    SidebarTagHiddenBySolo,
    A11ySidebarStash,
    A11ySidebarWorktree,
    A11ySidebarCurrentSuffix,
    A11ySidebarLockedSuffix,
    A11yPrList,
    A11yPrRow,
    A11yFileTree,
    A11yUnstagedTree,
    A11yStagedTree,
    CommitPanelTitle,
    CommitPanelUnstaged,
    CommitPanelStaged,
    CommitPanelList,
    CommitPanelTree,
    CommitPanelStageAll,
    CommitPanelUnstageAll,
    CommitPanelDiscardAll,
    CommitPanelNoStageTargets,
    CommitPanelNothingUnstaged,
    CommitPanelConflict,
    CommitPanelStage,
    CommitPanelUnstage,
    A11yFileTreeDir,
    A11yFileTreeFile,
    A11yFileTreeUnsaved,
    A11yFileAdded,
    A11yFileModified,
    A11yFileDeleted,
    A11yFileRenamed,
    A11yFileTypeChanged,
    A11yFileUnchanged,
    A11yFileConflicted,
    A11yDialogTwoStage,
    A11yDialogArmed,
    A11yDialogBlocked,
    PlanAmendConfirm,
    PlanAmendConfirmArmed,
    AmendArmedNotice,
    AmendArmedRecovery,
    PlanDiscardConfirm,
    PlanDiscardConfirmArmed,
    DiscardArmedNotice,
    DiscardBackedOutcome,
    DiscardAllTitle,
    DiscardUnsafePath,
    PlanSyncToRemote,
    PlanSyncToRemoteArmed,
    /// Delete-remote-branch confirm, and its armed second stage.
    PlanDeleteBranch,
    PlanDeleteBranchArmed,
    PlanDeleteRemoteBranch,
    PlanDeleteRemoteBranchArmed,
    /// Recoverable ref-only reset confirmation.
    PlanResetCurrent,
    /// Force-with-lease push confirm, and its armed second stage.
    PlanForcePush,
    /// Cancel button, shared by every modal.
    PlanCancel,
    // Input-confirm cards (#956).
    InputBranchTitle,
    InputTagTitle,
    InputWorktreeTitle,
    InputStashTitle,
    InputBranchName,
    InputNewBranchName,
    InputTagName,
    InputWorktreePath,
    // #1043 — Add Worktree path: the folder dialog's button.
    InputChooseFolder,
    InputStashMessage,
    InputStashCurrent,
    InputStashAfter,
    /// The status chip of a plan state with no working-tree changes.
    PlanStateClean,
    PlanStateUntrackedRetained,
    InputStashUntrackedWarning,
    InputUpstream,
    InputCheckoutAfterCreate,
    InputCreate,
    InputStash,
    InputSetUpstream,
    InputRename,
    InputPlanPending,
    InputRenameEnterNewName,
    CleanupEmpty,
    /// Branch Cleanup: shown while the background scan is still running.
    CleanupScanning,
    /// Branch Cleanup: PR column header.
    CleanupColPr,
    /// Branch Cleanup: author column header.
    CleanupColAuthor,
    /// Branch Cleanup: delete the ticked rows.
    CleanupDeleteSelected,
    // ── Tag context menu / push (ADR-0140) ──
    /// Tag menu: push item, `{}` = remote name.
    TagPushTo,
    /// Tag menu: push item when no remote is configured.
    TagPush,
    /// Tag menu: why the push item is disabled.
    TagNoRemote,
    /// Tag menu: copy the tag name.
    TagCopyName,
    /// Confirm button on the push-tag modal.
    PushTagConfirm,
    /// Status bar while the push runs.
    BusyPushTag,
    /// Status bar after it succeeds.
    PushTagDone,
    /// Body message when no repository is open.
    CleanupNoRepo,

    // ── Code Ecosystem / hot-spots (ADR-0119) ───────────────────────
    /// Ecosystem view title / toolbar button label.
    Ecosystem,
    /// "Copy diagnostic" button (export the current mode's view as LLM context).
    EcoCopyDiagnostic,
    /// Toast confirming the diagnostic was copied to the clipboard.
    EcoDiagnosticCopied,
    /// Body placeholder while the repo is being mined.
    EcoLoading,
    /// Secondary line under the loading spinner (large-repo expectation).
    EcoLoadingHint,
    /// Body message when the mine fails (followed by the error detail).
    EcoLoadFailed,
    /// Body message when there is no churn to show.
    EcoEmpty,
    /// Hotspots sub-view toggle: ranked list.
    EcoList,
    /// Hotspots sub-view toggle: treemap heatmap.
    EcoMap,
    /// Header of the expanded 1:many coupling panel ("couples with <file>").
    EcoCouplesWith,
    /// Coupling sub-view toggle: force-directed graph.
    EcoGraph,
    /// Coupling sub-view toggle: Mermaid flowchart source.
    EcoMermaid,
    /// Mermaid sub-view: "open in mermaid.live" button.
    EcoOpenMermaidLive,
    /// Mermaid sub-view: hint under the action bar.
    EcoMermaidHint,
    /// Graph viewport reset button (zoom/pan back to fit).
    EcoResetView,
    /// Help overlay title.
    EcoHelpTitle,
    EcoMode,
    EcoPeriod,
    EcoView,
    EcoExportFormat,

    // ── Editor Workspace (T-WS-EDITOR-001) ──────────────────────
    /// Left/center/right placeholder while the working-tree file list (or a
    /// selected file's content/diff) is loading.
    EditorWorkspaceLoading,
    /// Left/center placeholder when the working tree has no changed files.
    EditorWorkspaceEmpty,
    /// Center placeholder when no file is selected yet.
    EditorWorkspaceSelectFile,
    /// Center placeholder for a binary file (no text preview).
    EditorWorkspaceBinary,
    /// Center placeholder for a file over the line-count guard.
    EditorWorkspaceTooLarge,
    /// Right pane placeholder when the selected file has no WIP diff to show.
    EditorWorkspaceNoDiff,
    /// Tree-source chip: changed files only (T-WS-EDITOR-004).
    EditorWorkspaceSourceChanges,
    /// Tree-source chip: every tracked + untracked file (T-WS-EDITOR-004).
    EditorWorkspaceSourceAll,
    /// Center placeholder when the selected file's `fs::read` failed or its
    /// `ChangeKind` is `Deleted` (T-WS-EDITOR-005 finding #6).
    EditorWorkspaceDeleted,
    /// Center placeholder when the selected file's bytes aren't valid UTF-8
    /// (and weren't flagged binary by the NUL-byte probe; T-WS-EDITOR-005
    /// finding #6).
    EditorWorkspaceUndecodable,
    /// File History unavailable pre-rename path; `{path}` is its current name.
    FileHistoryPathRenamed,

    // ── Editor Workspace editable buffer (T-WS-EDITOR-002) ──────
    /// Banner shown above a dirty buffer when the FS watcher sees the file
    /// change on disk (not auto-reloaded, to avoid clobbering the edit).
    EditorWorkspaceExternalChanged,
    /// Button on the external-change banner: discard the buffer and re-read.
    EditorWorkspaceReload,
    /// Button on the external-change banner: save anyway, replacing the
    /// externally-changed on-disk file with the buffer.
    EditorWorkspaceOverwrite,
    /// Toast when Cmd-S is refused because the file changed on disk since
    /// the buffer was loaded (nothing was written).
    EditorWorkspaceSaveBlocked,
    /// Unsaved-changes confirmation title (file/source switch, close).
    EditorWorkspaceUnsavedTitle,
    EditorHostUnsavedTitle,
    EditorHostUnsavedCount,
    EditorHostSaveAll,
    EditorHostSaveWaiting,
    EditorHostRecoveryRequired,
    ConflictDraftSaveFailed,
    ConflictDraftRetry,
    ConflictDraftDiscardClose,
    ConflictDraftDiscardConfirm,
    ConflictDraftUnsavedDetail,
    ConflictDraftSavingTitle,
    ConflictDraftSaving,
    ConflictDraftDiscardDetail,
    ConflictDraftMixedDiscardDetail,
    EditorWorkspaceSaveInterrupted,
    /// Unsaved-changes confirmation: destructive "discard and proceed" button.
    EditorWorkspaceDiscard,
    /// Unsaved-changes confirmation: cancel button (stay on the buffer).
    EditorWorkspaceCancel,
    /// Editor Workspace header: the "← Graph" back button.
    EditorWorkspaceBackToGraph,
    /// Editor Workspace header: the "Editor Workspace" title label.
    EditorWorkspaceTitle,
    /// Editor's independent supplemental Diff/History pane.
    EditorDetailsShow,
    EditorDetailsHide,
    EditorDetailsUnavailable,

    // ── Repository owner-trust prompt (ADR-0160 / issue #310) ────
    /// Trust-confirmation title: repo owned by another user (in-process
    /// library open path — ADR-0160).
    TrustRepoTitle,
    /// Trust-confirmation body: why writes are blocked + what confirming does.
    TrustRepoBody,
    /// Trust-confirmation: the "trust this repository" confirm button.
    TrustRepoConfirm,
    TrustRepoSaveFailed,
    TrustRepoReopenFailed,
    TrustRepoRetry,

    // ── Editor Workspace History/Snapshot tabs (T-WS-EDITOR-008) ─
    /// Right-pane tab: the open file's WIP hunks (default, unchanged v1
    /// behaviour).
    EditorRightTabDiff,
    /// Right-pane tab: the open file's commit history.
    EditorRightTabHistory,
    /// Center-pane tab (once a History row is selected): the selected
    /// commit's diff for this file.
    EditorMiddleTabDiff,
    /// Center-pane tab (once a History row is selected): the file's full
    /// read-only text as of the selected commit.
    EditorMiddleTabSnapshot,
    /// Right-pane History tab placeholder when the file has no commit
    /// history.
    EditorHistoryEmpty,
    EditorHistoryFailed,
    EditorHistoryRetry,
    /// Snapshot tab placeholder when the load finished but found nothing
    /// (e.g. the path didn't exist yet at that commit).
    EditorSnapshotUnavailable,
    /// Historical content is selectable, never editable.
    EditorSnapshotReadOnly,
    /// Return from a historical revision to the live buffer.
    EditorOpenCurrent,

    // ── Editor Workspace tree context menu (T-WS-EDITOR-007) ────
    /// File/dir row: rename.
    EditorTreeRename,
    /// File/dir row: delete (moves to Trash — never permanent).
    EditorTreeDelete,
    /// File/dir row: copy the absolute path.
    EditorTreeCopyPath,
    /// File/dir row: copy the repo-relative path.
    EditorTreeCopyRelativePath,
    /// File/dir row: reveal (and select) in Finder.
    EditorTreeRevealFinder,
    /// File/dir row: reveal in the platform file manager (non-macOS label —
    /// macOS uses `EditorTreeRevealFinder`).
    EditorTreeRevealFile,
    EditorTreePreviewMarkdown,
    EditorWorkspacePreviewShow,
    EditorWorkspacePreviewEdit,
    EditorFindPrevious,
    EditorFindNext,
    EditorFindReplace,
    EditorFindReplaceAll,
    EditorFindClose,
    EditorFindCase,
    EditorFindReplaceMode,
    EditorFindField,
    EditorFindReplacementField,
    EditorWorkspaceMermaidRendering,
    EditorWorkspaceMermaidNeedsCli,
    EditorWorkspaceMermaidFailed,
    EditorWorkspaceMermaidNoDetails,
    /// File row: jump to File History.
    EditorTreeHistory,
    /// File row: stage this file (index-only, non-destructive).
    EditorTreeStage,
    /// File row: unstage this file.
    EditorTreeUnstage,
    /// File row: discard working-tree changes (tracked, changed files only).
    EditorTreeDiscard,
    /// File row (untracked only): append this path to `.gitignore`.
    EditorTreeAddGitignore,
    /// Dir row / empty area: create a new empty file.
    EditorTreeNewFile,
    /// Dir row / empty area: create a new empty folder.
    EditorTreeNewFolder,
    /// Rename prompt modal title.
    EditorFsPromptRenameTitle,
    /// New File prompt modal title.
    EditorFsPromptNewFileTitle,
    /// New Folder prompt modal title.
    EditorFsPromptNewFolderTitle,
    /// Fs-prompt modal: the name input's label.
    EditorFsPromptNameLabel,
    /// Fs-prompt modal: the New File/New Folder confirm button.
    EditorFsPromptCreateButton,
    /// Delete-confirm modal title for a file target.
    EditorDeleteConfirmTitleFile,
    /// Delete-confirm modal title for a directory target.
    EditorDeleteConfirmTitleFolder,
    /// Delete-confirm modal: the "recoverable from Trash" note.
    EditorDeleteConfirmTrashNote,
    /// Delete-confirm modal: warning that affected editor buffers are dirty.
    EditorDeleteConfirmUnsavedWarning,
    /// Delete-confirm modal: the confirm button.
    EditorDeleteConfirmButton,
    /// issue #348: Inspector "Generated (N)" fold section label (the count is
    /// appended by the caller).
    GeneratedFilesSection,
    /// issue #338: "Agent artifacts (N)" fold section label (count appended by
    /// the caller).
    AgentArtifactsSection,
    /// issue #338: emphasis badge on a convention-body file (AGENTS.md etc.).
    AgentConventionBadge,

    // ── issue #356: unsafe-unicode row badge (bidi / zero-width) ─────
    /// Small badge shown on a diff / review / conflict row that contains
    /// bidirectional-control or zero-width codepoints.
    UnsafeUnicodeBadge,
    /// Tooltip explaining the [`Msg::UnsafeUnicodeBadge`] badge.
    UnsafeUnicodeTooltip,
    // ── issue #346: GitHub-ruleset live-validation badge ─────────────
    /// Small badge shown next to the commit-message / branch-name field when
    /// the current input would violate the fetched GitHub branch ruleset.
    RulesetBadge,
    /// Tooltip lead-in for [`Msg::RulesetBadge`] (the specific finding text is
    /// appended by the caller).
    RulesetBadgeTooltip,

    /// issue #412: generic agent name shown in the provenance tooltip when the
    /// real `Co-authored-by:` label carries bidi / zero-width codepoints that
    /// `sanitize_control_bytes` deliberately does not rewrite.
    AgentUnnamed,

    /// issue #350: blame legend tooltip for the `*` marker on a line whose
    /// commit is listed in `.git-blame-ignore-revs`.
    BlameIgnoredMarkTip,
    /// issue #350: blame legend tooltip for the `?` marker on an unblamable
    /// (uncommitted) line.
    BlameUnblamableMarkTip,
    /// Live buffer text with no unchanged HEAD line attribution.
    BlameUncommitted,
    /// issue #350: the Editor Workspace's inline-blame toggle chip. "Blame"
    /// is a Git term of art (ADR-0048) — verbatim in both languages.
    EditorWorkspaceBlame,
    EditorPdfPrevious,
    EditorPdfNext,
    EditorPdfZoomOut,
    EditorPdfZoomIn,
    EditorPdfResetZoom,
    EditorPdfFitWidth,
    EditorPdfPage,
    EditorPdfLoading,
    EditorPdfInvalid,
    EditorPdfPassword,
    EditorPdfEncryption,
    EditorPdfDimensions,
    EditorPdfNoDiff,
    EditorPdfPartial,
    EditorPdfReadOnly,
    // #1016 PR B — modal controls, validation and AFTER-state chips.
    CloneChangeFolderWhileRunning,
    RemoteConnectTitle,
    RemoteSshNotice,
    RemoteHostLabel,
    RemotePortLabel,
    RemoteIdentityLabel,
    RemoteHostInvalid,
    RemotePortInvalid,
    RemoteConnect,
    RemoteConnecting,
    RemoteRequestBusy,
    RemoteChangeHost,
    RemoteGitRepository,
    RemoteDetached,
    RemoteNoCommits,
    RemoteOpenRepository,
    RepositoryPickerOpen,
    PathPickerFailed,
    RemoteOpeningRepository,
    RemoteClose,
    RemoteDirectoryList,
    RemoteParentDirectory,
    RemoteDirectoryRow,
    RemoteFileRow,
    RemoteDirectoryEmpty,
    SmartConsentTitle,
    SmartConsentIntro,
    SmartConsentEnable,
    SmartConsentStagedOnly,
    SmartConsentUnstagedExcluded,
    SmartConsentLocalhost,
    SmartConsentSecretsWarning,
    SmartModelTitle,
    SmartModelRemembered,
    SmartRuleBasedInserted,
    SmartNoModelsInserted,
    SmartGeneratingLocal,
    SmartGeneratingProvider,
    SmartGeneratedLocal,
    SmartGenerationFallback,
    SmartGenerationFailed,
    UpdateAvailableTitle,
    UpdateSkipVersion,
    UpdateReleasePage,
    UpdateLater,
    UpdateHideContinuing,
    UpdateInstalling,
    UpdateInstallingReason,
    UpdateRestartReady,
    UpdateRestart,
    UpdateRestartWaiting,
    UpdateFailed,
    UpdateNow,
    PrFieldsNoChanges,
    EditorFsNameEmpty,
    EditorFsNameInvalid,
    EditorFsNameSeparator,
    EditorFsRenameGitBlocked,
    EditorFsCreateGitBlocked,
    EditorFsAlreadyExists,
    EditorDeleteUnsupported,
    EditorDeleteGitBlocked,
    EditorTrashUnsupported,
    EditorTrashHomeUnset,
    EditorTrashCreateDirFailed,
    EditorTrashPathMissingName,
    EditorTrashTooManyNames,
    EditorTrashSameVolumeOnly,
    PrMergeMethodCommit,
    PrMergeMethodSquash,
    PrMergeMethodRebase,
    PrMergeSessionUnavailable,
    AfterStashDrop,
    AfterStashPop,
    AfterHistoryUndo,
    AfterHistoryAmend,
    AfterHistoryMove,
    AfterSwitchLatest,
    AfterReviewApproved,
    AfterReviewCommented,
    AfterReviewChanges,
    AfterPrMerged,
}

/// Localize the pure, typed removal assessment; backend error text is not a verdict.
pub fn worktree_removal_verdict_text(
    verdict: kagi_domain::remove::WorktreeRemovalVerdict,
) -> &'static str {
    use kagi_domain::remove::{WorktreeRemovalVerdict as V, WorktreeUnknownReason as U};
    match verdict {
        V::SafeMerged => Msg::WorktreeSafeMerged,
        V::SafePushed => Msg::WorktreeSafePushed,
        V::MainWorktree => Msg::WorktreeMainExcluded,
        V::Dirty => Msg::WorktreeDirty,
        V::Locked => Msg::WorktreeLocked,
        V::Unpublished => Msg::WorktreeUnpublished,
        V::Unknown(U::Unborn) => Msg::WorktreeUnborn,
        V::Unknown(U::UpstreamUnavailable) => Msg::WorktreeUpstreamUnavailable,
        V::Unknown(U::ObservationFailed) => Msg::WorktreeObservationFailed,
    }
    .t()
}

impl Msg {
    /// Operation-inclusive elapsed status, formatted without an intermediate string.
    pub fn clone_running_status(seconds: u64) -> String {
        match lang() {
            Lang::En => format!("Cloning… {seconds} s"),
            Lang::Ja => format!("clone 中… {seconds} 秒"),
        }
    }

    /// Resolve this message to a `&'static str` in the active [`lang()`].
    ///
    /// Domain words (Pull / Push / Branch / Stash / Pop / Undo / amend /
    /// checkout / cherry-pick / revert / discard / worktree / HEAD / branch /
    /// upstream / stash …) appear verbatim in both arms per ADR-0048.
    pub fn t(self) -> &'static str {
        self.t_for(lang())
    }

    pub(crate) fn t_for(self, language: Lang) -> &'static str {
        use kagi_domain::advice_template_display_en as advice_en;
        use kagi_domain::plan_note::{push::PushPunct, UntrackedCtx};
        use Lang::{En, Ja};
        use Msg::*;
        match (language, self) {
            (En, InspectorOpenLink) => "Open Link",
            (Ja, InspectorOpenLink) => "リンクを開く",
            (En, InspectorCopyLink) => "Copy Link",
            (Ja, InspectorCopyLink) => "リンクをコピー",
            (En, A11yCommandPaletteQuery) => "Command search",
            (Ja, A11yCommandPaletteQuery) => "コマンド検索",
            (En, A11yCommitSummary) => "Commit summary",
            (Ja, A11yCommitSummary) => "commit の概要",
            (En, A11yCommitDescription) => "Commit description",
            (Ja, A11yCommitDescription) => "commit の説明",
            (En, A11yConflictResult) => "Conflict resolution",
            (Ja, A11yConflictResult) => "conflict の解決結果",
            (En, A11yIssueTitle) => "Issue title",
            (Ja, A11yIssueTitle) => "Issue のタイトル",
            (En, A11yIssueBody) => "Issue description",
            (Ja, A11yIssueBody) => "Issue の説明",
            (En, A11yCommentBody) => "Comment",
            (Ja, A11yCommentBody) => "コメント",
            (En, A11yHomeFilter) => "Search repositories",
            (Ja, A11yHomeFilter) => "リポジトリを検索",
            (En, A11yListTitleFilter) => "Filter by title",
            (Ja, A11yListTitleFilter) => "タイトルで絞り込み",
            (En, A11ySidebarFilter) => "Filter sidebar",
            (Ja, A11ySidebarFilter) => "サイドバーを絞り込み",
            (En, A11ySidebarClearSearch) => "Clear search",
            (Ja, A11ySidebarClearSearch) => "検索内容を消去",
            (En, SidebarGroupFilterExpanded) => "Groups stay expanded while filtering. Clear the filter to collapse a group.",
            (Ja, SidebarGroupFilterExpanded) => "絞り込み中はグループを展開したままにします。グループを折りたたむには絞り込みを解除してください。",
            (En, A11yPrReviewersFilter) => "Filter pull request reviewers",
            (Ja, A11yPrReviewersFilter) => "pull request のレビュー担当者を絞り込み",
            (En, A11yPrAssigneesFilter) => "Filter pull request assignees",
            (Ja, A11yPrAssigneesFilter) => "pull request の担当者を絞り込み",
            (En, A11yPrLabelsFilter) => "Filter pull request labels",
            (Ja, A11yPrLabelsFilter) => "pull request のラベルを絞り込み",
            (En, A11yStashMessage) => "Stash message",
            (Ja, A11yStashMessage) => "stash メッセージ",
            (En, A11yAnalyzeIgnorePatterns) => "Analyze ignore patterns",
            (Ja, A11yAnalyzeIgnorePatterns) => "Analyze の除外パターン",
            (En, A11yEditorFileContents) => "File contents",
            (Ja, A11yEditorFileContents) => "ファイルの内容",
            (En, AboutTitle) => "About Kagi",
            (Ja, AboutTitle) => "Kagiについて",
            (En, AboutDescription) => "A safety-first Git GUI client",
            (Ja, AboutDescription) => "安全を優先する Git GUI クライアント",
            (En, MenuHideSidebar) => "Hide Sidebar",
            (Ja, MenuHideSidebar) => "サイドバーを非表示",
            (En, MenuShowSidebar) => "Show Sidebar",
            (Ja, MenuShowSidebar) => "サイドバーを表示",
            (En, MenuEnterFullScreen) => "Enter Full Screen",
            (Ja, MenuEnterFullScreen) => "フルスクリーンにする",
            (En, MenuExitFullScreen) => "Exit Full Screen",
            (Ja, MenuExitFullScreen) => "フルスクリーンを解除",
            (En, MenuPreviousTab) => "Show Previous Tab",
            (Ja, MenuPreviousTab) => "前のタブを表示",
            (En, MenuNextTab) => "Show Next Tab",
            (Ja, MenuNextTab) => "次のタブを表示",
            (En, MenuBringAllToFront) => "Bring All to Front",
            (Ja, MenuBringAllToFront) => "すべてを手前に移動",
            (En, NoOtherTabs) => "No other tabs",
            (Ja, NoOtherTabs) => "ほかのタブがありません",
            (En, MenuHideApp) => "Hide Kagi",
            (Ja, MenuHideApp) => "Kagiを隠す",
            (En, MenuHideOthers) => "Hide Others",
            (Ja, MenuHideOthers) => "ほかを隠す",
            (En, MenuShowAll) => "Show All",
            (Ja, MenuShowAll) => "すべてを表示",
            (En, MenuServices) => "Services",
            (Ja, MenuServices) => "サービス",
            (En, MenuUndo) => "Undo",
            (Ja, MenuUndo) => "取り消す",
            (En, MenuRedo) => "Redo",
            (Ja, MenuRedo) => "やり直す",
            (En, MenuCut) => "Cut",
            (Ja, MenuCut) => "カット",
            (En, MenuCopy) => "Copy",
            (Ja, MenuCopy) => "コピー",
            (En, MenuPaste) => "Paste",
            (Ja, MenuPaste) => "ペースト",
            (En, MenuSelectAll) => "Select All",
            (Ja, MenuSelectAll) => "すべてを選択",
            (En, MenuSave) => "Save",
            (Ja, MenuSave) => "保存",
            (En, MenuOpenRecent) => "Open Recent",
            (Ja, MenuOpenRecent) => "最近使った項目を開く",
            (En, MenuClearRecent) => "Clear Menu",
            (Ja, MenuClearRecent) => "メニューを消去",
            (En, MenuFind) => "Find",
            (Ja, MenuFind) => "検索",
            (En, MenuFindOpen) => "Find…",
            (Ja, MenuFindOpen) => "検索…",
            (En, MenuWorkspaceGraph) => "Graph",
            (Ja, MenuWorkspaceGraph) => "グラフ",
            (En, MenuWorkspacePrs) => "Pull Requests",
            (Ja, MenuWorkspacePrs) => "Pull Request",
            (En, MenuWorkspaceEditor) => "Editor",
            (Ja, MenuWorkspaceEditor) => "エディター",
            (En, NothingToSave) => "No editable unsaved file",
            (Ja, NothingToSave) => "保存できる未保存のファイルがありません",
            (language, OplogPanel(key)) => key.t_for(language),
            (En, RestoreRepoSessionUnavailable) => "The repository is no longer open. Reopen it and try again.",
            (Ja, RestoreRepoSessionUnavailable) => "リポジトリが開かれていません。開き直してからやり直してください。",
            (En, RestorePlanRowsInvalidSeeLog) => {
                "Can't restore: this Operation Log entry is damaged. See the Operation Log for details."
            }
            (Ja, RestorePlanRowsInvalidSeeLog) => {
                "復元できません: この操作記録が破損しています。詳細は操作ログを確認してください。"
            }
            (En, RestorePlanRowsInvalidNoRepository) => {
                "Can't restore: this Operation Log entry is damaged. No repository is open."
            }
            (Ja, RestorePlanRowsInvalidNoRepository) => {
                "復元できません: この操作記録が破損しています。リポジトリが開かれていません。"
            }
            (En, AdviceUntrackedRemain(ctx)) => match ctx {
                UntrackedCtx::AfterCheckout => advice_en!(UntrackedAfterCheckout),
                UntrackedCtx::AfterSwitching => advice_en!(UntrackedAfterSwitching),
                UntrackedCtx::AfterSwitchingBranches => advice_en!(UntrackedAfterSwitchingBranches),
                UntrackedCtx::AfterCherryPick => advice_en!(UntrackedAfterCherryPick),
                UntrackedCtx::AfterRevert => advice_en!(UntrackedAfterRevert),
                UntrackedCtx::Untouched => advice_en!(UntrackedUntouched),
            },
            (Ja, AdviceUntrackedRemain(ctx)) => plan::common::untracked_advice_ja(ctx),
            (En, AdviceSuggestStashPush) => advice_en!(SuggestStashPush),
            (Ja, AdviceSuggestStashPush) => plan::common::ADVICE_SUGGEST_STASH_PUSH,
            (En, AdviceNoForceUsed(PushPunct::EmDash)) => advice_en!(NoForceUsedEmDash),
            (En, AdviceNoForceUsed(PushPunct::Semicolon)) => advice_en!(NoForceUsedSemicolon),
            (Ja, AdviceNoForceUsed(_)) => plan::push::ADVICE_NO_FORCE_USED,
            (En, AdviceWillDetachHead) => advice_en!(WillDetachHead),
            (Ja, AdviceWillDetachHead) => plan::checkout::ADVICE_WILL_DETACH_HEAD,
            (En, AdviceRecommendCreateBranchHereFirst) => advice_en!(RecommendCreateBranchHereFirst),
            (Ja, AdviceRecommendCreateBranchHereFirst) => plan::checkout::ADVICE_RECOMMEND_CREATE_BRANCH_HERE_FIRST,
            (En, AdviceDirtyStashFirst) => advice_en!(DirtyStashFirst),
            (Ja, AdviceDirtyStashFirst) => plan::common::ADVICE_DIRTY_STASH_FIRST,
            (En, AdvicePullAutoStash) => advice_en!(PullAutoStash),
            (Ja, AdvicePullAutoStash) => plan::pull::ADVICE_AUTO_STASH,
            (En, AdviceDivergedSwitchOnly) => advice_en!(DivergedSwitchOnly),
            (Ja, AdviceDivergedSwitchOnly) => plan::switch::ADVICE_DIVERGED_SWITCH_ONLY,
            (En, AdviceDeleteUnmerged) => advice_en!(DeleteUnmerged),
            (Ja, AdviceDeleteUnmerged) => plan::branch::ADVICE_DELETE_UNMERGED,
            (En, AdviceUntrackedExcluded) => advice_en!(UntrackedExcluded),
            (Ja, AdviceUntrackedExcluded) => plan::stash::ADVICE_UNTRACKED_EXCLUDED,
            (En, AdviceCommonBranchInvalidRef) => advice_en!(CommonBranchInvalidRef),
            (Ja, AdviceCommonBranchInvalidRef) => plan::common::ADVICE_COMMON_BRANCH_INVALID_REF,
            (En, AdviceCommonConflictedFiles) => advice_en!(CommonConflictedFiles),
            (Ja, AdviceCommonConflictedFiles) => plan::common::ADVICE_COMMON_CONFLICTED_FILES,
            (En, AdviceCommonDirtyBlocksOp) => advice_en!(CommonDirtyBlocksOp),
            (Ja, AdviceCommonDirtyBlocksOp) => plan::common::ADVICE_COMMON_DIRTY_BLOCKS_OP,
            (En, AdviceCommonDirtyRollbackHint) => advice_en!(CommonDirtyRollbackHint),
            (Ja, AdviceCommonDirtyRollbackHint) => plan::common::ADVICE_COMMON_DIRTY_ROLLBACK_HINT,
            (En, AdviceCommonPartialCloneObjectMissing) => advice_en!(CommonPartialCloneObjectMissing),
            (Ja, AdviceCommonPartialCloneObjectMissing) => plan::common::ADVICE_COMMON_PARTIAL_CLONE_OBJECT_MISSING,
            (En, AdviceCommonSparseExcludedPath) => advice_en!(CommonSparseExcludedPath),
            (Ja, AdviceCommonSparseExcludedPath) => plan::common::ADVICE_COMMON_SPARSE_EXCLUDED_PATH,
            (En, AdviceCommonExternalFilter) => advice_en!(CommonExternalFilter),
            (Ja, AdviceCommonExternalFilter) => plan::common::ADVICE_COMMON_EXTERNAL_FILTER,
            (En, AdviceCommonExternalFilterSync) => advice_en!(CommonExternalFilterSync),
            (Ja, AdviceCommonExternalFilterSync) => plan::common::ADVICE_COMMON_EXTERNAL_FILTER_SYNC,
            (En, AdviceCommonSparseCheckoutUnsupported) => advice_en!(CommonSparseCheckoutUnsupported),
            (Ja, AdviceCommonSparseCheckoutUnsupported) => plan::common::ADVICE_COMMON_SPARSE_CHECKOUT_UNSUPPORTED,
            (En, AdviceCommonSubmoduleCheckoutUnsupported) => advice_en!(CommonSubmoduleCheckoutUnsupported),
            (Ja, AdviceCommonSubmoduleCheckoutUnsupported) => plan::common::ADVICE_COMMON_SUBMODULE_CHECKOUT_UNSUPPORTED,
            (En, AdviceCommonHunkChanged) => advice_en!(CommonHunkChanged),
            (Ja, AdviceCommonHunkChanged) => plan::common::ADVICE_COMMON_HUNK_CHANGED,
            (En, AdviceCommonMergeConflictWarning) => advice_en!(CommonMergeConflictWarning),
            (Ja, AdviceCommonMergeConflictWarning) => plan::common::ADVICE_COMMON_MERGE_CONFLICT_WARNING,
            (En, AdviceCommitNothingStaged) => advice_en!(CommitNothingStaged),
            (Ja, AdviceCommitNothingStaged) => plan::commit::ADVICE_COMMIT_NOTHING_STAGED,
            (En, AdviceCommitStagedContentChanged) => advice_en!(CommitStagedContentChanged),
            (Ja, AdviceCommitStagedContentChanged) => plan::commit::ADVICE_COMMIT_STAGED_CONTENT_CHANGED,
            (En, AdviceCommitIdentityUnavailable) => "Git author or committer identity is unavailable. Set user.name and user.email before committing.",
            (Ja, AdviceCommitIdentityUnavailable) => "Git の作者または committer の情報が設定されていません。commit する前に user.name と user.email を設定してください。",
            (En, AdviceGitIdentityUnavailable) => "This operation needs a Git identity. Set user.name and user.email before continuing.",
            (Ja, AdviceGitIdentityUnavailable) => "この操作には Git のユーザー情報が必要です。続行する前に user.name と user.email を設定してください。",
            (En, AdviceCommitConflictedFiles) => advice_en!(CommitConflictedFiles),
            (Ja, AdviceCommitConflictedFiles) => plan::commit::ADVICE_COMMIT_CONFLICTED_FILES,
            (En, AdviceCommitLeftoverNotIncluded) => advice_en!(CommitLeftoverNotIncluded),
            (Ja, AdviceCommitLeftoverNotIncluded) => plan::commit::ADVICE_COMMIT_LEFTOVER_NOT_INCLUDED,
            (En, AdviceDiscardTargetConflicted) => advice_en!(DiscardTargetConflicted),
            (Ja, AdviceDiscardTargetConflicted) => plan::discard::ADVICE_DISCARD_TARGET_CONFLICTED,
            (En, AdviceDiscardTargetSubmodule) => advice_en!(DiscardTargetSubmodule),
            (Ja, AdviceDiscardTargetSubmodule) => plan::discard::ADVICE_DISCARD_TARGET_SUBMODULE,
            (En, AdviceDiscardUntrackedWillBeDeleted) => advice_en!(DiscardUntrackedWillBeDeleted),
            (Ja, AdviceDiscardUntrackedWillBeDeleted) => plan::discard::ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED,
            (En, AdviceSnapshotSavepointFirst) => advice_en!(SnapshotSavepointFirst),
            (Ja, AdviceSnapshotSavepointFirst) => plan::snapshot::ADVICE_SNAPSHOT_SAVEPOINT_FIRST,
            (En, AdviceSnapshotRewritesWorkingTree) => advice_en!(SnapshotRewritesWorkingTree),
            (Ja, AdviceSnapshotRewritesWorkingTree) => plan::snapshot::ADVICE_SNAPSHOT_REWRITES_WORKING_TREE,
            (En, AdviceRulesetPatternViolation) => advice_en!(RulesetPatternViolation),
            (Ja, AdviceRulesetPatternViolation) => plan::ruleset::ADVICE_RULESET_PATTERN_VIOLATION,
            (En, AdviceRulesetPatternUncheckable) => advice_en!(RulesetPatternUncheckable),
            (Ja, AdviceRulesetPatternUncheckable) => plan::ruleset::ADVICE_RULESET_PATTERN_UNCHECKABLE,
            (En, AdviceRulesetFileTooLarge) => advice_en!(RulesetFileTooLarge),
            (Ja, AdviceRulesetFileTooLarge) => plan::ruleset::ADVICE_RULESET_FILE_TOO_LARGE,
            (En, AdviceRulesetRestrictedExtension) => advice_en!(RulesetRestrictedExtension),
            (Ja, AdviceRulesetRestrictedExtension) => plan::ruleset::ADVICE_RULESET_RESTRICTED_EXTENSION,
            (En, AdviceRulesetRestrictedPath) => advice_en!(RulesetRestrictedPath),
            (Ja, AdviceRulesetRestrictedPath) => plan::ruleset::ADVICE_RULESET_RESTRICTED_PATH,
            (En, AdviceRulesetPathTooLong) => advice_en!(RulesetPathTooLong),
            (Ja, AdviceRulesetPathTooLong) => plan::ruleset::ADVICE_RULESET_PATH_TOO_LONG,
            (En, AdviceRulesetSignatureRequired) => advice_en!(RulesetSignatureRequired),
            (Ja, AdviceRulesetSignatureRequired) => plan::ruleset::ADVICE_RULESET_SIGNATURE_REQUIRED,
            (En, AdviceRulesetLinearHistoryRequired) => advice_en!(RulesetLinearHistoryRequired),
            (Ja, AdviceRulesetLinearHistoryRequired) => plan::ruleset::ADVICE_RULESET_LINEAR_HISTORY_REQUIRED,
            (En, AdviceRulesetNonFastForward) => advice_en!(RulesetNonFastForward),
            (Ja, AdviceRulesetNonFastForward) => plan::ruleset::ADVICE_RULESET_NON_FAST_FORWARD,
            (En, AdviceRulesetCreationBlocked) => advice_en!(RulesetCreationBlocked),
            (Ja, AdviceRulesetCreationBlocked) => plan::ruleset::ADVICE_RULESET_CREATION_BLOCKED,
            (En, AdviceRulesetConstraintsUnknown) => advice_en!(RulesetConstraintsUnknown),
            (Ja, AdviceRulesetConstraintsUnknown) => plan::ruleset::ADVICE_RULESET_CONSTRAINTS_UNKNOWN,
            (En, AdviceChecklistPossibleSecretFileStaged) => advice_en!(ChecklistPossibleSecretFileStaged),
            (Ja, AdviceChecklistPossibleSecretFileStaged) => plan::checklist::ADVICE_CHECKLIST_POSSIBLE_SECRET_FILE_STAGED,
            (En, AdviceChecklistLargeBinaryStaged) => advice_en!(ChecklistLargeBinaryStaged),
            (Ja, AdviceChecklistLargeBinaryStaged) => plan::checklist::ADVICE_CHECKLIST_LARGE_BINARY_STAGED,
            (En, AdviceChecklistConflictMarkerFound) => advice_en!(ChecklistConflictMarkerFound),
            (Ja, AdviceChecklistConflictMarkerFound) => plan::checklist::ADVICE_CHECKLIST_CONFLICT_MARKER_FOUND,
            (En, AdviceChecklistPossibleSecretContentStaged) => advice_en!(ChecklistPossibleSecretContentStaged),
            (Ja, AdviceChecklistPossibleSecretContentStaged) => plan::checklist::ADVICE_CHECKLIST_POSSIBLE_SECRET_CONTENT_STAGED,
            (En, AdviceMergeWillConflict) => advice_en!(MergeWillConflict),
            (Ja, AdviceMergeWillConflict) => plan::merge::ADVICE_MERGE_WILL_CONFLICT,
            (En, AdviceMergeUnrelatedHistories) => advice_en!(MergeUnrelatedHistories),
            (Ja, AdviceMergeUnrelatedHistories) => plan::merge::ADVICE_MERGE_UNRELATED_HISTORIES,
            (En, AdviceMergeOperationInProgress) => advice_en!(MergeOperationInProgress),
            (Ja, AdviceMergeOperationInProgress) => plan::merge::ADVICE_MERGE_OPERATION_IN_PROGRESS,
            (En, AdviceMergeUntrackedWouldBeOverwritten) => advice_en!(MergeUntrackedWouldBeOverwritten),
            (Ja, AdviceMergeUntrackedWouldBeOverwritten) => plan::merge::ADVICE_MERGE_UNTRACKED_WOULD_BE_OVERWRITTEN,
            (En, AdviceMergeIntoUnrelatedHistories) => advice_en!(MergeIntoUnrelatedHistories),
            (Ja, AdviceMergeIntoUnrelatedHistories) => plan::merge::ADVICE_MERGE_INTO_UNRELATED_HISTORIES,
            (En, AdviceMergeIntoCheckedOutElsewhere) => advice_en!(MergeIntoCheckedOutElsewhere),
            (Ja, AdviceMergeIntoCheckedOutElsewhere) => plan::merge::ADVICE_MERGE_INTO_CHECKED_OUT_ELSEWHERE,
            (En, AdviceMergeIntoWouldConflict) => advice_en!(MergeIntoWouldConflict),
            (Ja, AdviceMergeIntoWouldConflict) => plan::merge::ADVICE_MERGE_INTO_WOULD_CONFLICT,
            (En, AdviceMergeIntoFastForward) => advice_en!(MergeIntoFastForward),
            (Ja, AdviceMergeIntoFastForward) => plan::merge::ADVICE_MERGE_INTO_FAST_FORWARD,
            (En, AdviceMergeIntoWorkingTreeUntouched) => advice_en!(MergeIntoWorkingTreeUntouched),
            (Ja, AdviceMergeIntoWorkingTreeUntouched) => plan::merge::ADVICE_MERGE_INTO_WORKING_TREE_UNTOUCHED,
            (En, AdviceMergeIntoCreatesLocalBranch) => advice_en!(MergeIntoCreatesLocalBranch),
            (Ja, AdviceMergeIntoCreatesLocalBranch) => plan::merge::ADVICE_MERGE_INTO_CREATES_LOCAL_BRANCH,
            (En, AdviceMergeIntoRemoteSource) => advice_en!(MergeIntoRemoteSource),
            (Ja, AdviceMergeIntoRemoteSource) => plan::merge::ADVICE_MERGE_INTO_REMOTE_SOURCE,
            (En, AdviceMergeIntoLocalDiffersFromRemote) => advice_en!(MergeIntoLocalDiffersFromRemote),
            (Ja, AdviceMergeIntoLocalDiffersFromRemote) => plan::merge::ADVICE_MERGE_INTO_LOCAL_DIFFERS_FROM_REMOTE,
            (En, AdviceRebaseDirtyWorkingTree) => advice_en!(RebaseDirtyWorkingTree),
            (Ja, AdviceRebaseDirtyWorkingTree) => plan::rebase::ADVICE_REBASE_DIRTY_WORKING_TREE,
            (En, AdviceRebaseMayConflict) => advice_en!(RebaseMayConflict),
            (Ja, AdviceRebaseMayConflict) => plan::rebase::ADVICE_REBASE_MAY_CONFLICT,
            (En, AdviceCherryRevertWouldConflictCherryPick) => advice_en!(CherryRevertWouldConflictCherryPick),
            (Ja, AdviceCherryRevertWouldConflictCherryPick) => plan::cherry_revert::ADVICE_CHERRY_REVERT_WOULD_CONFLICT_CHERRY_PICK,
            (En, AdviceCherryRevertWouldConflictRevert) => advice_en!(CherryRevertWouldConflictRevert),
            (Ja, AdviceCherryRevertWouldConflictRevert) => plan::cherry_revert::ADVICE_CHERRY_REVERT_WOULD_CONFLICT_REVERT,
            (En, AdviceCherryRevertDirtyMayRefuse) => advice_en!(CherryRevertDirtyMayRefuse),
            (Ja, AdviceCherryRevertDirtyMayRefuse) => plan::cherry_revert::ADVICE_CHERRY_REVERT_DIRTY_MAY_REFUSE,
            (En, AdviceResetRefOnlySoftReset) => advice_en!(ResetRefOnlySoftReset),
            (Ja, AdviceResetRefOnlySoftReset) => plan::reset::ADVICE_RESET_REF_ONLY_SOFT_RESET,
            (En, AdviceResetAbandonsCommits) => advice_en!(ResetAbandonsCommits),
            (Ja, AdviceResetAbandonsCommits) => plan::reset::ADVICE_RESET_ABANDONS_COMMITS,
            (En, AdviceResetTargetNotAncestor) => advice_en!(ResetTargetNotAncestor),
            (Ja, AdviceResetTargetNotAncestor) => plan::reset::ADVICE_RESET_TARGET_NOT_ANCESTOR,
            (En, AdviceHistoryPushedHistoryRewriteUndo) => advice_en!(HistoryPushedHistoryRewriteUndo),
            (Ja, AdviceHistoryPushedHistoryRewriteUndo) => plan::history::ADVICE_HISTORY_PUSHED_HISTORY_REWRITE_UNDO,
            (En, AdviceHistoryPushedHistoryRewriteAmend) => advice_en!(HistoryPushedHistoryRewriteAmend),
            (Ja, AdviceHistoryPushedHistoryRewriteAmend) => plan::history::ADVICE_HISTORY_PUSHED_HISTORY_REWRITE_AMEND,
            (En, AdviceHistoryAmendDivergesFromRemote) => advice_en!(HistoryAmendDivergesFromRemote),
            (Ja, AdviceHistoryAmendDivergesFromRemote) => plan::history::ADVICE_HISTORY_AMEND_DIVERGES_FROM_REMOTE,
            (En, AdviceHistoryNothingStagedForAmend) => advice_en!(HistoryNothingStagedForAmend),
            (Ja, AdviceHistoryNothingStagedForAmend) => plan::history::ADVICE_HISTORY_NOTHING_STAGED_FOR_AMEND,
            (En, AdviceHistoryWrongBranch) => advice_en!(HistoryWrongBranch),
            (Ja, AdviceHistoryWrongBranch) => plan::history::ADVICE_HISTORY_WRONG_BRANCH,
            (En, AdviceHistoryHeadNotOnBranch) => advice_en!(HistoryHeadNotOnBranch),
            (Ja, AdviceHistoryHeadNotOnBranch) => plan::history::ADVICE_HISTORY_HEAD_NOT_ON_BRANCH,
            (En, AdviceHistoryEntryStaleBranchMoved) => advice_en!(HistoryEntryStaleBranchMoved),
            (Ja, AdviceHistoryEntryStaleBranchMoved) => plan::history::ADVICE_HISTORY_ENTRY_STALE_BRANCH_MOVED,
            (En, AdviceHistoryEntryStaleUnreachable) => advice_en!(HistoryEntryStaleUnreachable),
            (Ja, AdviceHistoryEntryStaleUnreachable) => plan::history::ADVICE_HISTORY_ENTRY_STALE_UNREACHABLE,
            (En, AdviceHistorySoftMovePreservesChanges) => advice_en!(HistorySoftMovePreservesChanges),
            (Ja, AdviceHistorySoftMovePreservesChanges) => plan::history::ADVICE_HISTORY_SOFT_MOVE_PRESERVES_CHANGES,
            (En, AdviceConflictsObservationChanged) => advice_en!(ConflictsObservationChanged),
            (Ja, AdviceConflictsObservationChanged) => plan::conflicts::ADVICE_CONFLICTS_OBSERVATION_CHANGED,
            (En, AdviceConflictsSkipStepChanged) => "The operation moved to another step; review the current state",
            (Ja, AdviceConflictsSkipStepChanged) => "別の step に進んでいます。最新の状態を確認してください",
            (En, AdviceConflictsPlanChanged) => advice_en!(ConflictsPlanChanged),
            (Ja, AdviceConflictsPlanChanged) => plan::conflicts::ADVICE_CONFLICTS_PLAN_CHANGED,
            (En, AdviceConflictsRepositoryIdentityChanged) => advice_en!(ConflictsRepositoryIdentityChanged),
            (Ja, AdviceConflictsRepositoryIdentityChanged) => plan::conflicts::ADVICE_CONFLICTS_REPOSITORY_IDENTITY_CHANGED,
            (En, AdviceConflictsConflictGone) => advice_en!(ConflictsConflictGone),
            (Ja, AdviceConflictsConflictGone) => plan::conflicts::ADVICE_CONFLICTS_CONFLICT_GONE,
            (En, AdviceConflictsAbortStartUnavailable) => advice_en!(ConflictsAbortStartUnavailable),
            (Ja, AdviceConflictsAbortStartUnavailable) => plan::conflicts::ADVICE_CONFLICTS_ABORT_START_UNAVAILABLE,
            (En, AdviceConflictsAbortHeadMoved) => advice_en!(ConflictsAbortHeadMoved),
            (Ja, AdviceConflictsAbortHeadMoved) => plan::conflicts::ADVICE_CONFLICTS_ABORT_HEAD_MOVED,
            (En, AdviceConflictsDraftPreservationFailed) => advice_en!(ConflictsDraftPreservationFailed),
            (Ja, AdviceConflictsDraftPreservationFailed) => plan::conflicts::ADVICE_CONFLICTS_DRAFT_PRESERVATION_FAILED,
            (En, AdviceConflictsResolutionMarkers) => advice_en!(ConflictsResolutionMarkers),
            (Ja, AdviceConflictsResolutionMarkers) => plan::conflicts::ADVICE_CONFLICTS_RESOLUTION_MARKERS,
            (En, AdviceConflictsUnresolvedFiles) => advice_en!(ConflictsUnresolvedFiles),
            (Ja, AdviceConflictsUnresolvedFiles) => plan::conflicts::ADVICE_CONFLICTS_UNRESOLVED_FILES,
            (En, AdviceConflictsMarkerResidue) => advice_en!(ConflictsMarkerResidue),
            (Ja, AdviceConflictsMarkerResidue) => plan::conflicts::ADVICE_CONFLICTS_MARKER_RESIDUE,
            (En, AdviceConflictsIndexUnmerged) => advice_en!(ConflictsIndexUnmerged),
            (Ja, AdviceConflictsIndexUnmerged) => plan::conflicts::ADVICE_CONFLICTS_INDEX_UNMERGED,
            (En, AdviceConflictsBinaryUnresolved) => advice_en!(ConflictsBinaryUnresolved),
            (Ja, AdviceConflictsBinaryUnresolved) => plan::conflicts::ADVICE_CONFLICTS_BINARY_UNRESOLVED,
            (En, AdviceConflictsDeletionUndecided) => advice_en!(ConflictsDeletionUndecided),
            (Ja, AdviceConflictsDeletionUndecided) => plan::conflicts::ADVICE_CONFLICTS_DELETION_UNDECIDED,
            (En, AdviceConflictsEmptyMergeMessage) => advice_en!(ConflictsEmptyMergeMessage),
            (Ja, AdviceConflictsEmptyMergeMessage) => plan::conflicts::ADVICE_CONFLICTS_EMPTY_MERGE_MESSAGE,
            (En, AdviceConflictsNoConflictingFilesDetected) => advice_en!(ConflictsNoConflictingFilesDetected),
            (Ja, AdviceConflictsNoConflictingFilesDetected) => plan::conflicts::ADVICE_CONFLICTS_NO_CONFLICTING_FILES_DETECTED,
            (En, AdviceConflictsPartialResolutionsPreserved) => advice_en!(ConflictsPartialResolutionsPreserved),
            (Ja, AdviceConflictsPartialResolutionsPreserved) => plan::conflicts::ADVICE_CONFLICTS_PARTIAL_RESOLUTIONS_PRESERVED,
            (En, AdviceConflictsSkipDiscardsStep) => advice_en!(ConflictsSkipDiscardsStep),
            (Ja, AdviceConflictsSkipDiscardsStep) => plan::conflicts::ADVICE_CONFLICTS_SKIP_DISCARDS_STEP,
            (En, AdviceBranchRenameRefOnlyDirty) => advice_en!(BranchRenameRefOnlyDirty),
            (Ja, AdviceBranchRenameRefOnlyDirty) => plan::branch::ADVICE_BRANCH_RENAME_REF_ONLY_DIRTY,
            (En, AdviceBranchRenameRemoteNotRenamed) => advice_en!(BranchRenameRemoteNotRenamed),
            (Ja, AdviceBranchRenameRemoteNotRenamed) => plan::branch::ADVICE_BRANCH_RENAME_REMOTE_NOT_RENAMED,
            (En, AdviceBranchRenameConfig) => advice_en!(BranchRenameConfig),
            (Ja, AdviceBranchRenameConfig) => plan::branch::ADVICE_BRANCH_RENAME_CONFIG,
            (En, AdviceBranchRenameConfigChanged) => advice_en!(BranchRenameConfigChanged),
            (Ja, AdviceBranchRenameConfigChanged) => plan::branch::ADVICE_BRANCH_RENAME_CONFIG_CHANGED,
            (En, AdviceBranchRenameConfigExternal) => advice_en!(BranchRenameConfigExternal),
            (Ja, AdviceBranchRenameConfigExternal) => plan::branch::ADVICE_BRANCH_RENAME_CONFIG_EXTERNAL,
            (En, AdviceBranchRenameConfigConditional) => advice_en!(BranchRenameConfigConditional),
            (Ja, AdviceBranchRenameConfigConditional) => plan::branch::ADVICE_BRANCH_RENAME_CONFIG_CONDITIONAL,
            (En, AdviceBranchRenameConfigHeader) => advice_en!(BranchRenameConfigHeader),
            (Ja, AdviceBranchRenameConfigHeader) => plan::branch::ADVICE_BRANCH_RENAME_CONFIG_HEADER,
            (En, AdviceBranchDeleteCurrentBranch) => advice_en!(BranchDeleteCurrentBranch),
            (Ja, AdviceBranchDeleteCurrentBranch) => plan::branch::ADVICE_BRANCH_DELETE_CURRENT_BRANCH,
            (En, AdviceBranchDeleteReachabilityChanged) => advice_en!(BranchDeleteReachabilityChanged),
            (Ja, AdviceBranchDeleteReachabilityChanged) => plan::branch::ADVICE_BRANCH_DELETE_REACHABILITY_CHANGED,
            (En, AdviceBranchDeleteBranchCheckedOut) => advice_en!(BranchDeleteBranchCheckedOut),
            (Ja, AdviceBranchDeleteBranchCheckedOut) => plan::branch::ADVICE_BRANCH_DELETE_BRANCH_CHECKED_OUT,
            (En, AdviceBranchDeleteBranchInLockedWorktree) => advice_en!(BranchDeleteBranchInLockedWorktree),
            (Ja, AdviceBranchDeleteBranchInLockedWorktree) => plan::branch::ADVICE_BRANCH_DELETE_BRANCH_IN_LOCKED_WORKTREE,
            (En, AdviceBranchDeleteBranchInDirtyWorktree) => advice_en!(BranchDeleteBranchInDirtyWorktree),
            (Ja, AdviceBranchDeleteBranchInDirtyWorktree) => plan::branch::ADVICE_BRANCH_DELETE_BRANCH_IN_DIRTY_WORKTREE,
            (En, AdviceBranchDeleteRemovesPinningWorktree) => advice_en!(BranchDeleteRemovesPinningWorktree),
            (Ja, AdviceBranchDeleteRemovesPinningWorktree) => plan::branch::ADVICE_BRANCH_DELETE_REMOVES_PINNING_WORKTREE,
            (En, AdviceBranchDeleteSquashMerged) => advice_en!(BranchDeleteSquashMerged),
            (Ja, AdviceBranchDeleteSquashMerged) => plan::branch::ADVICE_BRANCH_DELETE_SQUASH_MERGED,
            (En, AdviceBranchDeleteKeepsRemote) => advice_en!(BranchDeleteKeepsRemote),
            (Ja, AdviceBranchDeleteKeepsRemote) => plan::branch::ADVICE_BRANCH_DELETE_KEEPS_REMOTE,
            (En, AdviceCheckoutCheckoutOverlap) => advice_en!(CheckoutCheckoutOverlap),
            (Ja, AdviceCheckoutCheckoutOverlap) => plan::checkout::ADVICE_CHECKOUT_CHECKOUT_OVERLAP,
            (En, AdviceCheckoutDirtyMayFail) => advice_en!(CheckoutDirtyMayFail),
            (Ja, AdviceCheckoutDirtyMayFail) => plan::checkout::ADVICE_CHECKOUT_DIRTY_MAY_FAIL,
            (En, AdviceSwitchWillCreateTracking) => advice_en!(SwitchWillCreateTracking),
            (Ja, AdviceSwitchWillCreateTracking) => plan::switch::ADVICE_SWITCH_WILL_CREATE_TRACKING,
            (En, AdviceSwitchFfLocalKnowledge) => advice_en!(SwitchFfLocalKnowledge),
            (Ja, AdviceSwitchFfLocalKnowledge) => plan::switch::ADVICE_SWITCH_FF_LOCAL_KNOWLEDGE,
            (En, AdviceSwitchAheadSwitchOnly) => advice_en!(SwitchAheadSwitchOnly),
            (Ja, AdviceSwitchAheadSwitchOnly) => plan::switch::ADVICE_SWITCH_AHEAD_SWITCH_ONLY,
            (En, AdviceWorktreeDirtyBlocksCheckoutAfterCreate) => advice_en!(WorktreeDirtyBlocksCheckoutAfterCreate),
            (Ja, AdviceWorktreeDirtyBlocksCheckoutAfterCreate) => plan::worktree::ADVICE_WORKTREE_DIRTY_BLOCKS_CHECKOUT_AFTER_CREATE,
            (En, AdviceWorktreeCreatesLinkedWorktree) => advice_en!(WorktreeCreatesLinkedWorktree),
            (Ja, AdviceWorktreeCreatesLinkedWorktree) => plan::worktree::ADVICE_WORKTREE_CREATES_LINKED_WORKTREE,
            (En, AdviceWorktreeLockedWithReason) => advice_en!(WorktreeLockedWithReason),
            (Ja, AdviceWorktreeLockedWithReason) => plan::worktree::ADVICE_WORKTREE_LOCKED_WITH_REASON,
            (En, AdviceWorktreeIncludeCopy) => advice_en!(WorktreeIncludeCopy),
            (Ja, AdviceWorktreeIncludeCopy) => plan::worktree::ADVICE_WORKTREE_INCLUDE_COPY,
            (En, AdviceWorktreeIncludeSkippedSymlinks) => advice_en!(WorktreeIncludeSkippedSymlinks),
            (Ja, AdviceWorktreeIncludeSkippedSymlinks) => plan::worktree::ADVICE_WORKTREE_INCLUDE_SKIPPED_SYMLINKS,
            (En, AdviceWorktreeIncludeOverCap) => advice_en!(WorktreeIncludeOverCap),
            (Ja, AdviceWorktreeIncludeOverCap) => plan::worktree::ADVICE_WORKTREE_INCLUDE_OVER_CAP,
            (En, AdviceWorktreeRemoveDirty) => advice_en!(WorktreeRemoveDirty),
            (Ja, AdviceWorktreeRemoveDirty) => plan::worktree::ADVICE_WORKTREE_REMOVE_DIRTY,
            (En, AdviceWorktreeRemoveLocked) => advice_en!(WorktreeRemoveLocked),
            (Ja, AdviceWorktreeRemoveLocked) => plan::worktree::ADVICE_WORKTREE_REMOVE_LOCKED,
            (En, AdviceWorktreeRemoveLiveShell) => advice_en!(WorktreeRemoveLiveShell),
            (Ja, AdviceWorktreeRemoveLiveShell) => plan::worktree::ADVICE_WORKTREE_REMOVE_LIVE_SHELL,
            (En, AdviceWorktreeRemoveLeftoverProcesses) => advice_en!(WorktreeRemoveLeftoverProcesses),
            (Ja, AdviceWorktreeRemoveLeftoverProcesses) => plan::worktree::ADVICE_WORKTREE_REMOVE_LEFTOVER_PROCESSES,
            (En, AdviceWorktreeRemoveIgnoredFiles) => advice_en!(WorktreeRemoveIgnoredFiles),
            (Ja, AdviceWorktreeRemoveIgnoredFiles) => plan::worktree::ADVICE_WORKTREE_REMOVE_IGNORED_FILES,
            (En, AdviceWorktreeRemoveIgnoredFilesAndFolders) => advice_en!(WorktreeRemoveIgnoredFilesAndFolders),
            (Ja, AdviceWorktreeRemoveIgnoredFilesAndFolders) => plan::worktree::ADVICE_WORKTREE_REMOVE_IGNORED_FILES_AND_FOLDERS,
            (En, AdviceWorktreeRemovesWorktreeDeleteBranch) => advice_en!(WorktreeRemovesWorktreeDeleteBranch),
            (Ja, AdviceWorktreeRemovesWorktreeDeleteBranch) => plan::worktree::ADVICE_WORKTREE_REMOVES_WORKTREE_DELETE_BRANCH,
            (En, AdviceWorktreeRemovesWorktreeKeepBranch) => advice_en!(WorktreeRemovesWorktreeKeepBranch),
            (Ja, AdviceWorktreeRemovesWorktreeKeepBranch) => plan::worktree::ADVICE_WORKTREE_REMOVES_WORKTREE_KEEP_BRANCH,
            (En, AdviceWorktreeLocksWorktree) => advice_en!(WorktreeLocksWorktree),
            (Ja, AdviceWorktreeLocksWorktree) => plan::worktree::ADVICE_WORKTREE_LOCKS_WORKTREE,
            (En, AdviceWorktreePrunePreview) => advice_en!(WorktreePrunePreview),
            (Ja, AdviceWorktreePrunePreview) => plan::worktree::ADVICE_WORKTREE_PRUNE_PREVIEW,
            (En, AdviceWorktreeRepairsWorktrees) => advice_en!(WorktreeRepairsWorktrees),
            (Ja, AdviceWorktreeRepairsWorktrees) => plan::worktree::ADVICE_WORKTREE_REPAIRS_WORKTREES,
            (En, AdviceWorktreePostCreateSteps) => advice_en!(WorktreePostCreateSteps),
            (Ja, AdviceWorktreePostCreateSteps) => plan::worktree::ADVICE_WORKTREE_POST_CREATE_STEPS,
            (En, AdviceWorktreePostCreateStepsTrustRequired) => advice_en!(WorktreePostCreateStepsTrustRequired),
            (Ja, AdviceWorktreePostCreateStepsTrustRequired) => plan::worktree::ADVICE_WORKTREE_POST_CREATE_STEPS_TRUST_REQUIRED,
            (En, AdviceWorktreePreRemoveSteps) => advice_en!(WorktreePreRemoveSteps),
            (Ja, AdviceWorktreePreRemoveSteps) => plan::worktree::ADVICE_WORKTREE_PRE_REMOVE_STEPS,
            (En, AdviceWorktreePreRemoveStepsTrustRequired) => advice_en!(WorktreePreRemoveStepsTrustRequired),
            (Ja, AdviceWorktreePreRemoveStepsTrustRequired) => plan::worktree::ADVICE_WORKTREE_PRE_REMOVE_STEPS_TRUST_REQUIRED,
            (En, AdviceTagNameErrorEmpty) => advice_en!(TagNameErrorEmpty),
            (Ja, AdviceTagNameErrorEmpty) => plan::tag::ADVICE_TAG_NAME_ERROR_EMPTY,
            (En, AdviceTagNameErrorLeadingDash) => advice_en!(TagNameErrorLeadingDash),
            (Ja, AdviceTagNameErrorLeadingDash) => plan::tag::ADVICE_TAG_NAME_ERROR_LEADING_DASH,
            (En, AdviceTagPushRemoteSideEffect) => advice_en!(TagPushRemoteSideEffect),
            (Ja, AdviceTagPushRemoteSideEffect) => plan::tag::ADVICE_TAG_PUSH_REMOTE_SIDE_EFFECT,
            (En, AdviceTagPushRejectedIfMoved) => advice_en!(TagPushRejectedIfMoved),
            (Ja, AdviceTagPushRejectedIfMoved) => plan::tag::ADVICE_TAG_PUSH_REJECTED_IF_MOVED,
            (En, AdviceCleanupNoLongerCandidate) => advice_en!(CleanupNoLongerCandidate),
            (Ja, AdviceCleanupNoLongerCandidate) => plan::cleanup::ADVICE_CLEANUP_NO_LONGER_CANDIDATE,
            (En, AdviceCleanupNotSafelyDeletable) => advice_en!(CleanupNotSafelyDeletable),
            (Ja, AdviceCleanupNotSafelyDeletable) => plan::cleanup::ADVICE_CLEANUP_NOT_SAFELY_DELETABLE,
            (En, AdviceCleanupTipMoved) => advice_en!(CleanupTipMoved),
            (Ja, AdviceCleanupTipMoved) => plan::cleanup::ADVICE_CLEANUP_TIP_MOVED,
            (En, AdviceCleanupSquashHeuristicOnly) => advice_en!(CleanupSquashHeuristicOnly),
            (Ja, AdviceCleanupSquashHeuristicOnly) => plan::cleanup::ADVICE_CLEANUP_SQUASH_HEURISTIC_ONLY,
            (En, AdviceCleanupRemoteDeleteNetwork) => advice_en!(CleanupRemoteDeleteNetwork),
            (Ja, AdviceCleanupRemoteDeleteNetwork) => plan::cleanup::ADVICE_CLEANUP_REMOTE_DELETE_NETWORK,
            (En, AdvicePushNoUpstreamNoRemotes) => advice_en!(PushNoUpstreamNoRemotes),
            (Ja, AdvicePushNoUpstreamNoRemotes) => plan::push::ADVICE_PUSH_NO_UPSTREAM_NO_REMOTES,
            (En, AdvicePushUpstreamFormatInvalid) => advice_en!(PushUpstreamFormatInvalid),
            (Ja, AdvicePushUpstreamFormatInvalid) => plan::push::ADVICE_PUSH_UPSTREAM_FORMAT_INVALID,
            (En, AdvicePushUpstreamNotPresentLocally) => advice_en!(PushUpstreamNotPresentLocally),
            (Ja, AdvicePushUpstreamNotPresentLocally) => plan::push::ADVICE_PUSH_UPSTREAM_NOT_PRESENT_LOCALLY,
            (En, AdvicePullNoUpstreamWithHint) => advice_en!(PullNoUpstreamWithHint),
            (Ja, AdvicePullNoUpstreamWithHint) => plan::pull::ADVICE_PULL_NO_UPSTREAM_WITH_HINT,
            (En, AdvicePullStrictFfLocalChangesOverlap) => advice_en!(PullStrictFfLocalChangesOverlap),
            (Ja, AdvicePullStrictFfLocalChangesOverlap) => plan::pull::ADVICE_PULL_STRICT_FF_LOCAL_CHANGES_OVERLAP,
            (En, AdvicePullMergePrediction) => advice_en!(PullMergePrediction),
            (Ja, AdvicePullMergePrediction) => plan::pull::ADVICE_PULL_MERGE_PREDICTION,
            (En, AdvicePullRestoreConflict) => advice_en!(PullRestoreConflict),
            (Ja, AdvicePullRestoreConflict) => plan::pull::ADVICE_PULL_RESTORE_CONFLICT,
            (En, AdvicePullRestoreConflictPossible) => advice_en!(PullRestoreConflictPossible),
            (Ja, AdvicePullRestoreConflictPossible) => plan::pull::ADVICE_PULL_RESTORE_CONFLICT_POSSIBLE,
            (En, AdvicePullRestoreConflictMore) => advice_en!(PullRestoreConflictMore),
            (Ja, AdvicePullRestoreConflictMore) => plan::pull::ADVICE_PULL_RESTORE_CONFLICT_MORE,
            (En, AdvicePullCannotFastForward) => advice_en!(PullCannotFastForward),
            (Ja, AdvicePullCannotFastForward) => plan::pull::ADVICE_PULL_CANNOT_FAST_FORWARD,
            (En, AdvicePullRemoteDiverged) => advice_en!(PullRemoteDiverged),
            (Ja, AdvicePullRemoteDiverged) => plan::pull::ADVICE_PULL_REMOTE_DIVERGED,
            (En, AdvicePullRemoteDirty) => advice_en!(PullRemoteDirty),
            (Ja, AdvicePullRemoteDirty) => plan::pull::ADVICE_PULL_REMOTE_DIRTY,
            (En, AdvicePullRemotePreviewStale) => advice_en!(PullRemotePreviewStale),
            (Ja, AdvicePullRemotePreviewStale) => plan::pull::ADVICE_PULL_REMOTE_PREVIEW_STALE,
            (En, AdviceStashHeadRequired) => advice_en!(StashHeadRequired),
            (Ja, AdviceStashHeadRequired) => plan::stash::ADVICE_STASH_HEAD_REQUIRED,
            (En, AdviceStashUntrackedIncluded) => advice_en!(StashUntrackedIncluded),
            (Ja, AdviceStashUntrackedIncluded) => plan::stash::ADVICE_STASH_UNTRACKED_INCLUDED,
            (En, AdviceStashDirtyBlocksApply) => advice_en!(StashDirtyBlocksApply),
            (Ja, AdviceStashDirtyBlocksApply) => plan::stash::ADVICE_STASH_DIRTY_BLOCKS_APPLY,
            (En, AdviceStashConflictUnknownFiles) => advice_en!(StashConflictUnknownFiles),
            (Ja, AdviceStashConflictUnknownFiles) => plan::stash::ADVICE_STASH_CONFLICT_UNKNOWN_FILES,
            (En, AdviceStashPopWouldConflict) => advice_en!(StashPopWouldConflict),
            (Ja, AdviceStashPopWouldConflict) => plan::stash::ADVICE_STASH_POP_WOULD_CONFLICT,
            (En, AdviceStashApplyWouldConflict) => advice_en!(StashApplyWouldConflict),
            (Ja, AdviceStashApplyWouldConflict) => plan::stash::ADVICE_STASH_APPLY_WOULD_CONFLICT,
            (En, AdviceStashApplyPredictionUnavailable) => advice_en!(StashApplyPredictionUnavailable),
            (Ja, AdviceStashApplyPredictionUnavailable) => plan::stash::ADVICE_STASH_APPLY_PREDICTION_UNAVAILABLE,
            (En, AdviceStashPopPredictionUnavailable) => advice_en!(StashPopPredictionUnavailable),
            (Ja, AdviceStashPopPredictionUnavailable) => plan::stash::ADVICE_STASH_POP_PREDICTION_UNAVAILABLE,
            (En, AdviceStashRemoteDropIrreversible) => advice_en!(StashRemoteDropIrreversible),
            (Ja, AdviceStashRemoteDropIrreversible) => plan::stash::ADVICE_STASH_REMOTE_DROP_IRREVERSIBLE,
            (En, AdviceStashTargetChanged) => advice_en!(StashTargetChanged),
            (Ja, AdviceStashTargetChanged) => plan::stash::ADVICE_STASH_TARGET_CHANGED,
            (En, AdviceStashListChanged) => advice_en!(StashListChanged),
            (Ja, AdviceStashListChanged) => plan::stash::ADVICE_STASH_LIST_CHANGED,
            (En, AdviceRemoteBranchLocalBranchUntouched) => advice_en!(RemoteBranchLocalBranchUntouched),
            (Ja, AdviceRemoteBranchLocalBranchUntouched) => plan::remote_branch::ADVICE_REMOTE_BRANCH_LOCAL_BRANCH_UNTOUCHED,
            (En, AdviceForceLeaseNoUpstream) => advice_en!(ForceLeaseNoUpstream),
            (Ja, AdviceForceLeaseNoUpstream) => plan::force_lease::ADVICE_FORCE_LEASE_NO_UPSTREAM,
            (En, AdviceForceLeaseRewritesRemoteHistory) => advice_en!(ForceLeaseRewritesRemoteHistory),
            (Ja, AdviceForceLeaseRewritesRemoteHistory) => plan::force_lease::ADVICE_FORCE_LEASE_REWRITES_REMOTE_HISTORY,
            (En, AdviceForceLeaseLeaseValue) => advice_en!(ForceLeaseLeaseValue),
            (Ja, AdviceForceLeaseLeaseValue) => plan::force_lease::ADVICE_FORCE_LEASE_LEASE_VALUE,
            (En, AdviceGithubHeadUnavailable) => advice_en!(GithubHeadUnavailable),
            (Ja, AdviceGithubHeadUnavailable) => plan::github::ADVICE_GITHUB_HEAD_UNAVAILABLE,
            (En, AdviceGithubNotMergeable) => advice_en!(GithubNotMergeable),
            (Ja, AdviceGithubNotMergeable) => plan::github::ADVICE_GITHUB_NOT_MERGEABLE,
            (En, AdviceGithubIsDraft) => advice_en!(GithubIsDraft),
            (Ja, AdviceGithubIsDraft) => plan::github::ADVICE_GITHUB_IS_DRAFT,
            (En, AdviceGithubChecksFailing) => advice_en!(GithubChecksFailing),
            (Ja, AdviceGithubChecksFailing) => plan::github::ADVICE_GITHUB_CHECKS_FAILING,
            (En, AdviceGithubChecksPending) => advice_en!(GithubChecksPending),
            (Ja, AdviceGithubChecksPending) => plan::github::ADVICE_GITHUB_CHECKS_PENDING,
            (En, AdviceGithubChangesRequested) => advice_en!(GithubChangesRequested),
            (Ja, AdviceGithubChangesRequested) => plan::github::ADVICE_GITHUB_CHANGES_REQUESTED,
            (En, AdviceGithubRemoteSideEffect) => advice_en!(GithubRemoteSideEffect),
            (Ja, AdviceGithubRemoteSideEffect) => plan::github::ADVICE_GITHUB_REMOTE_SIDE_EFFECT,
            (En, AdviceGithubDeletesBranch) => advice_en!(GithubDeletesBranch),
            (Ja, AdviceGithubDeletesBranch) => plan::github::ADVICE_GITHUB_DELETES_BRANCH,
            (En, AdviceGithubForkKeepsRemoteBranch) => advice_en!(GithubForkKeepsRemoteBranch),
            (Ja, AdviceGithubForkKeepsRemoteBranch) => plan::github::ADVICE_GITHUB_FORK_KEEPS_REMOTE_BRANCH,
            (En, AdviceGithubDeletesLocalBranchPresent) => advice_en!(GithubDeletesLocalBranchPresent),
            (Ja, AdviceGithubDeletesLocalBranchPresent) => plan::github::ADVICE_GITHUB_DELETES_LOCAL_BRANCH_PRESENT,
            (En, AdviceGithubDeletesLocalBranchAbsent) => advice_en!(GithubDeletesLocalBranchAbsent),
            (Ja, AdviceGithubDeletesLocalBranchAbsent) => plan::github::ADVICE_GITHUB_DELETES_LOCAL_BRANCH_ABSENT,
            (En, AdviceGithubKeepsLocalBranch) => advice_en!(GithubKeepsLocalBranch),
            (Ja, AdviceGithubKeepsLocalBranch) => plan::github::ADVICE_GITHUB_KEEPS_LOCAL_BRANCH,
            (En, AdviceGithubLocalBranchKept) => advice_en!(GithubLocalBranchKept),
            (Ja, AdviceGithubLocalBranchKept) => plan::github::ADVICE_GITHUB_LOCAL_BRANCH_KEPT,
            (En, AdviceGithubLocalBranchNotDeleted) => advice_en!(GithubLocalBranchNotDeleted),
            (Ja, AdviceGithubLocalBranchNotDeleted) => plan::github::ADVICE_GITHUB_LOCAL_BRANCH_NOT_DELETED,
            (En, AdviceGithubSuggestionRangeGone) => advice_en!(GithubSuggestionRangeGone),
            (Ja, AdviceGithubSuggestionRangeGone) => plan::github::ADVICE_GITHUB_SUGGESTION_RANGE_GONE,
            (En, AdviceGithubSuggestionStale) => advice_en!(GithubSuggestionStale),
            (Ja, AdviceGithubSuggestionStale) => plan::github::ADVICE_GITHUB_SUGGESTION_STALE,
            (En, AdviceGithubSuggestionNotPrHead) => advice_en!(GithubSuggestionNotPrHead),
            (Ja, AdviceGithubSuggestionNotPrHead) => plan::github::ADVICE_GITHUB_SUGGESTION_NOT_PR_HEAD,
            (En, AdviceGithubSuggestionHeadUnavailable) => advice_en!(GithubSuggestionHeadUnavailable),
            (Ja, AdviceGithubSuggestionHeadUnavailable) => plan::github::ADVICE_GITHUB_SUGGESTION_HEAD_UNAVAILABLE,
            (En, AdviceGithubSuggestionWorkingTreeOnly) => advice_en!(GithubSuggestionWorkingTreeOnly),
            (Ja, AdviceGithubSuggestionWorkingTreeOnly) => plan::github::ADVICE_GITHUB_SUGGESTION_WORKING_TREE_ONLY,
            (En, AdviceGithubCommentBodyEmpty) => advice_en!(GithubCommentBodyEmpty),
            (Ja, AdviceGithubCommentBodyEmpty) => plan::github::ADVICE_GITHUB_COMMENT_BODY_EMPTY,
            (En, AdviceGithubIssueTitleEmpty) => advice_en!(GithubIssueTitleEmpty),
            (Ja, AdviceGithubIssueTitleEmpty) => plan::github::ADVICE_GITHUB_ISSUE_TITLE_EMPTY,
            (En, AdviceGithubIssueCreateFields) => advice_en!(GithubIssueCreateFields),
            (Ja, AdviceGithubIssueCreateFields) => plan::github::ADVICE_GITHUB_ISSUE_CREATE_FIELDS,
            (En, AdviceGithubIssueUnknownLabels) => advice_en!(GithubIssueUnknownLabels),
            (Ja, AdviceGithubIssueUnknownLabels) => plan::github::ADVICE_GITHUB_ISSUE_UNKNOWN_LABELS,
            (En, AdviceGithubIssueUnassignableUsers) => advice_en!(GithubIssueUnassignableUsers),
            (Ja, AdviceGithubIssueUnassignableUsers) => plan::github::ADVICE_GITHUB_ISSUE_UNASSIGNABLE_USERS,
            (En, AdviceGithubReviewBodyEmpty) => advice_en!(GithubReviewBodyEmpty),
            (Ja, AdviceGithubReviewBodyEmpty) => plan::github::ADVICE_GITHUB_REVIEW_BODY_EMPTY,
            (En, AdviceGithubFieldEditEmpty) => advice_en!(GithubFieldEditEmpty),
            (Ja, AdviceGithubFieldEditEmpty) => plan::github::ADVICE_GITHUB_FIELD_EDIT_EMPTY,
            (En, AdviceGithubLocalBranchNotDeletedDeletionUnauthorized) => advice_en!(GithubLocalBranchNotDeletedDeletionUnauthorized),
            (Ja, AdviceGithubLocalBranchNotDeletedDeletionUnauthorized) => plan::github::ADVICE_GITHUB_LOCAL_BRANCH_NOT_DELETED_DELETION_UNAUTHORIZED,
            // ── issue #346: ruleset badge ───────────────────────────
            (En, RulesetBadge) => "Ruleset",
            (Ja, RulesetBadge) => "Ruleset",
            (En, RulesetBadgeTooltip) => "This may violate the branch ruleset:",
            (Ja, RulesetBadgeTooltip) => "branch の ruleset に違反する可能性があります:",

            (En, ShortcutsRemapHelp) => "In ~/.kagi/settings.json, set \"keybinding.<command id>\" to a key combination; for example, \"keybinding.view.toggleSidebar\": \"secondary-shift-b\". Separate modifiers and the key with hyphens (secondary = ⌘ on macOS, Ctrl elsewhere). Use \"\" to unbind, then restart Kagi.",
            (Ja, ShortcutsRemapHelp) => "~/.kagi/settings.json の \"keybinding.<command id>\" にキーの組み合わせを設定します。例: \"keybinding.view.toggleSidebar\": \"secondary-shift-b\"。修飾キーとキーはハイフンで区切ります(secondary は macOS では ⌘、その他では Ctrl)。\"\" で割り当てを解除できます。変更後は Kagi を再起動してください。",
            // ── Generic guards ──────────────────────────────────────
            (En, AppNoticeDismiss) => "Dismiss",
            (En, ToastDismiss) => "Dismiss notification",
            (Ja, ToastDismiss) => "通知を閉じる",
            (En, BusyHideProgress) => "Hide progress (work continues)",
            (Ja, BusyHideProgress) => "進行状況を非表示（処理は続行）",
            (En, ToastOpenRepository) => "Open Repository",
            (Ja, ToastOpenRepository) => "リポジトリを開く",
            (En, ToastMore) => "{count} more notifications",
            (Ja, ToastMore) => "ほか{count}件の通知",
            (En, ToastMoreOne) => "1 more notification",
            (Ja, ToastMoreOne) => "ほか1件の通知",
            (En, ToastCollapse) => "Collapse notifications",
            (Ja, ToastCollapse) => "通知を折りたたむ",
            (En, AppNoticeTitle) => "Operation notice",
            (Ja, AppNoticeTitle) => "操作に関する通知",
            (En, AppNoticeDetailsInOpLog) => "View and copy operation details in Operation Log.",
            (Ja, AppNoticeDetailsInOpLog) => "操作ログで操作の詳細を確認・コピーできます。",
            (En, AppReconcileInspect) => "Inspect current state",
            (Ja, AppReconcileInspect) => "現在の状態を照会",
            (En, AppReconcileConfirm) => "I've checked",
            (Ja, AppReconcileConfirm) => "確認しました",
            (En, AppReconcileRequired) => "Check the previous operation's result before continuing.",
            (Ja, AppReconcileRequired) => "前の操作の結果を確認してから続行してください。",
            (En, AppReconcileAcknowledge) => "The operation stopped. Confirm you have checked the current state to continue. Nothing is retried.",
            (Ja, AppReconcileAcknowledge) => "操作は停止しました。現在の状態を確認したら続行できます。再実行はしません。",
            (En, AppReconcileUnobservable) => "The operation stopped, but Kagi couldn't confirm what happened on the remote. You can continue without checking; nothing is retried.",
            (Ja, AppReconcileUnobservable) => "操作は停止しましたが、remote 側の結果を確認できていません。確認せずに続行できます。再実行はしません。",
            (En, AppReconcileArmRelease) => "Review continuing without checking",
            (Ja, AppReconcileArmRelease) => "未確認のまま続行する内容を確認",
            (En, AppReconcileReleaseUnobservable) => "Continue without checking the remote",
            (Ja, AppReconcileReleaseUnobservable) => "remote を確認せずに続行",
            (En, AppReconcileReleaseArmed) => "Click again to continue without checking the remote. Nothing is retried.",
            (Ja, AppReconcileReleaseArmed) => "もう一度押すと、remote を確認せずに続行します。再実行はしません。",
            (Ja, AppNoticeDismiss) => "閉じる",
            (En, OpInProgress) => "another operation is in progress",
            (Ja, OpInProgress) => "別の操作が実行中です",
            (En, CheckoutStashCannotQueue) => "Can't stash and switch while another operation is running. Try again when it finishes.",
            (Ja, CheckoutStashCannotQueue) => "別の操作の実行中は stash して切り替えできません。完了後にやり直してください。",
            (En, SlowWriteNetwork) => "network: waiting for the remote",
            (Ja, SlowWriteNetwork) => "network: remote の応答を待っています",
            (En, SlowWriteRebase) => "rebase: replaying commits",
            (Ja, SlowWriteRebase) => "rebase: commit を適用中",
            (En, SlowWriteCheckout) => "checkout: updating the worktree",
            (Ja, SlowWriteCheckout) => "checkout: worktree を更新中",
            (En, SlowWriteRefs) => "refs: updating branches",
            (Ja, SlowWriteRefs) => "refs: branch を更新中",
            (En, SlowWriteMerge) => "merge: combining changes",
            (Ja, SlowWriteMerge) => "merge: 変更を統合中",
            (En, SlowWriteCommit) => "commit: writing repository history",
            (Ja, SlowWriteCommit) => "commit: 履歴を書き込み中",
            (En, SlowWriteStash) => "stash: updating saved changes",
            (Ja, SlowWriteStash) => "stash: 保存した変更を更新中",
            (En, SlowWriteWorktree) => "worktree: updating local files",
            (Ja, SlowWriteWorktree) => "worktree: ローカルファイルを更新中",
            (En, SlowWriteConflict) => "conflict resolution: updating local files",
            (Ja, SlowWriteConflict) => "conflict 解決: ローカルファイルを更新中",
            (En, SlowWriteLocal) => "writing local repository data",
            (Ja, SlowWriteLocal) => "ローカルの repository データを書き込み中",
            (En, SlowWriteGeneric) => "operation in progress",
            (Ja, SlowWriteGeneric) => "処理中",
            (En, TransportRetryHeld) => "Retry is turned off because this may already have changed the remote. Check the remote before trying again.",
            (Ja, TransportRetryHeld) => "remote が既に変わっている可能性があるため、再実行できません。remote を確認してからやり直してください。",
            (En, RemoteOpAwaitingCompletion) => "Kagi can't confirm yet whether this remote operation finished. Don't retry it; check the remote first.",
            (Ja, RemoteOpAwaitingCompletion) => "この remote 操作が完了したかまだ確認できていません。再実行せず、先に remote を確認してください。",
            (En, NoRepoOpen) => "no repository is open",
            (Ja, NoRepoOpen) => "リポジトリが開かれていません",
            (En, NoCommitSelected) => "no commit selected",
            (Ja, NoCommitSelected) => "commit が選択されていません",
            (En, DiffNotOpen) => "no diff is open",
            (Ja, DiffNotOpen) => "diff が開かれていません",
            (En | Ja, DiffViewSplit) => "Split",
            (En | Ja, DiffViewUnified) => "Unified",
            (En, DiffLineEndingLf) => "LF line ending",
            (Ja, DiffLineEndingLf) => "LF 改行",
            (En, DiffLineEndingCrLf) => "CRLF line ending",
            (Ja, DiffLineEndingCrLf) => "CRLF 改行",
            (En, DiffLineEndingNone) => "No line ending",
            (Ja, DiffLineEndingNone) => "改行なし",
            (En, DiffBinaryNoText) => "Binary file — no text diff. Open in external editor to compare.",
            (Ja, DiffBinaryNoText) => "binary ファイル — テキスト diff はありません。外部エディタで開いて比較してください。",
            (En, DiffImageAdded) => "Added image (no previous version)",
            (Ja, DiffImageAdded) => "追加された画像（以前のバージョンはありません）",
            (En, DiffImageModified) => "Modified image: before → after",
            (Ja, DiffImageModified) => "変更された画像：変更前 → 変更後",
            (En, DiffImageDeleted) => "Deleted image",
            (Ja, DiffImageDeleted) => "削除された画像",
            (En, DiffImageBefore) => "Before",
            (Ja, DiffImageBefore) => "変更前",
            (En, DiffImageAfter) => "After",
            (Ja, DiffImageAfter) => "変更後",
            (En, DiffImagePreviewUnavailable) => "Preview not available for this version.",
            (Ja, DiffImagePreviewUnavailable) => "このバージョンはプレビューできません。",
            (En, DiffPdfPreview) => "PDF preview",
            (Ja, DiffPdfPreview) => "PDF プレビュー",
            (En, DiffPdfPrevious) => "Previous version",
            (Ja, DiffPdfPrevious) => "変更前",
            (En, DiffPdfCurrent) => "Current version",
            (Ja, DiffPdfCurrent) => "変更後",
            (En, DiffPdfNoPrevious) => "No previous version (added file)",
            (Ja, DiffPdfNoPrevious) => "変更前のバージョンはありません（追加ファイル）",
            (En, DiffPdfNoCurrent) => "No current version (deleted file)",
            (Ja, DiffPdfNoCurrent) => "変更後のバージョンはありません（削除ファイル）",
            (En, DiffPdfReadFailed) => "Unable to read this version.",
            (Ja, DiffPdfReadFailed) => "このバージョンを読み込めません。",
            (En, DiffPdfScrollIndependent) => "Independent scrolling",
            (Ja, DiffPdfScrollIndependent) => "個別スクロール",
            (En, DiffPdfScrollSynchronized) => "Synchronized scrolling",
            (Ja, DiffPdfScrollSynchronized) => "同期スクロール",
            (En, ImageFileSize) => "Size",
            (Ja, ImageFileSize) => "サイズ",
            (En, ImageDimensions) => "Dimensions",
            (Ja, ImageDimensions) => "寸法",
            (En, CommitHeaderSelectPrompt) => "Select a commit to see its details.",
            (Ja, CommitHeaderSelectPrompt) => "commit を選択すると詳細が表示されます。",

            // ── Agent provenance (issue #337) ───────────────────────
            (En, AgentReviewed) => "reviewed",
            (Ja, AgentReviewed) => "レビュー済み",
            (En, AgentCreatedTooltip) => "Created by an AI agent",
            (Ja, AgentCreatedTooltip) => "AI エージェントによる作成",
            (En, AgentReviewedTooltip) => "Created by an AI agent, reviewed by a human",
            (Ja, AgentReviewedTooltip) => "AI エージェントが作成 / 人がレビュー済み",
            (En, AgentUnnamed) => "AI agent",
            (Ja, AgentUnnamed) => "AI エージェント",

            // ── Placeholders ────────────────────────────────────────
            (En, MultiWindowUnsupported) => "multiple windows are not supported",
            (Ja, MultiWindowUnsupported) => "複数ウィンドウは未対応です",
            (En, ResetUnimplemented) => "Reset is not available yet",
            (Ja, ResetUnimplemented) => "reset はまだ利用できません",
            (En, FinishDialogFirst) => "Finish or dismiss the open dialog first",
            (Ja, FinishDialogFirst) => "開いているダイアログを完了するか閉じてください",

            // ── Busy footers ────────────────────────────────────────
            (En, BusyCheckout) => "checkout in progress…",
            (Ja, BusyCheckout) => "checkout 実行中…",
            (En, BusySwitchToLatest) => "switching to latest…",
            (Ja, BusySwitchToLatest) => "最新へ切り替え中…",
            (En, BusyPull) => "pull in progress…",
            (Ja, BusyPull) => "pull 実行中…",
            (En, PullAutoStashConfirm) => "Stash & Pull",
            (Ja, PullAutoStashConfirm) => "StashしてPull",
            (En, PullReviewConflicts) => "Review conflict resolution",
            (Ja, PullReviewConflicts) => "conflict の解決方法を確認",
            (En, PullReviewChanges) => "Review changes",
            (Ja, PullReviewChanges) => "変更を確認",
            (En, AutoStashRestored) => "auto-stash restored",
            (Ja, AutoStashRestored) => "auto-stash を復元しました",
            (En, BusyPush) => "push in progress…",
            (Ja, BusyPush) => "push 実行中…",
            (En, BusyStash) => "stash in progress…",
            (Ja, BusyStash) => "stash 実行中…",
            (En, BusyStashPop) => "stash pop in progress…",
            (Ja, BusyStashPop) => "stash pop 実行中…",
            (En, StashPopConflictedKept) => {
                "applied with conflicts — the stash was KEPT. Resolve the conflicts, then drop the stash manually."
            }
            (Ja, StashPopConflictedKept) => {
                "conflict ありで適用しました — stash は保持されています。conflict を解決してから手動で drop してください。"
            }
            (En, BusyStashDrop) => "stash drop in progress…",
            (Ja, BusyStashDrop) => "stash drop 実行中…",
            (En, BusyCherryPick) => "cherry-pick in progress…",
            (Ja, BusyCherryPick) => "cherry-pick 実行中…",
            (En, BusyRevert) => "revert in progress…",
            (Ja, BusyRevert) => "revert 実行中…",
            (En, BusyAmend) => "amend in progress…",
            (Ja, BusyAmend) => "amend 実行中…",
            (En, BusyDeleteBranch) => "delete branch in progress…",
            (Ja, BusyDeleteBranch) => "delete branch 実行中…",
            (En, BusyDeleteBranchPlan) => "planning delete branch…",
            (Ja, BusyDeleteBranchPlan) => "delete branch 計画中…",
            (En, BusyDiscard) => "discard in progress…",
            (Ja, BusyDiscard) => "discard 実行中…",
            (En, BusyCommit) => "commit in progress…",
            (Ja, BusyCommit) => "commit 実行中…",
            (En, BusyCreateWorktree) => "create worktree in progress…",
            (Ja, BusyCreateWorktree) => "create worktree 実行中…",
            (En, BusyRemoveWorktree) => "remove worktree in progress…",
            (Ja, BusyRemoveWorktree) => "remove worktree 実行中…",
            (En, WorktreeMeasuring) => "Measuring…",
            (Ja, WorktreeMeasuring) => "計測中…",
            (En, WorktreeNotMeasured) => "Not measured",
            (Ja, WorktreeNotMeasured) => "未計測",
            (En, WorktreeInspectionRefresh) => "Refresh measurement",
            (Ja, WorktreeInspectionRefresh) => "再計測",
            (En, WorktreeInspectionStale) => "Refresh required — Git state may have changed",
            (Ja, WorktreeInspectionStale) => "更新が必要 — Git の状態が変わった可能性があります",
            (En, WorktreeSafeMerged) => "Git removal conditions met: clean, merged, unlocked",
            (Ja, WorktreeSafeMerged) => "Git 上の削除条件を満たしています: clean・merge 済み・lock なし",
            (En, WorktreeSafePushed) => "Git removal conditions met: clean, pushed, unlocked",
            (Ja, WorktreeSafePushed) => "Git 上の削除条件を満たしています: clean・push 済み・lock なし",
            (En, WorktreeMainExcluded) => "Not removable: main worktree",
            (Ja, WorktreeMainExcluded) => "削除対象外: main worktree",
            (En, WorktreeDirty) => "Not safe: uncommitted or untracked changes",
            (Ja, WorktreeDirty) => "削除不可: 未 commit の変更または untracked ファイルがあります",
            (En, WorktreeLocked) => "Not safe: worktree is locked",
            (Ja, WorktreeLocked) => "削除不可: worktree が lock されています",
            (En, WorktreeUnpublished) => "Not safe: HEAD is neither pushed nor merged",
            (Ja, WorktreeUnpublished) => "削除不可: HEAD は push 済みでも merge 済みでもありません",
            (En, WorktreeUnborn) => "Unknown: HEAD has no commit yet",
            (Ja, WorktreeUnborn) => "不明: HEAD に commit がありません",
            (En, WorktreeUpstreamUnavailable) => "Unknown: upstream has not been fetched",
            (Ja, WorktreeUpstreamUnavailable) => "不明: upstream が未取得です",
            (En, WorktreeObservationFailed) => "Unknown: couldn't read this worktree",
            (Ja, WorktreeObservationFailed) => "不明: worktree の状態を読み取れませんでした",
            (En, WorktreeSafeShort) => "Git-safe",
            (Ja, WorktreeSafeShort) => "Git条件OK",
            (En, WorktreeKeepShort) => "Keep",
            (Ja, WorktreeKeepShort) => "保持",
            (En, WorktreeUnknownShort) => "Unknown",
            (Ja, WorktreeUnknownShort) => "不明",
            (En, WorktreeDirtyShort) => "Dirty",
            (Ja, WorktreeDirtyShort) => "変更あり",
            (En, WorktreeLockedShort) => "Locked",
            (Ja, WorktreeLockedShort) => "ロック中",
            (En, WorktreeStaleShort) => "Stale",
            (Ja, WorktreeStaleShort) => "要更新",
            (En, WorktreeErrorShort) => "Error",
            (Ja, WorktreeErrorShort) => "失敗",
            (En, WorktreeDetachedShort) => "Detached HEAD",
            // `detached HEAD` is a Git domain word and stays English (ADR-0048),
            // matching `CmDetachedHead`.
            (Ja, WorktreeDetachedShort) => "detached HEAD",
            (En, MenuUnlockWorktree) => "Unlock worktree…",
            (Ja, MenuUnlockWorktree) => "worktree のロックを解除…",
            (En, MenuWorktreeNotLocked) => "This worktree is not locked",
            (Ja, MenuWorktreeNotLocked) => "この worktree はロックされていません",
            (En, MenuLockWorktree) => "Lock worktree…",
            (Ja, MenuLockWorktree) => "worktree をロック…",
            (En, MenuWorktreeAlreadyLocked) => "This worktree is already locked",
            (Ja, MenuWorktreeAlreadyLocked) => "この worktree は既にロックされています",
            (En, MenuRemoveWorktreeKeepBranch) => "Remove worktree (keep branch)…",
            (Ja, MenuRemoveWorktreeKeepBranch) => "worktree を削除(branch は残す)…",
            (En, MenuRemoveWorktreeAndBranch) => "Remove worktree and branch…",
            (Ja, MenuRemoveWorktreeAndBranch) => "worktree と branch を削除…",
            (En, MenuPruneWorktrees) => "Prune stale worktrees…",
            (Ja, MenuPruneWorktrees) => "古い worktree を prune…",
            (En, MenuRepairWorktrees) => "Repair worktree links…",
            (Ja, MenuRepairWorktrees) => "worktree リンクを修復…",
            (En, MenuOpenWorktreeInNewTab) => "Open in new tab",
            (Ja, MenuOpenWorktreeInNewTab) => "新しいタブで開く",
            (En, MenuOpenWorktreeDir) => "Open worktree",
            (Ja, MenuOpenWorktreeDir) => "worktree を開く",
            (En, GraphDetachedWorktree) => "detached",
            // `detached` is a Git domain word and stays English (ADR-0048).
            (Ja, GraphDetachedWorktree) => "detached",
            (En, InWorktree) => "in worktree",
            (Ja, InWorktree) => "対象 worktree",
            (En, WorktreeLockDefaultReason) => "locked in kagi",
            (Ja, WorktreeLockDefaultReason) => "locked in kagi",
            (En, WorktreeLockReason) => "Lock reason (optional)",
            (Ja, WorktreeLockReason) => "ロック理由（任意）",
            (En, WorktreeLockReview) => "Review lock…",
            (Ja, WorktreeLockReview) => "ロック内容を確認…",
            (En, TerminalAutoLockPendingRelease) => "Confirm the terminal worktree unlock or unlock it manually before restarting the shell",
            (Ja, TerminalAutoLockPendingRelease) => "terminal の worktree ロック解除を確認するか、手動で解除してから shell を再起動してください",
            (En, TerminalAutoLockStaleOwner) => "This lock no longer belongs to this terminal. Unlock it manually.",
            (Ja, TerminalAutoLockStaleOwner) => "このロックはこの terminal のものではなくなりました。手動で解除してください。",
            (En, TerminalNotStartedTitle) => "Shell not started",
            (Ja, TerminalNotStartedTitle) => "shell は未起動です",
            (En, TerminalExitedTitle) => "Shell exited",
            (Ja, TerminalExitedTitle) => "shell が終了しました",
            (En, TerminalFailedTitle) => "Shell failed",
            (Ja, TerminalFailedTitle) => "shell でエラーが発生しました",
            (En, TerminalNotStartedHint) => "Start a shell in this repository.",
            (Ja, TerminalNotStartedHint) => "このリポジトリで shell を起動します。",
            (En, TerminalRestartHint) => "Restart to open a new shell in this repository.",
            (Ja, TerminalRestartHint) => "再起動すると、このリポジトリで新しい shell が開きます。",
            (En, TerminalRestart) => "Restart",
            (Ja, TerminalRestart) => "Restart",
            (En, TerminalPasteTitle) => "Paste into Terminal?",
            (Ja, TerminalPasteTitle) => "Terminal に Paste しますか？",
            (En, TerminalPasteBody) => "This shell has not enabled bracketed paste. Newlines in this text may execute commands immediately. Paste only if you trust the clipboard contents.",
            (Ja, TerminalPasteBody) => "この shell は bracketed paste を有効にしていません。テキストの改行でコマンドが直ちに実行される場合があります。clipboard の内容を信頼できる場合だけ Paste してください。",
            (En, TerminalPasteControlBody) => "This clipboard contains invisible control characters that may interrupt programs or execute commands. This shell has not enabled bracketed paste. Paste only if you trust the clipboard contents.",
            (Ja, TerminalPasteControlBody) => "clipboard に不可視の制御文字が含まれています。プログラムの中断やコマンドの実行につながる場合があります。この shell は bracketed paste を有効にしていません。内容を信頼できる場合だけ Paste してください。",
            (En, TerminalPasteConfirm) => "Paste",
            (Ja, TerminalPasteConfirm) => "Paste",
            (En, TerminalFocusDescription) => "Terminal input. Tab and Shift-Tab send shell completion keys. Press Control-Shift-Tab or Command-Shift-J to focus the bottom-panel tabs; use Left or Right, then Return to switch tabs.",
            (Ja, TerminalFocusDescription) => "Terminal 入力。Tab と Shift-Tab は shell の補完キーを送信します。Control-Shift-Tab または Command-Shift-J で下部パネルのタブに移動し、左右キーと Return でタブを切り替えます。",
            (En, UnbornEmptyTitle) => "No commits yet",
            (Ja, UnbornEmptyTitle) => "まだ commit がありません",
            (En, UnbornEmptyHint) => "Add files and create the first commit",
            (Ja, UnbornEmptyHint) => "ファイルを追加して最初の commit を作成してください",
            (En, BusyMerge) => "merge in progress…",
            (Ja, BusyMerge) => "merge 実行中…",
            (En, MergeDestinationChanged) => "The destination worktree or branch changed. Refresh and try the merge again.",
            (Ja, MergeDestinationChanged) => "merge 先の worktree または branch が変わりました。更新してからもう一度操作してください。",

            // ── No-op toasts ─────────────────────────────────────────
            (En, AlreadyUpToDatePull) => "Already up to date — nothing to pull",
            (Ja, AlreadyUpToDatePull) => "すでに最新です。pull するものはありません",
            (En, AlreadyUpToDatePush) => "Already up to date — nothing to push",
            (Ja, AlreadyUpToDatePush) => "すでに最新です。push するものはありません",
            (En, PullUpstreamChangedDuringFetch) => "Pull upstream changed during fetch; request Pull again",
            (Ja, PullUpstreamChangedDuringFetch) => "fetch 中に Pull の upstream が変わりました。もう一度 Pull してください。",
            (En, PullFilesChangedDuringPlanning) => "Your files changed while planning Pull. Nothing was stashed or pulled; press Pull again.",
            (Ja, PullFilesChangedDuringPlanning) => "Pull の計画中にファイルが変わりました。stash も pull もしていません。もう一度 Pull を押してください。",
            (En, PullAutoStashIdentityChanged) => "The branch or its remote changed after you confirmed. Nothing was stashed or pulled; press Pull again.",
            (Ja, PullAutoStashIdentityChanged) => "確認後に branch または remote が変わりました。stash も pull もしていません。もう一度 Pull を押してください。",
            (En, PullAutoStashPlanStale) => "Your files changed after you confirmed. Nothing was stashed or pulled; press Pull again.",
            (Ja, PullAutoStashPlanStale) => "確認後にファイルが変わりました。stash も pull もしていません。もう一度 Pull を押してください。",
            (En, PullAutoStashRestoreChanged) => "The stash restore preview changed after confirmation. Nothing was stashed or pulled; press Pull again to review the new restore preview.",
            (Ja, PullAutoStashRestoreChanged) => "確認後に stash の復元予測が変わりました。stash も pull も実行していません。Pull をもう一度押して最新の復元予測を確認してください。",

            // ── Toolbar guards ──────────────────────────────────────
            (En, PullBusy) => "Pull: another operation is in progress",
            (Ja, PullBusy) => "Pull: 別の操作が実行中です",
            (En, PullDetached) => "Pull: detached HEAD — switch to a branch first",
            (Ja, PullDetached) => "Pull: detached HEAD です。branch に切り替えてください",
            (En, PullUnborn) => "Pull: no commits yet — no upstream",
            (Ja, PullUnborn) => "Pull: commit も upstream もまだありません",
            (En, PullNoUpstream) => "Pull: no upstream branch is set",
            (Ja, PullNoUpstream) => "Pull: upstream が設定されていません",
            (En, PullNothing) => "Pull: already up to date",
            (Ja, PullNothing) => "Pull: pull する変更がありません",
            (En, PushBusy) => "Push: another operation is in progress",
            (Ja, PushBusy) => "Push: 別の操作が実行中です",
            (En, PushDetached) => "Push: detached HEAD — switch to a branch first",
            (Ja, PushDetached) => "Push: detached HEAD です。branch に切り替えてください",
            (En, PushUnborn) => "Push: no commits yet — no upstream",
            (Ja, PushUnborn) => "Push: commit も upstream もまだありません",
            (En, PushNoRemote) => "Push: no upstream and no remote configured",
            (Ja, PushNoRemote) => "Push: upstream も remote も設定されていません",
            (En, PushNothing) => "Push: nothing to push",
            (Ja, PushNothing) => "Push: push する変更がありません",
            (En, StashClean) => "Stash: working tree is clean — nothing to stash",
            (Ja, StashClean) => "working tree に stash する変更がありません",
            (En, StashBusy) => "Stash: another operation is in progress",
            (Ja, StashBusy) => "別の操作が実行中です",
            (En, PopEmpty) => "Pop: stash is empty",
            (Ja, PopEmpty) => "stash した変更がありません",
            (En, PopBusy) => "Pop: another operation is in progress",
            (Ja, PopBusy) => "別の操作が実行中です",
            // ── Operation Undo / Redo (ADR-0081; domain words English) ──
            (En, Undo) | (Ja, Undo) => "Undo",
            (En, Redo) | (Ja, Redo) => "Redo",
            (En, NothingToUndo) => "nothing to undo",
            (Ja, NothingToUndo) => "undo する操作がありません",
            (En, NothingToRedo) => "nothing to redo",
            (Ja, NothingToRedo) => "redo する操作がありません",

            // ── Checkout / compare prose ────────────────────────────
            (En, CheckoutSelectFirst) => "Checkout: select a commit, then press Enter",
            (Ja, CheckoutSelectFirst) => "Checkout: commit を選択してから Enter",
            (En, AlreadyHead) => "already at HEAD",
            (Ja, AlreadyHead) => "既に HEAD です",
            (En, NoLocalChanges) => "no local changes",
            (Ja, NoLocalChanges) => "local changes がありません",
            (En, DirtyStashFirst) => {
                "Working tree is dirty: confirming will stash your changes first \
                 (saved to stash@{0}, restore with `git stash pop`)"
            }
            (Ja, DirtyStashFirst) => {
                "Working tree が dirty です: 確定すると先に変更を stash します\
                 (stash@{0} に保存、`git stash pop` で復元)"
            }
            (En, AmendNeedMessageOrStaged) => "Amend: enter a message or stage changes",
            (Ja, AmendNeedMessageOrStaged) => "Amend: メッセージを入力するか変更を stage してください",
            (En, AmendStatusLoading) => "Amend: waiting for repository status",
            (Ja, AmendStatusLoading) => "Amend: リポジトリの状態を読み込み中",

            // ── Context-menu disabled reasons ───────────────────────
            (En, CmDetachedHead) => "detached HEAD",
            (Ja, CmDetachedHead) => "detached HEAD",
            (En, CmSameAsHead) => "same as HEAD",
            (Ja, CmSameAsHead) => "HEAD と同一",
            (En, CmMergeUnsupported) => "Not available for merge commits",
            (Ja, CmMergeUnsupported) => "merge commit には使えません",
            (En, CmAlreadyInBranch) => "already in the current branch",
            (Ja, CmAlreadyInBranch) => "既に現在 branch に含まれています",
            (En, CmNotInBranch) => "not in the current branch",
            (Ja, CmNotInBranch) => "現在 branch に含まれない",
            (En, CmAlreadyHead) => "already at HEAD",
            (Ja, CmAlreadyHead) => "既に HEAD",
            (En, CmIdentical) => "identical",
            (Ja, CmIdentical) => "同一",
            (En, CmNoLocalChanges) => "no local changes",
            (Ja, CmNoLocalChanges) => "local changes がありません",
            (En, CmResetUnneeded) => "not needed (same as HEAD)",
            (Ja, CmResetUnneeded) => "不要(HEAD と同一)",
            (En, CmNoCurrentBranch) => "no current branch",
            (Ja, CmNoCurrentBranch) => "現在 branch がありません",
            (En, CmNoSelectedBranch) => "selected commit has no local branch",
            (Ja, CmNoSelectedBranch) => "選択した commit にローカル branch がありません",
            (En, CmResetUnimplemented) => "Reset is not available yet",
            (Ja, CmResetUnimplemented) => "reset はまだ利用できません",
            (En, BcmBusy) => "another operation is in progress",
            (Ja, BcmBusy) => "別の操作が実行中です",
            (En, BcmNotImplementedYet) => "Not available here yet",
            (Ja, BcmNotImplementedYet) => "ここではまだ利用できません",
            (En, BcmCurrentBranch) => "current branch",
            (Ja, BcmCurrentBranch) => "現在 branch",
            (En, BcmLocalOnly) => "local branches only",
            (Ja, BcmLocalOnly) => "local branch のみ",
            (En, BcmCurrentBranchOnly) => "current branch only",
            (Ja, BcmCurrentBranchOnly) => "現在の branch のみ",
            (En, BcmOnlyFromCurrentBranch) => "only available from the current branch",
            (Ja, BcmOnlyFromCurrentBranch) => "現在の branch からのみ実行できます",
            (En, BcmNoUpstream) => "no upstream configured",
            (Ja, BcmNoUpstream) => "upstream が設定されていません",
            (En, BcmDetachedHead) => "detached HEAD",
            (Ja, BcmDetachedHead) => "detached HEAD",
            (En, BcmDeleteBranch) => "Delete branch...",
            (Ja, BcmDeleteBranch) => "branch を削除...",
            (En, BcmDeleteUnmerged) => "Delete branch... (unmerged)",
            (Ja, BcmDeleteUnmerged) => "branch を削除...（unmerged）",
            (En, BcmCheckedOutElsewhere) => "branch is checked out in another worktree",
            (Ja, BcmCheckedOutElsewhere) => "branch は別の worktree で checkout 済みです",
            (En, BcmConflictMode) => "resolve conflicts first",
            (Ja, BcmConflictMode) => "先に conflict を解決してください",
            (En, BcmNothingToPush) => "nothing to push",
            (Ja, BcmNothingToPush) => "push するものがありません",
            (En, BcmIntegrateMenu) => "Integrate",
            (Ja, BcmIntegrateMenu) => "統合",
            (En, BcmAdvancedMenu) => "Advanced",
            (Ja, BcmAdvancedMenu) => "詳細",
            (En, BcmWorktreeMenu) => "Worktree",
            (Ja, BcmWorktreeMenu) => "Worktree",

            // ── Empty states ────────────────────────────────────────
            (En, NoLocalBranches) => "No local branches",
            (Ja, NoLocalBranches) => "ローカル branch がありません",
            (En, BranchPickerCheckout) => "Checkout Branch",
            (Ja, BranchPickerCheckout) => "checkout する branch",
            (En, BranchPickerDelete) => "Delete Branch",
            (Ja, BranchPickerDelete) => "削除する branch",
            (En, BranchPickerRename) => "Rename Branch",
            (Ja, BranchPickerRename) => "リネームする branch",
            (En, A11yIssueOpenRow) => "Open issue #{}, {}, {}",
            (Ja, A11yIssueOpenRow) => "Issue #{} を開く、{}、{}",
            (En, A11yRecentOpenRow) => "Open repository {name}",
            (Ja, A11yRecentOpenRow) => "リポジトリ {name} を開く",
            (En, CommandPalettePlaceholder) => "Search commands…",
            (Ja, CommandPalettePlaceholder) => "コマンドを検索…",
            (En, CommandPaletteNoResults) => "No matching commands",
            (Ja, CommandPaletteNoResults) => "一致するコマンドがありません",
            (En, CommandPaletteActions) => "Actions",
            (Ja, CommandPaletteActions) => "操作",
            (En, NoOperationsYet) => "No operations yet",
            (Ja, NoOperationsYet) => "操作履歴はまだありません",
            (En, OpLogCopyEntry) => "Copy entry",
            (Ja, OpLogCopyEntry) => "エントリをコピー",
            (En, OpLogDetailSelectionOff) => {
                "Long entry: text selection is off. The copy button copies the full text."
            }
            (Ja, OpLogDetailSelectionOff) => {
                "長いエントリのため文字選択は無効です。全文はコピーボタンから取得できます。"
            }
            // #1025 — Operation Log plan-recovery section.
            (En, OpLogRecovery) => "Recovery",
            (Ja, OpLogRecovery) => "復旧",
            (En, OpLogRecoveryNotRecorded) => "not recorded",
            (Ja, OpLogRecoveryNotRecorded) => "記録されていません",
            (En, OpLogRecoveryCommand) => "command:",
            (Ja, OpLogRecoveryCommand) => "コマンド:",

            // ── Misc footers ────────────────────────────────────────
            (En, Refreshed) => "Refreshed",
            (Ja, Refreshed) => "更新しました",
            (En, SnapshotCreated) => "Snapshot created",
            (Ja, SnapshotCreated) => "スナップショットを作成しました",
            (En, SnapshotFailed) => "Snapshot failed",
            (Ja, SnapshotFailed) => "スナップショットの作成に失敗しました",
            (En, FetchAuthenticationRequired) => "Fetch from {} requires authentication. Check your credentials.",
            (Ja, FetchAuthenticationRequired) => "{} からの Fetch には認証が必要です。認証情報を確認してください。",
            (En, LoadMoreCommits) => "Load more commits",
            (Ja, LoadMoreCommits) => "commit をさらに読み込む",
            (En, OpenedInFinder) => "Opened in Finder",
            (Ja, OpenedInFinder) => "Finder で開きました",
            (En, ModalFilesToChange) => "Files to change ({})",
            (Ja, ModalFilesToChange) => "変更するファイル ({})",
            (En, ModalFilesMore) => "…and {} more",
            (Ja, ModalFilesMore) => "…ほか {} 件",
            (En, ModalFilesMoreUnknown) => "…and more files",
            (Ja, ModalFilesMoreUnknown) => "…ほかにもファイルがあります",

            // ── W30-CONFLICT-UI: Conflict Mode ──────────────────────
            (En, ConflictContinue) => "Continue",
            (Ja, ConflictContinue) => "続行",
            (En, ConflictAbort) => "Abort",
            (Ja, ConflictAbort) => "中止",
            (En, ConflictSkip) => "Skip",
            (Ja, ConflictSkip) => "スキップ",
            (En, ConflictResolved) => "resolved",
            (Ja, ConflictResolved) => "resolved",
            (En, ConflictUnresolved) => "unresolved",
            (Ja, ConflictUnresolved) => "unresolved",
            (En, ConflictResolvedShort) => "resolved",
            (Ja, ConflictResolvedShort) => "resolved",
            (En, ConflictNeedsReview) => "needs review",
            (Ja, ConflictNeedsReview) => "要確認",
            (En, ConflictKindContent) => "content",
            (Ja, ConflictKindContent) => "content",
            (En, ConflictKindRenameDelete) => "rename/delete",
            (Ja, ConflictKindRenameDelete) => "rename/delete",
            (En, ConflictKindModifyDelete) => "modify/delete",
            (Ja, ConflictKindModifyDelete) => "modify/delete",
            (En, ConflictKindAddAdd) => "add/add",
            (Ja, ConflictKindAddAdd) => "add/add",
            (En, ConflictKindSubmodule) => "submodule",
            (Ja, ConflictKindSubmodule) => "submodule",
            (En, ConflictKindSymlink) => "symlink",
            (Ja, ConflictKindSymlink) => "シンボリックリンク",
            (En, ConflictKindBinary) => "binary",
            (Ja, ConflictKindBinary) => "binary",
            (En, ConflictKindDirFile) => "directory/file",
            (Ja, ConflictKindDirFile) => "ディレクトリ/ファイル",
            (En, ConflictSelectFile) => "Select a conflicting file to resolve it",
            (Ja, ConflictSelectFile) => "解決する conflict ファイルを選択してください",
            (En, ConflictKeepCurrent) => "Keep current",
            (Ja, ConflictKeepCurrent) => "現在の側を採用",
            (En, ConflictTakeIncoming) => "Take incoming",
            (Ja, ConflictTakeIncoming) => "取り込む側を採用",
            (En, ConflictKeepBoth) => "Keep both (current first)",
            (Ja, ConflictKeepBoth) => "両方採用(現在の側を先)",
            (En, ConflictKeepDirectory) => "Keep directory…",
            (Ja, ConflictKeepDirectory) => "ディレクトリを採用…",
            (En, ConflictKeepFile) => "Keep file…",
            (Ja, ConflictKeepFile) => "ファイルを採用…",
            (En, ConflictConfirmDeletion) => "Confirm deletion (deleted on both sides)…",
            (Ja, ConflictConfirmDeletion) => "削除を確定（両側で削除済み）…",
            (En, ConflictUseDeletedSide) => "Use deleted side…",
            (Ja, ConflictUseDeletedSide) => "削除側を採用…",
            (En, ConflictUseModifiedSide) => "Use modified side",
            (Ja, ConflictUseModifiedSide) => "変更側を採用",
            (En, ConflictResolutionPlan) => "Review conflict resolution",
            (Ja, ConflictResolutionPlan) => "conflict の解決内容を確認",
            (En, ConflictResolutionConfirm) => "Confirm resolution",
            (Ja, ConflictResolutionConfirm) => "解決を確定",
            (En, ConflictResolutionConsequence) => "Only the paths below will change. Removed working files will no longer be present. The operation remains in progress until you review Continue.",
            (Ja, ConflictResolutionConsequence) => "変更されるのは以下のパスのみです。削除される作業ファイルは残りません。「続行」の内容を確認するまで操作は進行中のままです。",
            (En, ConflictRemoveIndex) => "Remove from index",
            (Ja, ConflictRemoveIndex) => "index から削除",
            (En, ConflictRemoveWorktree) => "Remove working files",
            (Ja, ConflictRemoveWorktree) => "作業ファイルを削除",
            (En, ConflictKeepPaths) => "Keep",
            (Ja, ConflictKeepPaths) => "保持",
            (En, ConflictRestorePaths) => "Restore at original path",
            (Ja, ConflictRestorePaths) => "元のパスに復元",
            (En, ConflictDirFileHint) => {
                "One side is a directory, the other a file — keep exactly one."
            }
            (Ja, ConflictDirFileHint) => "片方はディレクトリ、片方はファイルです。どちらか一方を採用します。",
            (En, ConflictResultPreview) => "Result preview",
            (Ja, ConflictResultPreview) => "解決結果プレビュー",
            (En, ConflictPreviewHint) => "Choose a side above to preview the resolved file.",
            (Ja, ConflictPreviewHint) => "上のボタンで側を選ぶと解決後のファイルをプレビューします。",
            (En, ConflictBinaryNoPreview) => "Binary file — choose a side; no text preview is available.",
            (Ja, ConflictBinaryNoPreview) => "binary ファイルです。側を選択してください。テキストプレビューはありません。",
            (En, ConflictSymlinkTarget) => "Symlink target",
            (Ja, ConflictSymlinkTarget) => "シンボリックリンクの参照先",
            (En, ConflictOpenBothExternal) => "Open both sides in external editor",
            (Ja, ConflictOpenBothExternal) => "両方の側を外部エディタで開く",
            (En, ConflictImageTooLarge) => "Image too large to preview inline.",
            (Ja, ConflictImageTooLarge) => "画像が大きすぎてインラインプレビューできません。",
            (En, ConflictSubmoduleCommit) => "Submodule commit",
            (Ja, ConflictSubmoduleCommit) => "submodule の commit",
            // ── W32-CONFLICT-EDITOR ──────────────────────────────────
            (En, ConflictInProgress) => "In progress",
            (Ja, ConflictInProgress) => "作業中",
            (En, ConflictSaved) => "Saved",
            (Ja, ConflictSaved) => "保存済み",
            (En, ConflictSavedStaged) => "Saved · staged",
            (Ja, ConflictSavedStaged) => "保存済み · stage 済み",
            (En, ConflictStagedContents) => "Staged contents",
            (Ja, ConflictStagedContents) => "stage 済みの内容",
            (En, ConflictReviewContinue) => "Review to continue",
            (Ja, ConflictReviewContinue) => "確認へ進む",
            (En, ConflictUnsaved) => "Unsaved",
            (Ja, ConflictUnsaved) => "未保存",
            (En, ConflictSaveRefused) => "Unsaved · save refused",
            (Ja, ConflictSaveRefused) => "未保存 · 保存を拒否",
            (En, ConflictWorktreeChanged) => "File changed outside Kagi",
            (Ja, ConflictWorktreeChanged) => "ファイルが外部で変更されました",
            (En, ConflictWorktreeChangedHelp) => "The file changed outside Kagi. Undo the external change and reload, or resolve and stage it outside; your draft is kept.",
            (Ja, ConflictWorktreeChangedHelp) => "ファイルが Kagi の外で変更されました。外部の変更を取り消して 再読込 するか、外部で解決して stage してください。下書きは保持しています。",
            (En, ConflictReload) => "Reload",
            (Ja, ConflictReload) => "再読込",
            (En, ConflictSaveAgain) => "Save again",
            (Ja, ConflictSaveAgain) => "もう一度保存",
            (En, ConflictNextFile) => "Next file in progress",
            (Ja, ConflictNextFile) => "次の作業中ファイル",
            (En, ConflictNotCommitted) => "Not committed",
            (Ja, ConflictNotCommitted) => "未 commit",
            (En, ConflictReviewNeeded) => "Review needed",
            (Ja, ConflictReviewNeeded) => "要確認",
            (En, ConflictShowSidebar) => "Show sidebar",
            (Ja, ConflictShowSidebar) => "サイドバーを表示",
            (En, ConflictHideSidebar) => "Hide sidebar",
            (Ja, ConflictHideSidebar) => "サイドバーを非表示",
            (En, ConflictReview) => "Review result",
            (Ja, ConflictReview) => "結果を確認",
            (En, ConflictBackToHunk) => "Choices",
            (Ja, ConflictBackToHunk) => "選択に戻る",
            (En, ConflictTools) => "More",
            (Ja, ConflictTools) => "その他",
            (En, ConflictDraft) => "Draft",
            (Ja, ConflictDraft) => "下書き",
            (En, ConflictDraftReady) => "Ready to save",
            (Ja, ConflictDraftReady) => "保存できます",
            (En, ConflictStaged) => "Staged",
            (Ja, ConflictStaged) => "stage 済み",
            (En, ConflictHunkResult) => "Result",
            (Ja, ConflictHunkResult) => "結果",
            (En, ConflictEmptyResult) => "∅",
            (Ja, ConflictEmptyResult) => "∅",
            (En, ConflictManualResult) => "Manual result",
            (Ja, ConflictManualResult) => "手動編集の結果",
            (En, ConflictUseThisSide) => "Use this side",
            (Ja, ConflictUseThisSide) => "採用",
            (En, ConflictUseThisSideAccessible) => "Use this side",
            (Ja, ConflictUseThisSideAccessible) => "この側を使う",
            (En, ConflictBothCurrentFirst) => "Both (current → incoming)",
            (Ja, ConflictBothCurrentFirst) => "両方（現在の側 → 取り込む側）",
            (En, ConflictBothIncomingFirst) => "Both (incoming → current)",
            (Ja, ConflictBothIncomingFirst) => "両方（取り込む側 → 現在の側）",
            (En, ConflictUsed) => "Used",
            (Ja, ConflictUsed) => "採用",
            (En, ConflictNotUsed) => "Not used",
            (Ja, ConflictNotUsed) => "不採用",
            (En, ConflictPartlyUsed) => "Partly used",
            (Ja, ConflictPartlyUsed) => "一部採用",
            (En, ConflictChange) => "Change",
            (Ja, ConflictChange) => "変更",
            (En, ConflictChangeAccessible) => "Change",
            (Ja, ConflictChangeAccessible) => "選び直す",
            (En, ConflictResetFocused) => "Reset",
            (Ja, ConflictResetFocused) => "リセット",
            (En, ConflictResetFocusedAccessible) => "Reset this conflict",
            (Ja, ConflictResetFocusedAccessible) => "この conflict をリセット",
            (En, ConflictEditResult) => "Edit",
            (Ja, ConflictEditResult) => "編集",
            (En, ConflictRemainingFiles) => "Remaining files",
            (Ja, ConflictRemainingFiles) => "残りのファイル",
            (En, ConflictToCommit) => "Review commit",
            (Ja, ConflictToCommit) => "commit を確認",
            (En, EditorCurrentSide) => "Current",
            (Ja, EditorCurrentSide) => "現在の側",
            (En, EditorIncomingSide) => "Incoming",
            (Ja, EditorIncomingSide) => "取り込む側",
            (En, EditorConflictNofM) => "conflict",
            (Ja, EditorConflictNofM) => "conflict",
            (En, EditorPrevHunk) => "‹ Prev",
            (Ja, EditorPrevHunk) => "‹ 前へ",
            (En, EditorNextHunk) => "Next ›",
            (Ja, EditorNextHunk) => "次へ ›",
            (En, EditorOpenExternal) => "Open external tool",
            (Ja, EditorOpenExternal) => "外部ツールで開く",
            (En, EditorReset) => "Reset all",
            (Ja, EditorReset) => "すべてリセット",
            (En, EditorSave) => "Save",
            (Ja, EditorSave) => "保存",
            (En, EditorResultOutput) => "Result",
            (Ja, EditorResultOutput) => "結果",
            (En, EditorAllResolved) => "All hunks resolved",
            (Ja, EditorAllResolved) => "すべての hunk を解決しました",
            (En, EditorUnresolvedHunks) => "hunk(s) still unresolved",
            (Ja, EditorUnresolvedHunks) => "件の hunk が未解決です",
            (En, EditorSavedResolved) => "Saved. File marked as resolved.",
            (Ja, EditorSavedResolved) => "保存しました。ファイルを解決済みにしました。",
            (En, EditorNoTextMerge) => "No text merge is available for this file (binary or single-sided). Use the conflict list to choose a side.",
            (Ja, EditorNoTextMerge) => "このファイルはテキスト merge できません(binary / 片側のみ)。conflict 一覧で側を選択してください。",
            // ── T-CONFLICT-UI/UX: 3-pane editor controls ──
            (En, EditorResetAllConfirm) => "Confirm Reset All",
            (Ja, EditorResetAllConfirm) => "すべてリセットを確認",
            // ── T-CONFLICT-UX-010/012: per-hunk accept controls ──
            (En, EditorHunkLabel) => "Hunk",
            (Ja, EditorHunkLabel) => "Hunk",
            // Deliberately English in BOTH languages: the JA reading ("現在の
            // 方を先") was more confusing than the English (user feedback), and
            // the arrow shows what "first" even means — the order the two
            // blocks land in the result when both sides are taken.
            (En | Ja, EditorCurrentFirst) => "Current → Incoming",
            (En | Ja, EditorIncomingFirst) => "Incoming → Current",
            (En, EditorSelectionNone) => "Not adopted",
            (Ja, EditorSelectionNone) => "未採用",
            (En, EditorSelectionPartial) => "Partially adopted",
            (Ja, EditorSelectionPartial) => "一部を採用",
            (En, EditorSelectionAll) => "Fully adopted",
            (Ja, EditorSelectionAll) => "すべて採用済み",
            // ── W33-CONFLICT-DASHBOARD ───────────────────────────────
            (En, ConflictDashHeader) => "Merge conflicts detected",
            (Ja, ConflictDashHeader) => "conflict が検出されました",
            (En, ConflictRebaseApplying) => "Applying",
            (Ja, ConflictRebaseApplying) => "適用中",
            (En, ConflictRebaseCommits) => "commits",
            (Ja, ConflictRebaseCommits) => "commit",
            (En, ConflictRoleCurrent) => "Current",
            (Ja, ConflictRoleCurrent) => "現在の側",
            (En, ConflictRoleIncoming) => "Incoming",
            (Ja, ConflictRoleIncoming) => "取り込む側",
            (En, ConflictGitTermHint) => "Side of the conflict",
            (Ja, ConflictGitTermHint) => "conflict の側",
            (En, ConflictConflictedCount) => "conflicted",
            (Ja, ConflictConflictedCount) => "conflicted",
            (En, ConflictResolvedCount) => "resolved",
            (Ja, ConflictResolvedCount) => "resolved",
            (En, ConflictSectionConflicted) => "Conflicted Files",
            (Ja, ConflictSectionConflicted) => "Conflicted ファイル",
            (En, ConflictSectionResolved) => "Resolved Files",
            (Ja, ConflictSectionResolved) => "Resolved ファイル",
            (En, ConflictConfirmSkip) => "Confirm skip",
            (Ja, ConflictConfirmSkip) => "スキップを確定",
            (En, ConflictConfirmSkipButton) => "Confirm",
            (Ja, ConflictConfirmSkipButton) => "確定",
            (En, MenuConfirmSkip) => "Confirm Skip",
            (Ja, MenuConfirmSkip) => "スキップを確定",
            (En, MenuAbortOperation) => "Abort Operation…",
            (Ja, MenuAbortOperation) => "操作を中止…",
            (En, MenuAbortMerge) => "Abort Merge…",
            (Ja, MenuAbortMerge) => "merge を中止…",
            (En, MenuAbortRebase) => "Abort Rebase…",
            (Ja, MenuAbortRebase) => "rebase を中止…",
            (En, MenuAbortCherryPick) => "Abort Cherry-Pick…",
            (Ja, MenuAbortCherryPick) => "cherry-pick を中止…",
            (En, MenuAbortRevert) => "Abort Revert…",
            (Ja, MenuAbortRevert) => "revert を中止…",
            (En, MenuAbortStashApply) => "Abort Stash Apply…",
            (Ja, MenuAbortStashApply) => "stash apply を中止…",
            (En, ConflictConfirmSkipHint) => {
                "Skip drops this step's changes and your in-progress resolution.                  Click again to confirm."
            }
            (Ja, ConflictConfirmSkipHint) => {
                "スキップするとこのステップの変更と編集中の解決内容が失われます。                 もう一度クリックすると確定します。"
            }
            (En, ConflictAborted) => "Operation aborted",
            (Ja, ConflictAborted) => "操作を中止しました",
            (En, ConflictConfirmAbort) => "Confirm abort",
            (Ja, ConflictConfirmAbort) => "中止を確定",
            (En, OperationInProgress) => "in progress",
            (Ja, OperationInProgress) => "進行中",
            (En, ConflictBackToConflicts) => "Back to conflicts",
            (Ja, ConflictBackToConflicts) => "conflict に戻る",
            (En, ConflictExternalTool) => "Open in external tool",
            (Ja, ConflictExternalTool) => "外部ツールで開く",
            (En, ConflictExternalToolUnset) => {
                "No external merge tool is configured. Set \"mergetool\" in settings.json with $LOCAL/$BASE/$REMOTE/$MERGED placeholders."
            }
            (Ja, ConflictExternalToolUnset) => {
                "外部 merge tool が未設定です。settings.json の \"mergetool\" に $LOCAL/$BASE/$REMOTE/$MERGED を含むコマンドを設定してください。"
            }
            (En, ConflictOpenTerminal) => "Open terminal at repo root",
            (Ja, ConflictOpenTerminal) => "リポジトリのターミナルを開く",
            (En, ConflictCopyPath) => "Copy conflict file path",
            (Ja, ConflictCopyPath) => "conflict ファイルのパスをコピー",
            (En, ConflictCopyGitCommand) => "Copy git command",
            (Ja, ConflictCopyGitCommand) => "git コマンドをコピー",
            (En, ConflictBlockerUnresolved) => "Some files are still unresolved.",
            (Ja, ConflictBlockerUnresolved) => "未解決のファイルがあります。",
            (En, ConflictBlockerMarker) => "Conflict markers remain in a resolved file.",
            (Ja, ConflictBlockerMarker) => "解決済みファイルに conflict marker が残っています。",
            (En, ConflictBlockerBinary) => "A binary conflict still needs a side chosen.",
            (Ja, ConflictBlockerBinary) => "binary conflict の側が未選択です。",
            (En, ConflictBlockerDeletion) => "A keep-or-delete decision is still pending.",
            (Ja, ConflictBlockerDeletion) => "keep / delete の判断が未了です。",
            (En, ConflictBlockerIndex) => "Some unmerged files are not shown in this list. Refresh the repository.",
            (Ja, ConflictBlockerIndex) => "一覧に表示されていない未 merge のファイルがあります。リポジトリを再読み込みしてください。",
            (En, ConflictBlockerMessage) => "The merge commit message is empty.",
            (Ja, ConflictBlockerMessage) => "merge commit のメッセージが空です。",
            (En, ConflictBlockerChecklist) => "A commit checklist rule is blocking continue.",
            (Ja, ConflictBlockerChecklist) => "commit checklist のルールが continue を妨げています。",
            (En, ConflictContinueReady) => "All conflicts resolved — ready to continue.",
            (Ja, ConflictContinueReady) => "すべて解決済み。continue できます。",
            (En, ConflictMore) => "More",
            (Ja, ConflictMore) => "その他",
            (En, ConflictNextConflict) => "Next conflict",
            (Ja, ConflictNextConflict) => "次の conflict へ",
            (En, ConflictNoConflictedFiles) => "No conflicted files remain.",
            (Ja, ConflictNoConflictedFiles) => "未解決ファイルはありません。",
            (En, ConflictNoResolvedFiles) => "No files resolved yet.",
            (Ja, ConflictNoResolvedFiles) => "まだ解決済みのファイルはありません。",
            // ── Branch-name / worktree-path validation ───────────────
            (En, BranchNameEmpty) => "Branch name must not be empty.",
            (Ja, BranchNameEmpty) => "branch 名を入力してください。",
            (En, BranchNameRequired) => "Branch name is required.",
            (Ja, BranchNameRequired) => "branch 名を入力してください。",
            (En, BranchNameWhitespace) => "Branch name must not start or end with whitespace.",
            (Ja, BranchNameWhitespace) => "branch 名の先頭・末尾に空白は使えません。",
            (En, BranchNameSame) => "Branch already has that name.",
            (Ja, BranchNameSame) => "branch は既にその名前です。",
            (En, WorktreePathEmpty) => "Worktree path must not be empty.",
            (Ja, WorktreePathEmpty) => "worktree のパスを入力してください。",

            // ── Misc UI prose sweep ──────────────────────────────────
            (En, NoFileChanges) => "No file changes",
            (Ja, NoFileChanges) => "ファイルの変更はありません",
            (En, DiffUnavailable) => "(diff unavailable)",
            (Ja, DiffUnavailable) => "(diff を取得できません)",
            (En, CoAuthoredBy) => "Co-authored by",
            (En, Trailers) => "Trailers",
            (En, CommitTitle) => "Summary",
            (Ja, CommitTitle) => "概要",
            (En, CommitBody) => "Description (optional)",
            (Ja, CommitBody) => "説明(任意)",
            (En, CompactCommitAffordance) => "Commit…",
            (Ja, CompactCommitAffordance) => "Commit…",
            (En, CompactCommitComposer) => "Commit message",
            (Ja, CompactCommitComposer) => "Commit メッセージ",
            (En, CompactCommitClose) => "Close commit composer",
            (Ja, CompactCommitClose) => "Commit の入力を閉じる",
            (En, CommitDraftSaveFailed) => "Draft could not be saved",
            (Ja, CommitDraftSaveFailed) => "下書きを保存できません",
            (En, CommitDraftSaveRetry) => "Save Draft",
            (Ja, CommitDraftSaveRetry) => "下書きを保存",
            (En, GenerateMessage) => "Generate commit message",
            (Ja, GenerateMessage) => "commit メッセージを生成",
            (En, SmartDetecting) => "Detecting Smart Commit providers…",
            (Ja, SmartDetecting) => "Smart Commit のプロバイダーを検出中…",
            (En, AddCoAuthor) => "Add co-author",
            (Ja, AddCoAuthor) => "共同作成者を追加",
            (En, AmendLastCommit) => "Amend last commit",
            (Ja, AmendLastCommit) => "直前の commit を修正",
            (En, NoRecentAuthors) => "No other authors in recent history",
            (En, EditorUseAll) => "Use all",
            (En, MenuOpenInEditor) => "Open in Editor",
            (Ja, MenuOpenInEditor) => "エディタで開く",
            (En, MenuShowFileHistory) => "Show File History",
            (En, A11yInspectorFiles) => "Changed files",
            (Ja, A11yInspectorFiles) => "変更されたファイル",
            (En, A11yFileHistory) => "File History",
            (Ja, A11yFileHistory) => "ファイルの履歴",
            (En, OpenInExternalEditor) => "Open in External Editor",
            (En, DiffStageHunk) => "Stage hunk",
            (En, DiffUnstageHunk) => "Unstage hunk",
            (En, PrBranchNotFetched) => "Branch not fetched",
            (Ja, PrBranchNotFetched) => "branch が未取得です",
            (En, PrOpenOnGitHub) => "Open on GitHub",
            (Ja, PrOpenOnGitHub) => "GitHub で開く",
            (En, PrCopyUrl) => "Copy PR URL",
            (Ja, PrCopyUrl) => "PR の URL をコピー",
            (En, PrPeek) => "Peek changes",
            (Ja, PrPeek) => "変更を覗く",
            (En, PrJumpToBranch) => "Jump to branch",
            (Ja, PrJumpToBranch) => "branch へジャンプ",
            (En, PrDraft) => "draft",
            (Ja, PrDraft) => "ドラフト",
            (En, PrPaneTitle) => "Pull Requests",
            (Ja, PrPaneTitle) => "pull request",
            (En, PrPaneEmpty) => "No pull requests match this filter.",
            (Ja, PrPaneEmpty) => "このフィルタに該当する PR はありません。",
            (En, PrReviewApproved) => "approved",
            (Ja, PrReviewApproved) => "承認済",
            (En, PrReviewChanges) => "changes",
            (Ja, PrReviewChanges) => "要修正",
            (En, PrReviewRequired) => "pending",
            (Ja, PrReviewRequired) => "待ち",
            (En, PrModeExit) => "Exit",
            (Ja, PrModeExit) => "終了",
            (En, PrModeSelectHint) => "Select a pull request",
            (Ja, PrModeSelectHint) => "PR を選択してください",
            (En, PrModeNoFile) => "No file selected",
            (Ja, PrModeNoFile) => "ファイル未選択",
            (En, PrModeFiles) => "FILES",
            (Ja, PrModeFiles) => "ファイル",
            (En, PrModeShowDescription) => "Show description",
            (En, PrModeDescription) => "DESCRIPTION",
            (Ja, PrModeDescription) => "説明",
            (En, PrModeCommits) => "COMMITS",
            (Ja, PrModeCommits) => "commit",
            (En, PrQueueNeedsYou) => "NEEDS YOU",
            (Ja, PrQueueNeedsYou) => "対応が必要",
            (En, PrQueuePending) => "DETERMINING",
            (Ja, PrQueuePending) => "判定待ち",
            (En, PrQueueInProgress) => "IN PROGRESS",
            (Ja, PrQueueInProgress) => "進行中",
            (En, PrQueueReady) => "READY",
            (Ja, PrQueueReady) => "merge 可能",
            (En, PrQueueWaiting) => "WAITING",
            (Ja, PrQueueWaiting) => "待ち",
            (En, PrQueueDormant) => "OTHERS",
            (Ja, PrQueueDormant) => "その他",
            (En, PrSectionInbox) => "INBOX",
            (Ja, PrSectionInbox) => "対応が必要",
            (En, PrSectionMine) => "MY PRS",
            (Ja, PrSectionMine) => "自分の PR",
            (En, PrSectionReview) => "REVIEW",
            (Ja, PrSectionReview) => "レビュー依頼",
            (En, PrSectionAssigned) => "ASSIGNED",
            (Ja, PrSectionAssigned) => "担当",
            (En, PrLaneTitle) => "Graph",
            (Ja, PrLaneTitle) => "Graph",
            (En, PrLaneLoaded) => "{} loaded",
            (Ja, PrLaneLoaded) => "{} 読み込み済み",
            (En, PrLaneCommit) => "{}; SHA {}; PR #{}",
            (Ja, PrLaneCommit) => "{}、SHA {}、PR #{}",
            (En, PrLaneContext) => "{}; SHA {}; read-only context",
            (Ja, PrLaneContext) => "{}、SHA {}、読み取り専用の周辺履歴",
            (En, PrSwimlane) => "Graph",
            (Ja, PrSwimlane) => "Graph",
            (En, PrShowNavigator) => "Show PR sidebar",
            (Ja, PrShowNavigator) => "PR サイドバーを表示",
            (En, PrHideNavigator) => "Hide PR sidebar",
            (Ja, PrHideNavigator) => "PR サイドバーを非表示",
            (En, PrShowGraph) => "Show Graph",
            (Ja, PrShowGraph) => "Graph を表示",
            (En, PrHideGraph) => "Hide Graph",
            (Ja, PrHideGraph) => "Graph を非表示",
            (En, NoPrOpen) => "No PR is open",
            (Ja, NoPrOpen) => "開いている PR がありません",
            (En, PrRailReviewers) => "REVIEWERS",
            (Ja, PrRailReviewers) => "レビュアー",
            (En, PrRailAssignees) => "ASSIGNEES",
            (Ja, PrRailAssignees) => "担当",
            (En, PrRailLabels) => "LABELS",
            (Ja, PrRailLabels) => "ラベル",
            (En, PrRailWorktree) => "WORKTREE",
            (Ja, PrRailWorktree) => "WORKTREE",
            (En, PrRailWorktreeClean) => "checked out · clean",
            (Ja, PrRailWorktreeClean) => "checkout 済み・変更なし",
            (En, PrRailNone) => "None",
            (Ja, PrRailNone) => "なし",
            (En, PrHomeOpen) => "OPEN",
            (Ja, PrHomeOpen) => "OPEN",
            (En, PrHomeDraft) => "DRAFT",
            (Ja, PrHomeDraft) => "DRAFT",
            (En, PrHomeSortUpdated) => "SORT: UPDATED",
            (Ja, PrHomeSortUpdated) => "並び: 更新順",
            (En, PrHomeSortCreated) => "SORT: CREATED",
            (Ja, PrHomeSortCreated) => "並び: 作成順",
            (En, PrHomeSortNumber) => "SORT: NUMBER",
            (Ja, PrHomeSortNumber) => "並び: 番号順",
            (En, PrColNo) => "NO",
            (Ja, PrColNo) => "NO",
            (En, PrColTitle) => "TITLE / BRANCH",
            (Ja, PrColTitle) => "TITLE / BRANCH",
            (En, PrColState) => "STATE",
            (Ja, PrColState) => "STATE",
            (En, PrColAuthor) => "AUTHOR",
            (Ja, PrColAuthor) => "AUTHOR",
            (En, PrColChecks) => "CHECKS",
            (Ja, PrColChecks) => "CHECKS",
            (En, PrColFiles) => "FILES",
            (Ja, PrColFiles) => "FILES",
            (En, PrColAge) => "AGE",
            (Ja, PrColAge) => "AGE",
            (En, PrWhyChangesRequested) => "Changes requested",
            (Ja, PrWhyChangesRequested) => "修正依頼",
            (En, PrWhyConflicting) => "Conflicts",
            (Ja, PrWhyConflicting) => "conflict",
            (En, PrWhyCiRunning) => "CI running",
            (Ja, PrWhyCiRunning) => "CI 実行中",
            (En, PrWhyReadyToMerge) => "Ready to merge",
            (Ja, PrWhyReadyToMerge) => "merge 可能",
            (En, PrWhyReviewRequested) => "Your review requested",
            (Ja, PrWhyReviewRequested) => "レビュー依頼",
            (En, PrWhyAwaitingReview) => "Awaiting review",
            (Ja, PrWhyAwaitingReview) => "レビュー待ち",
            (En, PrWhyPending) => "Determining",
            (Ja, PrWhyPending) => "判定待ち",
            (En, PrModeChecks) => "CHECKS",
            (Ja, PrModeChecks) => "チェック",
            (En, PrModeReview) => "REVIEW",
            (Ja, PrModeReview) => "レビュー",
            (En, PrModeOverview) => "Overview",
            (Ja, PrModeOverview) => "概要",
            (En, PrModeNoReview) => "No reviews or comments yet.",
            (Ja, PrModeNoReview) => "レビュー・コメントはまだありません。",
            (En, PrChecksPassed) => "All checks have passed",
            (Ja, PrChecksPassed) => "チェックはすべて成功",
            (En, PrChecksFailed) => "Some checks failed",
            (Ja, PrChecksFailed) => "失敗したチェックがあります",
            (En, PrChecksRunning) => "Checks are still running",
            (Ja, PrChecksRunning) => "チェックを実行中",
            (En, PrChecksOpenInBrowser) => "OPEN IN BROWSER",
            (Ja, PrChecksOpenInBrowser) => "ブラウザで開く",
            (En, PrReviewSubmit) => "Review",
            (Ja, PrReviewSubmit) => "レビュー",
            (En, PrReviewApprove) => "APPROVE",
            (Ja, PrReviewApprove) => "承認",
            (En, PrReviewRequestChanges) => "REQUEST CHANGES",
            (Ja, PrReviewRequestChanges) => "修正を依頼",
            (En, PrFieldsFilterPlaceholder) => "Type to filter…",
            (Ja, PrFieldsFilterPlaceholder) => "絞り込み…",
            (En, PrFieldsApply) => "Apply",
            (Ja, PrFieldsApply) => "適用",
            (En, PrEditApply) => "Edit",
            (Ja, PrEditApply) => "編集",
            (En, PrCommentPlaceholder) => "Leave a comment…",
            (Ja, PrCommentPlaceholder) => "コメントを書く…",
            (En, PrCommentPost) => "COMMENT",
            (Ja, PrCommentPost) => "コメント",
            (En, IssueCompose) => "What would you like to build or fix?",
            (Ja, IssueCompose) => "作りたいこと、直したいことを書く…",
            (En, IssueComposeEmpty) => "What do you want to fix?",
            (Ja, IssueComposeEmpty) => "何をなおしたい？",
            (En, IssueTitleOptional) => "Title (optional — defaults to the first line)",
            (Ja, IssueTitleOptional) => "タイトル（省略時は本文の先頭行）",
            (En, IssueCreate) => "Create",
            (Ja, IssueCreate) => "作成",
            (En, HomeTabTitle) => "Home",
            (Ja, HomeTabTitle) => "ホーム",
            (En, HomeTitle) => "Open a repository",
            (Ja, HomeTitle) => "リポジトリを開く",
            (En, HomeRecent) => "Recently opened",
            (Ja, HomeRecent) => "最近開いたリポジトリ",
            (En, HomeNoRecent) => "No recently opened repositories.",
            (Ja, HomeNoRecent) => "最近開いたリポジトリはありません。",
            (En, HomeRemoveRecent) => "Remove from Recently Opened",
            (Ja, HomeRemoveRecent) => "最近使った項目から取り除く",
            (En, HomeOpenFolder) => "Open Folder…",
            (Ja, HomeOpenFolder) => "フォルダーを開く…",
            (En, HomeConnectRemote) => "Connect to SSH remote…",
            (Ja, HomeConnectRemote) => "SSH remote に接続…",
            (En, HomeGithubMine) => "Yours",
            (Ja, HomeGithubMine) => "自分",
            (En, HomeGithubOwnerFailed) => "Could not be read: {}",
            (Ja, HomeGithubOwnerFailed) => "読み込めませんでした: {}",
            (En, HomeGithubFilter) => "Filter…",
            (Ja, HomeGithubFilter) => "絞り込む…",
            (En, HomeGithubRefresh) => "Refresh",
            (Ja, HomeGithubRefresh) => "再読み込み",
            (En, HomeGithubLoading) => "Loading your repositories from GitHub…",
            (Ja, HomeGithubLoading) => "GitHub からリポジトリを読み込んでいます…",
            (En, HomeGithubOrgsLoading) => "Loading your organizations' repositories…",
            (Ja, HomeGithubOrgsLoading) => "organization のリポジトリを読み込んでいます…",
            (En, HomeGithubUpdating) => "Updating…",
            (Ja, HomeGithubUpdating) => "更新中…",
            (En, HomeGithubOrgs) => "Organizations",
            (Ja, HomeGithubOrgs) => "organization",
            (En, HomeGithubOrgsFailed) => "Could not list your organizations: {}",
            (Ja, HomeGithubOrgsFailed) => "所属 organization の一覧を読めませんでした: {}",
            (En, HomeGithubRefreshFailed) => {
                "Could not refresh the GitHub list (showing the last one): {}"
            }
            (Ja, HomeGithubRefreshFailed) => {
                "GitHub の一覧を更新できませんでした(前回の一覧を表示しています): {}"
            }
            (En, HomeGithubFailedHint) => {
                "Check that the GitHub CLI is signed in (`gh auth status`), then Refresh."
            }
            (Ja, HomeGithubFailedHint) => {
                "GitHub CLI にサインインしているか(`gh auth status`)確認してから再読み込みしてください。"
            }
            (En, HomeGithubSectionEmpty) => "No repositories to display for {owner} on {host}.",
            (Ja, HomeGithubSectionEmpty) => "{host} の {owner} には表示できるリポジトリがありません。",
            (En, HomeGithubMineEmpty) => "You have no repositories to display on {host}.",
            (Ja, HomeGithubMineEmpty) => "{host} に表示できるあなたのリポジトリはありません。",
            (En, HomeGithubNoMatch) => "No repository matches the filter.",
            (Ja, HomeGithubNoMatch) => "絞り込みに一致するリポジトリはありません。",
            (En, HomeGithubTruncated) => {
                "Showing the first {} — more exist on GitHub. Filter, or open the repository by folder."
            }
            (Ja, HomeGithubTruncated) => {
                "先頭の {} 件を表示しています。GitHub にはさらにあります。絞り込むか、フォルダーから開いてください。"
            }
            (En, HomeGithubOpen) => "Open",
            (Ja, HomeGithubOpen) => "開く",
            (En, HomeGithubClone) => "Clone",
            (Ja, HomeGithubClone) => "clone",
            (En, HomeGithubCloning) => "Cloning…",
            (Ja, HomeGithubCloning) => "clone 中…",
            (En, HomePaneRepos) => "Repositories",
            (Ja, HomePaneRepos) => "リポジトリ",
            (En, HomePanePrs) => "Pull requests",
            (Ja, HomePanePrs) => "Pull Request",
            (En, HomePaneIssues) => "Issues",
            (Ja, HomePaneIssues) => "Issue",
            (En, HomeWorkMine) => "Created by you",
            (Ja, HomeWorkMine) => "自分の PR",
            (En, HomeWorkReview) => "Review requested",
            (Ja, HomeWorkReview) => "レビュー依頼",
            (En, HomeWorkNoPrs) => "No open pull requests",
            (Ja, HomeWorkNoPrs) => "Open な PR はありません",
            (En, HomeWorkNoReview) => "No review requests",
            (Ja, HomeWorkNoReview) => "レビュー依頼はありません",
            (En, HomeWorkNoIssues) => "No issues assigned",
            (Ja, HomeWorkNoIssues) => "担当の Issue はありません",
            (En, HomeWorkNoMatch) => "Nothing matches the filter",
            (Ja, HomeWorkNoMatch) => "絞り込みに一致するものはありません",
            (En, HomeWorkTruncated) => "More than 100",
            (Ja, HomeWorkTruncated) => "100 件以上",
            (En, HomeWorkOpenOnGithub) => "Open on GitHub",
            (Ja, HomeWorkOpenOnGithub) => "GitHub で開く",
            (En, HomeWorkLoading) => "Loading…",
            (Ja, HomeWorkLoading) => "読み込み中…",
            (En, HomeWorkFailed) => "Could not load: {}",
            (Ja, HomeWorkFailed) => "読み込めませんでした: {}",
            (En, HomeWorkOpenFailed) => "Could not open #{}: {}",
            (Ja, HomeWorkOpenFailed) => "#{} を開けませんでした: {}",
            (En, HomeWorkOtherRepo) => "#{} is in {}; this clone's GitHub repository is {}",
            (Ja, HomeWorkOtherRepo) => "#{} は {} の Issue です。この clone の GitHub リポジトリは {} です",
            (En, HomeGithubPrivate) => "Private",
            (Ja, HomeGithubPrivate) => "非公開",
            (En, HomeGithubFork) => "Fork",
            (Ja, HomeGithubFork) => "Fork",
            (En, CloneTitle) => "Clone {}",
            (Ja, CloneTitle) => "{} を clone",
            (En, CloneLocation) => "Location",
            (Ja, CloneLocation) => "保存先",
            (En, CloneNoLocation) => "Choose a folder to clone into",
            (Ja, CloneNoLocation) => "clone 先のフォルダーを選んでください",
            (En, CloneWillCreate) => "Creates {}",
            (Ja, CloneWillCreate) => "{} を作成します",
            (En, CloneChooseFolder) => "Choose…",
            (Ja, CloneChooseFolder) => "選択…",
            (En, CloneChangeFolder) => "Change…",
            (Ja, CloneChangeFolder) => "変更…",
            (En, CloneCopyLocation) => "Copy Location",
            (Ja, CloneCopyLocation) => "保存先をコピー",
            (En, CloneConfirm) => "Clone",
            (Ja, CloneConfirm) => "clone",
            (En, CloneBusy) => "A clone is already running in this window. Wait for it to finish.",
            (Ja, CloneBusy) => "このウィンドウでは別の clone を実行中です。終わるまで待ってください。",
            (En, CloneRunning) => "Cloning… {}",
            (Ja, CloneRunning) => "clone 中… {}",
            (En, CloneRunningHint) => {
                "Downloading from {}. A large repository can take a few minutes; closing this card does not stop the clone."
            }
            (Ja, CloneRunningHint) => {
                "{} からダウンロードしています。大きなリポジトリでは数分かかることがあります。このカードを閉じても clone は続きます。"
            }
            (En, CloneHide) => "Continue in background",
            (Ja, CloneHide) => "バックグラウンドで続ける",
            (En, CloneHiddenRunning) => "Cloning {}…",
            (Ja, CloneHiddenRunning) => "{} を clone 中…",
            (En, CloneShowProgress) => "Show Progress",
            (Ja, CloneShowProgress) => "進捗を表示",
            (En, IssuePostedAs) => "Posted as {}",
            (Ja, IssuePostedAs) => "{} として投稿",
            (En, IssueNewFieldsTitle) => "New issue",
            (Ja, IssueNewFieldsTitle) => "新しい Issue",
            (En, IssueReply) => "Reply",
            (Ja, IssueReply) => "返信",
            (En, ComposerWrite) => "Write",
            (Ja, ComposerWrite) => "編集",
            (En, ComposerPreview) => "Preview",
            (Ja, ComposerPreview) => "プレビュー",
            (En, IssueFocusEditor) => "Focus Editor",
            (Ja, IssueFocusEditor) => "エディタを拡大",
            (En, IssueExitFocus) => "Exit Focus Editor",
            (Ja, IssueExitFocus) => "通常表示に戻る",
            (En, ComposerDraftSaved) => "Draft saved",
            (Ja, ComposerDraftSaved) => "下書き保存済み",
            (En, ComposerDraftSaving) => "Saving draft…",
            (Ja, ComposerDraftSaving) => "下書き保存中…",
            (En, IssueRepoUnavailable) => "GitHub repository unavailable",
            (Ja, IssueRepoUnavailable) => "GitHub リポジトリを取得できません",
            (En, PrModeMerge) => "Merge",
            (Ja, PrModeMerge) => "merge",
            (En, PrModeMergeDone) => "Merged",
            (Ja, PrModeMergeDone) => "merge しました",
            (En, PrSuggestion) => "suggestion",
            (En, PrApplySuggestion) => "Apply suggestion…",
            (En, PrHunkCopy) => "Copy this hunk",
            (Ja, PrHunkCopy) => "このコードをコピー",
            (En, PrModeConflicts) => "Conflicts",
            (Ja, PrModeConflicts) => "Conflicts",
            (En, PrConflictsNone) => "No conflicts against the base as it is now.",
            (Ja, PrConflictsNone) => "現在の base に対して conflict はありません。",
            (En, PrConflictsHint) => "What merging this PR would conflict on, computed locally. Nothing here changes your repository.",
            (Ja, PrConflictsHint) => "この PR を merge した場合に conflict する内容です。ローカルで計算しており、リポジトリには一切変更を加えません。",
            (En, PrConflictDeleteModify) => "deleted on one side, changed on the other",
            (Ja, PrConflictDeleteModify) => "片方で削除、もう片方で変更",
            (En, PrConflictBothAdded) => "added on both sides",
            (Ja, PrConflictBothAdded) => "両方で追加",
            (En, PrConflictBinary) => "binary file, changed on both sides",
            (Ja, PrConflictBinary) => "binary ファイル、両方で変更",
            (En, PrConflictHunk) => "conflict",
            (Ja, PrConflictHunk) => "conflict",
            (En, PrConflictTooLarge) => "conflict too large to display",
            (Ja, PrConflictTooLarge) => "conflict が大きすぎるため表示できません",
            (En, PrHunkCopied) => "Hunk copied",
            (Ja, PrHunkCopied) => "コードをコピーしました",
            (Ja, PrSuggestion) => "コード提案",
            (Ja, PrApplySuggestion) => "提案を適用…",
            (Ja, PrModeShowDescription) => "説明文を表示",
            (En, PrModeNoDescription) => "No description.",
            (Ja, PrModeNoDescription) => "説明文はありません。",
            (En, PrMergeStatusHeading) => "Merge status",
            (Ja, PrMergeStatusHeading) => "merge の状態",
            (En, PrMergeActionConflict) => "Conflicts with the base — open the conflict view.",
            (Ja, PrMergeActionConflict) => "base と conflict しています。確認してください。",
            (En, PrMergeActionUpdateBranch) => "Behind the base — rebase or update the branch.",
            (Ja, PrMergeActionUpdateBranch) => "base に遅れています。rebase か update してください。",
            (En, PrMergeActionBlocked) => "Blocked — required checks are not yet met:",
            (Ja, PrMergeActionBlocked) => "blocked。必要な条件が未達です:",
            (En, PrMergeActionUnstable) => "A non-required check is failing.",
            (Ja, PrMergeActionUnstable) => "必須でない check が失敗しています。",
            (En, PrMergeActionReady) => "Ready to merge.",
            (Ja, PrMergeActionReady) => "merge できます。",
            (En, PrMergeActionDraft) => "Draft — mark ready for review first.",
            (Ja, PrMergeActionDraft) => "draft です。まず ready にしてください。",
            (En, PrMergeActionWait) => "Waiting on GitHub to compute mergeability.",
            (Ja, PrMergeActionWait) => "GitHub が mergeability を計算中です。",
            (En, PrMergeMissingApprovals) => "approving reviews still needed",
            (Ja, PrMergeMissingApprovals) => "件の approve が必要",
            (En, PrMergeMissingCodeowner) => "CODEOWNERS review required from",
            (Ja, PrMergeMissingCodeowner) => "CODEOWNERS のレビューが必要:",
            (En, PrMergeUnresolvedThreads) => "unresolved review threads",
            (Ja, PrMergeUnresolvedThreads) => "件の未解決スレッド",
            (En, PrMergeQueueLabel) => "In merge queue — position",
            (Ja, PrMergeQueueLabel) => "merge queue 内・順位",
            (En, WorkspaceGraph) => "Graph",
            (Ja, WorkspaceGraph) => "Graph",
            (En, WorkspacePrs) => "PRs",
            (Ja, WorkspacePrs) => "PRs",
            (En, WorkspaceIssues) => "Issues",
            (Ja, WorkspaceIssues) => "Issues",
            (En, IssuesLoadingList) => "Loading issues…",
            (Ja, IssuesLoadingList) => "Issues を読み込み中…",
            (En, IssuesLoadingMore) => "Loading more issues…",
            (Ja, IssuesLoadingMore) => "さらに読み込み中…",
            (En, IssuesRetryLoadMore) => "Retry loading more",
            (Ja, IssuesRetryLoadMore) => "追加の読み込みを再試行",
            (En, ListFilterAll) => "All",
            (Ja, ListFilterAll) => "すべて",
            (En, ListFilterTitle) => "Filter titles…",
            (Ja, ListFilterTitle) => "タイトルを絞り込み…",
            (En, ListFilterReady) => "Ready",
            (Ja, ListFilterReady) => "レビュー可能",
            (En, ListFilterPassing) => "Passing",
            (Ja, ListFilterPassing) => "成功",
            (En, ListFilterFailing) => "Failing",
            (Ja, ListFilterFailing) => "失敗",
            (En, ListFilterSort) => "Sort",
            (Ja, ListFilterSort) => "並び替え",
            (En, ListSortComments) => "SORT: COMMENTS",
            (Ja, ListSortComments) => "並び: コメント数",
            (En, ListSortAscending) => "Ascending",
            (Ja, ListSortAscending) => "昇順",
            (En, ListSortDescending) => "Descending",
            (Ja, ListSortDescending) => "降順",
            (En, ListFilterClear) => "Clear filters",
            (Ja, ListFilterClear) => "絞り込みを解除",
            (En, ListLoadMore) => "Load more",
            (Ja, ListLoadMore) => "さらに読み込む",
            (En, PrLoadingMore) => "Loading more pull requests…",
            (Ja, PrLoadingMore) => "さらに読み込み中…",
            (En, ListLoadedCount) => "Loaded {}",
            (Ja, ListLoadedCount) => "読み込み済み {} 件",
            (En, ListMatchesCount) => "{} matches",
            (Ja, ListMatchesCount) => "一致 {} 件",
            (En, IssuesEmpty) => "No issues",
            (Ja, IssuesEmpty) => "Issues はありません",
            (En, IssuesFilterEmpty) => "No issues in this view",
            (Ja, IssuesFilterEmpty) => "この一覧に Issue はありません",
            (En, IssuesViewerUnknown) => "Your account on this server is not known yet",
            (Ja, IssuesViewerUnknown) => "このサーバーでのあなたのアカウントがまだ分かりません",
            (En, IssuesRefreshing) => "Refreshing issues…",
            (Ja, IssuesRefreshing) => "Issues を更新中…",
            (En, IssuesAssignedToMe) => "Assigned to me",
            (Ja, IssuesAssignedToMe) => "自分の担当",
            (En, IssuesCreatedByMe) => "Created by me",
            (Ja, IssuesCreatedByMe) => "自分が作成",
            (En, IssuesMentioningMe) => "Mentioning me",
            (Ja, IssuesMentioningMe) => "自分への mention",
            (En, IssuesRecentlyUpdated) => "Recently updated",
            (Ja, IssuesRecentlyUpdated) => "最近更新",
            (En, IssueStateOpen) => "Open",
            (Ja, IssueStateOpen) => "Open",
            (En, IssueStateClosed) => "Closed",
            (Ja, IssueStateClosed) => "Closed",
            (En, IssueStateUnknown) => "Unknown",
            (Ja, IssueStateUnknown) => "不明",
            (En, IssueSelect) => "Select an issue",
            (Ja, IssueSelect) => "Issue を選択してください",
            (En, IssueBackToList) => "← Issues",
            (Ja, IssueBackToList) => "Issues に戻る",
            (En, IssueLoading) => "Loading issue…",
            (Ja, IssueLoading) => "Issue を読み込み中…",
            (En, IssueDetailsUnavailable) => "Issue details unavailable",
            (Ja, IssueDetailsUnavailable) => "Issue の詳細を取得できません",
            (En, IssueNoDescription) => "No description provided.",
            (Ja, IssueNoDescription) => "説明はありません。",
            (En, PrAllPrs) => "All PRs",
            (Ja, PrAllPrs) => "PR 一覧",
            (En, PrStack) => "STACK",
            (Ja, PrStack) => "スタック",
            (En, PrFetchFailed) => "Couldn't reach GitHub — this is not an empty list.",
            (Ja, PrFetchFailed) => "GitHub に接続できませんでした(PR が 0 件なのではありません)。",
            (En, PrFetchStale) => "Showing the last successful fetch — the newest one failed.",
            (Ja, PrFetchStale) => "前回取得できた一覧を表示しています(最新の取得は失敗しました)。",
            (En, PrFetchAuth) => "GitHub authentication failed — run `gh auth login`.",
            (Ja, PrFetchAuth) => "GitHub の認証に失敗しました(`gh auth login` を実行してください)。",
            (En, PrFetchNetwork) => "Couldn't reach GitHub. Check your network connection.",
            (Ja, PrFetchNetwork) => "GitHub に接続できませんでした。ネットワーク接続を確認してください。",
            (En, PrFetchInvalid) => "GitHub returned a response kagi could not read.",
            (Ja, PrFetchInvalid) => "GitHub の応答を解析できませんでした。",
            (En, PrFetchUnknown) => "The GitHub fetch failed for an unknown reason.",
            (Ja, PrFetchUnknown) => "GitHub の取得が不明な理由で失敗しました。",
            (En, PrGithubUnavailable) => "No GitHub remote is configured for this repository.",
            (Ja, PrGithubUnavailable) => "この repository に GitHub remote が設定されていません。",
            (En, CleanupPrEvidenceStale) => "PR column may be out of date — the GitHub fetch failed.",
            (Ja, CleanupPrEvidenceStale) => "PR 列は最新でない可能性があります(GitHub 取得に失敗)。",
            (En, PrRefresh) => "Refresh",
            (Ja, PrRefresh) => "更新",
            (En, PrRefreshing) => "Refreshing pull requests…",
            (Ja, PrRefreshing) => "pull request を更新中…",
            (Ja, OpenInExternalEditor) => "外部エディタで開く",
            (Ja, DiffStageHunk) => "hunk を stage",
            (Ja, DiffUnstageHunk) => "hunk を unstage",
            (Ja, MenuShowFileHistory) => "ファイルの履歴を表示",
            (Ja, EditorUseAll) => "すべて採用",
            (En, ToggleCommitTemplate) => "Use your commit.template",
            (Ja, ToggleCommitTemplate) => "commit.template を使う",
            (En, Template) => "Template",
            (Ja, Template) => "テンプレート",
            (Ja, NoRecentAuthors) => "最近の履歴に他の作成者がいません",
            (Ja, CoAuthoredBy) => "共同作成者",
            (Ja, Trailers) => "トレーラー",
            (En, Ready) => "Ready",
            (Ja, Ready) => "準備完了",
            (En, NoUpstreamSet) => "No upstream set",
            (Ja, NoUpstreamSet) => "upstream が設定されていません",

            // ── Merge-into-conflict (W31-MERGE-INTO-CONFLICT) ────────
            (En, MergeAndResolveConflicts) => "Merge and resolve conflicts",
            (Ja, MergeAndResolveConflicts) => "merge して conflict を解決",
            (En, MergeConflictWarning) => {
                "This merge will produce conflicts. It will leave conflict markers and enter Conflict Mode, where you resolve each file (or abort to restore the pre-merge state)."
            }
            (Ja, MergeConflictWarning) => {
                "この merge は conflict を発生させます。conflict marker を残して Conflict Mode に入り、各ファイルを解決します(中止すれば merge 前の状態に戻せます)。"
            }

            // ── T-SETTINGS-001: Settings window ──────────────────────
            (En, SettingsTitle) => "Settings",
            (Ja, SettingsTitle) => "設定",
            (En, SettingsGeneral) => "General",
            (Ja, SettingsGeneral) => "一般",
            (En, SettingsWindowFailed) => "Could not open Settings",
            (Ja, SettingsWindowFailed) => "設定ウィンドウを開けませんでした",
            (En, SettingsAppearance) => "Appearance",
            (Ja, SettingsAppearance) => "外観",
            (En, SettingsGraph) | (Ja, SettingsGraph) => "Graph",
            (En, SettingsNetwork) => "Network",
            (Ja, SettingsNetwork) => "ネットワーク",
            (En, SettingsBehaviour) => "Behaviour / Terminal",
            (Ja, SettingsBehaviour) => "動作 / Terminal",
            (En, SettingsLaneCompactSummary) => "Show author avatars and lane-colour bands.",
            (Ja, SettingsLaneCompactSummary) => "著者アバターとレーン色の帯を表示します。",
            (En, SettingsTerminalAutoLockSummary) => "Offer to lock linked worktrees; confirm each time.",
            (Ja, SettingsTerminalAutoLockSummary) => "linked worktree のロックを毎回確認して提案します。",
            (En, SettingsCustomThemesSummary) => "Load themes from JSON files.",
            (Ja, SettingsCustomThemesSummary) => "JSON ファイルからテーマを読み込みます。",
            (En, SettingsAnalyzeIgnoreSummary) => "Exclude matching files. Save, then reload to apply.",
            (Ja, SettingsAnalyzeIgnoreSummary) => "一致するファイルを除外します。保存後に再読み込みで反映します。",
            (En, SettingsGraphCopySummary) => "Copy Hash or Branch; Hash if no local branch.",
            (Ja, SettingsGraphCopySummary) => "Hash または Branch。local branch が無い行は Hash。",
            (En, SettingsSmartProviderSummary) => "Local Ollama or your installed CLI.",
            (Ja, SettingsSmartProviderSummary) => "ローカルの Ollama、またはインストール済み CLI。",
            (En, SettingsAnalyze) | (Ja, SettingsAnalyze) => "Analyze",
            (En, SettingsAutoFetchSummary) => "Keep remote history up to date in the background.",
            (Ja, SettingsAutoFetchSummary) => "remote の履歴をバックグラウンドで最新に保ちます。",
            (En, SettingsReduceMotionSummary) => "Keep decorative animations still.",
            (Ja, SettingsReduceMotionSummary) => "装飾的なアニメーションを静止させます。",
            (En, SettingsSmartEnableSummary) => "Draft commit messages with an LLM.",
            (Ja, SettingsSmartEnableSummary) => "LLM で commit メッセージの草案を作成します。",
            (En, SettingsLanguage) => "Language",
            (Ja, SettingsLanguage) => "言語",
            (En, SettingsTheme) => "Theme",
            (Ja, SettingsTheme) => "テーマ",
            (En, SettingsThemeDesc) => "Colour theme used across the whole app.",
            (Ja, SettingsThemeDesc) => "アプリ全体で使用するカラーテーマ。",
            (En, SettingsAccent) => "Accent",
            (Ja, SettingsAccent) => "アクセント",
            (En, SettingsAccentSystem) => "System",
            (Ja, SettingsAccentSystem) => "システム",
            (En, SettingsAccentTheme) => "Theme accent",
            (Ja, SettingsAccentTheme) => "テーマのアクセント",
            (En, SettingsAccentGraphite) => "Detected Mac accent: Graphite",
            (Ja, SettingsAccentGraphite) => "Mac のアクセント：グラファイト",
            (En, SettingsAccentRed) => "Detected Mac accent: Red",
            (Ja, SettingsAccentRed) => "Mac のアクセント：レッド",
            (En, SettingsAccentOrange) => "Detected Mac accent: Orange",
            (Ja, SettingsAccentOrange) => "Mac のアクセント：オレンジ",
            (En, SettingsAccentYellow) => "Detected Mac accent: Yellow",
            (Ja, SettingsAccentYellow) => "Mac のアクセント：イエロー",
            (En, SettingsAccentGreen) => "Detected Mac accent: Green",
            (Ja, SettingsAccentGreen) => "Mac のアクセント：グリーン",
            (En, SettingsAccentBlue) => "Detected Mac accent: Blue",
            (Ja, SettingsAccentBlue) => "Mac のアクセント：ブルー",
            (En, SettingsAccentPurple) => "Detected Mac accent: Purple",
            (Ja, SettingsAccentPurple) => "Mac のアクセント：パープル",
            (En, SettingsAccentPink) => "Detected Mac accent: Pink",
            (Ja, SettingsAccentPink) => "Mac のアクセント：ピンク",
            (En, SettingsAccentDefault) => "Mac accent: Default (Blue)",
            (Ja, SettingsAccentDefault) => "Mac のアクセント：既定（ブルー）",
            (En, SettingsCustomThemes) => "Custom themes",
            (Ja, SettingsCustomThemes) => "自作テーマ",
            (En, SettingsCustomThemesDesc) => "Add JSON files to the folder below, then reload the themes.",
            (Ja, SettingsCustomThemesDesc) => "下のフォルダーに JSON ファイルを追加し、テーマを再読み込みします。",
            (En, SettingsCustomThemesGuide) => "Theme file guide",
            (Ja, SettingsCustomThemesGuide) => "テーマファイルの書き方",
            (En, SettingsCustomThemesOpen) => "Open theme folder",
            (Ja, SettingsCustomThemesOpen) => "テーマフォルダーを開く",
            (En, SettingsCustomThemesReload) => "Reload Themes",
            (Ja, SettingsCustomThemesReload) => "テーマを再読み込み",
            (En, SettingsThemesFolderUnavailable) => "Theme folder path unavailable",
            (Ja, SettingsThemesFolderUnavailable) => "テーマフォルダーのパスが見つかりません",
            (En, SettingsZoom) => "UI Zoom",
            (Ja, SettingsZoom) => "UI ズーム",
            (En, SettingsZoomDesc) => "Scale all text and layout (0.7×–2.0×).",
            (Ja, SettingsZoomDesc) => "テキストとレイアウト全体を拡大縮小します(0.7×〜2.0×)。",
            (En, SettingsCompactThreshold) => "Compact layout threshold",
            (Ja, SettingsCompactThreshold) => "コンパクト表示への切替幅",
            (En, SettingsCompactThresholdDesc) => "Switch to compact below this width (40–160rem). rem is the root text size, not a literal character count; UI zoom changes the logical-pixel threshold.",
            (Ja, SettingsCompactThresholdDesc) => "この幅未満でコンパクト表示に切り替えます (40〜160rem)。rem は基準文字サイズの倍率で、実際の文字数ではありません。UI ズームに応じて logical px 換算の切替幅も変わります。",
            // "graph" is a domain word and stays English in both arms (ADR-0048).
            (En, SettingsCompact) => "Compact graph",
            (Ja, SettingsCompact) => "graph をコンパクト表示",
            (En, SettingsCompactDesc) => "Use a tighter row height in the commit graph.",
            (Ja, SettingsCompactDesc) => "commit graph の行の高さを詰めて表示します。",
            (En, SettingsLaneCompact) => "Avatar commit nodes",
            (Ja, SettingsLaneCompact) => "commit ノードをアバター表示",
            (En, SettingsLaneCompactDesc) => {
                "On: each commit's node is the author's avatar (ringed in its lane colour) with a faint branch-colour band behind the graph. Off: a plain coloured dot, the avatar back in the message row, and no band. Either way the graph keeps its lane colours (graph lines, nodes and branch/tag labels)."
            }
            (Ja, SettingsLaneCompactDesc) => {
                "オン: 各 commit のノードが著者アバター(レーン色の輪付き)になり、グラフ背後に branch 色の帯が付きます。オフ: 通常の色ドットになり、アバターはメッセージ行に戻り、帯は出ません。どちらでもグラフのレーン色(線・ノード・branch/tag のラベル)は維持されます。"
            }
            (En, SettingsAutoFetch) => "Auto-fetch",
            (Ja, SettingsAutoFetch) => "自動 fetch",
            (En, SettingsAutoFetchDesc) => {
                "Periodically fetch the remote in the background so the graph and ahead/behind stay current."
            }
            (Ja, SettingsAutoFetchDesc) => {
                "バックグラウンドで定期的に remote を fetch し、graph や ahead/behind を最新に保ちます。"
            }
            (En, SettingsGraphCopy) => "Graph Cmd+C copies",
            (Ja, SettingsGraphCopy) => "Graph の Cmd+C でコピー",
            (En, SettingsGraphCopyDesc) => {
                "What ⌘C copies from the selected commit row. Branch falls back to the hash when the row has no local branch."
            }
            (Ja, SettingsGraphCopyDesc) => {
                "選択中の commit 行を ⌘C でコピーする対象。branch は local branch が無い行では hash を代わりにコピーします。"
            }
            (En, SettingsMarkdownDefaultView) => "Markdown opening mode",
            (Ja, SettingsMarkdownDefaultView) => "Markdown を開くモード",
            (En, SettingsMarkdownDefaultViewDesc) => {
                "Initial view for newly opened Markdown files. Open tabs keep their current view."
            }
            (Ja, SettingsMarkdownDefaultViewDesc) => {
                "新しく開く Markdown ファイルの表示方法です。開いているタブの表示は変わりません。"
            }
            (En, MarkdownDefaultPreview) => "Preview",
            (Ja, MarkdownDefaultPreview) => "プレビュー",
            (En, MarkdownDefaultSource) => "Source",
            (Ja, MarkdownDefaultSource) => "ソース",
            (En, SettingsToolbarDisplay) => "Toolbar",
            (Ja, SettingsToolbarDisplay) => "ツールバー",
            (En, SettingsToolbarDisplayDesc) => "Show icons alone or icons with captions.",
            (Ja, SettingsToolbarDisplayDesc) => "アイコンのみ、またはアイコンと文字を表示します。",
            (En, ToolbarIconOnly) => "Icon Only",
            (Ja, ToolbarIconOnly) => "アイコンのみ",
            (En, ToolbarIconAndText) => "Icon and Text",
            (Ja, ToolbarIconAndText) => "アイコンと文字",
            (En, ToolbarBranch) => "Branch",
            (Ja, ToolbarBranch) => "Branch",
            (En, ToolbarStash) => "Stash",
            (Ja, ToolbarStash) => "Stash",
            (En, ToolbarPop) => "Pop",
            (Ja, ToolbarPop) => "Pop",
            (En, ToolbarEditor) => "Editor",
            (Ja, ToolbarEditor) => "エディター",
            (En, ToolbarAnalyze) => "Analyze",
            (Ja, ToolbarAnalyze) => "分析",
            (En, ToolbarTerminal) => "Terminal",
            (Ja, ToolbarTerminal) => "ターミナル",
            (En, BottomTabOperationLog) => "Operation Log",
            (Ja, BottomTabOperationLog) => "操作ログ",
            (En, BottomTabActivity) => "Activity",
            (Ja, BottomTabActivity) => "アクティビティ",
            (En, BottomTabChanges) => "Changes",
            (Ja, BottomTabChanges) => "変更",
            (En, ActivityPeriod) => "Activity period",
            (Ja, ActivityPeriod) => "アクティビティの期間",
            (En, ActivityChart) => "Commit activity",
            (Ja, ActivityChart) => "commit の推移",
            (En, ActivityDay) => "Day",
            (Ja, ActivityDay) => "日",
            (En, ActivityWeek) => "Week",
            (Ja, ActivityWeek) => "週",
            (En, ActivityMonth) => "Month",
            (Ja, ActivityMonth) => "月",
            (En, ActivityYear) => "Year",
            (Ja, ActivityYear) => "年",
            (En, ActivityAll) => "All",
            (Ja, ActivityAll) => "全期間",
            (En, StatusShowTerminal) => "Show Terminal",
            (Ja, StatusShowTerminal) => "ターミナルを表示",
            (En, StatusHideTerminal) => "Hide Terminal (⌘J)",
            (Ja, StatusHideTerminal) => "ターミナルを隠す (⌘J)",
            (En, StatusShowOperationLog) => "Show Operation Log (↵ / Space)",
            (Ja, StatusShowOperationLog) => "操作ログを表示 (↵ / Space)",
            (En, StatusHideOperationLog) => "Hide Operation Log (↵ / Space)",
            (Ja, StatusHideOperationLog) => "操作ログを隠す (↵ / Space)",
            (En, ToolbarOverflow) => "More Toolbar Commands",
            (Ja, ToolbarOverflow) => "その他のツールバーコマンド",
            (En, TabOverflow) => "All Tabs",
            (Ja, TabOverflow) => "すべてのタブ",
            (En, SettingsReduceMotion) => "Reduce motion",
            (Ja, SettingsReduceMotion) => "モーションを減らす",
            (En, SettingsReduceMotionDesc) => {
                "Stop decorative animations. macOS Reduce Motion is also respected, even when this switch is off."
            }
            (Ja, SettingsReduceMotionDesc) => {
                "装飾的なアニメーションを静止させます。オフでも macOS の「視差効果を減らす」を尊重します。"
            }
            (En, SettingsTerminalAutoLock) => "Lock worktree while the terminal is open",
            (Ja, SettingsTerminalAutoLock) => "terminal を開いている間 worktree をロック",
            (En, SettingsTerminalAutoLockDesc) => {
                "When the terminal opens in a linked worktree, offer to lock that worktree and to unlock it when the shell exits. Locks you added yourself are left alone. macOS / Linux only."
            }
            (Ja, SettingsTerminalAutoLockDesc) => {
                "linked worktree で terminal を開いたときにロックを提案し、shell 終了時に解除を提案します。自分で付けたロックはそのままです。macOS / Linux のみ。"
            }
            (En, SettingsInterfaceLang) => "Interface language",
            (Ja, SettingsInterfaceLang) => "表示言語",
            (En, SettingsInterfaceLangDesc) => {
                "Language for explanatory text (Git domain words stay English)."
            }
            (Ja, SettingsInterfaceLangDesc) => {
                "説明文の言語(Git の用語は英語のままです)。"
            }

            // ── Analyze ignore section (ADR-0119) ────────────────────
            (En, SettingsAnalyzeIgnore) | (Ja, SettingsAnalyzeIgnore) => "Analyze ignore",
            (En, SettingsAnalyzeIgnoreDesc) => {
                "Files matching these patterns are excluded from Analyze (Hotspots / Coupling / Ownership). One pattern per line, .gitignore syntax — wildcards (* ** ?) and negation (!) work. Reload the repository to apply."
            }
            (Ja, SettingsAnalyzeIgnoreDesc) => {
                "これらのパターンに一致するファイルは Analyze(Hotspots / Coupling / Ownership)から除外されます。1 行 1 パターン、.gitignore と同じ書式でワイルドカード(* ** ?)や否定(!)が使えます。反映にはリポジトリの再読み込みが必要です。"
            }
            (En, SettingsAnalyzeIgnoreSave) => "Save",
            (Ja, SettingsAnalyzeIgnoreSave) => "保存",
            (En, SettingsAnalyzeIgnoreReset) => "Reset to defaults",
            (Ja, SettingsAnalyzeIgnoreReset) => "デフォルトに戻す",
            (En, SettingsAnalyzeIgnoreUnsaved) => "Unsaved changes",
            (Ja, SettingsAnalyzeIgnoreUnsaved) => "未保存の変更",
            (En, SettingsAnalyzeIgnoreSaved) => "Analyze ignore saved",
            (Ja, SettingsAnalyzeIgnoreSaved) => "Analyze ignore を保存しました",
            (En, SettingsAnalyzeIgnoreFailed) => "Could not save Analyze ignore",
            (Ja, SettingsAnalyzeIgnoreFailed) => "Analyze ignore を保存できませんでした",
            (En, SettingsAnalyzeIgnoreClean) => "Saved",
            (Ja, SettingsAnalyzeIgnoreClean) => "保存済み",
            (En, SettingsAnalyzeResetTitle) => "Discard changes and reset Analyze ignore?",
            (Ja, SettingsAnalyzeResetTitle) => "変更を破棄して Analyze ignore をリセットしますか？",
            (En, SettingsAnalyzeResetDetail) => "Your unsaved patterns will be discarded. Your Analyze ignore patterns will be replaced with the defaults.",
            (Ja, SettingsAnalyzeResetDetail) => "未保存のパターンは破棄されます。Analyze ignore のパターンを既定値に置き換えます。",

            // ── Smart Commit section (ADR-0090 / ADR-0099) ───────────
            // "Smart Commit" is a product feature name and stays English.
            (En, SettingsSmartCommit) | (Ja, SettingsSmartCommit) => "Smart Commit",
            (En, SettingsSmartEnable) => "Enable Smart Commit (LLM)",
            (Ja, SettingsSmartEnable) => "Smart Commit (LLM) を有効化",
            (En, SettingsSmartEnableDesc) => {
                "Use an LLM to draft commit messages. The local Ollama provider keeps the staged diff on localhost."
            }
            (Ja, SettingsSmartEnableDesc) => {
                "LLM で commit メッセージの草案を作成します。ローカルの Ollama provider は staged な diff を localhost にとどめます。"
            }
            (En, SettingsSmartProvider) => "Provider",
            (Ja, SettingsSmartProvider) => "プロバイダー",
            (En, SettingsSmartProviderDesc) => {
                "Where commit messages are generated. Ollama is local; Claude Code / Codex use your installed CLI."
            }
            (Ja, SettingsSmartProviderDesc) => {
                "commit メッセージを生成する場所。Ollama はローカルで、Claude Code / Codex はインストール済みの CLI を使います。"
            }
            (En, SettingsSmartModel) => "LLM model",
            (Ja, SettingsSmartModel) => "LLM モデル",
            (En, SettingsSmartModelDesc) => {
                "Local Ollama model used to generate commit messages."
            }
            (Ja, SettingsSmartModelDesc) => {
                "commit メッセージの生成に使うローカルの Ollama モデル。"
            }
            (En, SettingsSmartNoModels) => "No local models detected — start Ollama",
            (Ja, SettingsSmartNoModels) => {
                "ローカルモデルが検出されませんでした — Ollama を起動してください"
            }

            // ── Branch Cleanup (ADR-0128) ───────────────────────────
            (En, CleanupTitle) => "Merged branches",
            (Ja, CleanupTitle) => "merge 済み branch",
            (En, CleanupDeleteMerged) => "Delete merged",
            (Ja, CleanupDeleteMerged) => "merge 済みを削除",
            (En, CleanupCopyAll) => "Copy all names",
            (Ja, CleanupCopyAll) => "全 branch 名をコピー",
            (En, CleanupNamesCopied) => "Branch names copied to clipboard",
            (Ja, CleanupNamesCopied) => "branch 名をクリップボードにコピーしました",
            (En, CleanupColBranch) => "Branch",
            (Ja, CleanupColBranch) => "branch",
            (En, CleanupColWhere) => "Where",
            (Ja, CleanupColWhere) => "場所",
            (En, CleanupColMergedAt) => "Merged",
            (Ja, CleanupColMergedAt) => "merge 日",
            (En, CleanupColStatus) => "Status",
            (Ja, CleanupColStatus) => "状態",
            (En, CleanupBadgeMerged) => "merged",
            (Ja, CleanupBadgeMerged) => "merge 済み",
            (En, CleanupBadgeSquash) => "squash?",
            (Ja, CleanupBadgeSquash) => "squash済み?",
            (En, CleanupBadgeGrown) => "grown",
            (Ja, CleanupBadgeGrown) => "merge 後に追加",
            (En, CleanupBadgeStale) => "stale",
            (Ja, CleanupBadgeStale) => "ストール",
            (En, CleanupGrownHint) => "new commits since merge:",
            (Ja, CleanupGrownHint) => "merge 後の新規 commit:",
            (En, ModalRecoveryCommands) => "recovery commands:",
            (Ja, ModalRecoveryCommands) => "復旧コマンド:",
            (En, AmendFoldedFiles) => "Staged changes folded in",
            (Ja, AmendFoldedFiles) => "この commit に取り込む staged 変更",
            (En, ModalCopyAll) => "Copy this dialog as text",
            (Ja, ModalCopyAll) => "この内容をテキストでコピー",
            // #1016 modal polish.
            (En, ModalCherryPickConfirm) => "Cherry-pick",
            (Ja, ModalCherryPickConfirm) => "cherry-pick",
            (En, ModalCommitConfirm) => "Commit",
            (Ja, ModalCommitConfirm) => "commit",
            (En, ModalNoDiscardTargets) => "No files to discard",
            (Ja, ModalNoDiscardTargets) => "破棄するファイルがありません",
            (En, ModalPlanNotReady) => "Still checking…",
            (Ja, ModalPlanNotReady) => "確認中…",
            (En, MaintenanceCommitGraphAfter) => "commit-graph written",
            (Ja, MaintenanceCommitGraphAfter) => "commit-graph 書き込み済み",
            (En, MaintenanceFsmonitorAfter) => "fsmonitor enabled",
            (Ja, MaintenanceFsmonitorAfter) => "fsmonitor 有効",
            (En, MaintenanceCommitGraphDetail) => {
                "commit-graph written for every reachable commit; working tree unchanged"
            }
            (Ja, MaintenanceCommitGraphDetail) => {
                "到達可能なすべての commit の commit-graph を書き込みます。working tree は変更されません。"
            }
            (En, MaintenanceFsmonitorDetail) => {
                "core.fsmonitor = true (local config); working tree unchanged"
            }
            (Ja, MaintenanceFsmonitorDetail) => {
                "ローカル設定で core.fsmonitor = true にします。working tree は変更されません。"
            }
            (En, MaintenanceWriteCommitGraph) => "Write commit-graph",
            (Ja, MaintenanceWriteCommitGraph) => "commit-graph を書き込む",
            (En, MaintenanceEnableFsmonitor) => "Enable fsmonitor",
            (Ja, MaintenanceEnableFsmonitor) => "fsmonitor を有効化",
            (En, PlanCreateBranchHere) => "Create branch here...",
            (Ja, PlanCreateBranchHere) => "ここに branch を作成...",
            (En, InputApply) => "Apply",
            (Ja, InputApply) => "適用",
            (En, ModalEquivalentCommand) => "equivalent command:",
            (Ja, ModalEquivalentCommand) => "相当するコマンド:",
            (En, ModalCopyList) => "Copy every row",
            (Ja, ModalCopyList) => "一覧をすべてコピー",
            (En, ModalCopied) => "Copied to clipboard",
            (Ja, ModalCopied) => "クリップボードにコピーしました",
            (En, ModalCannotUndo) => "Cannot be undone",
            (Ja, ModalCannotUndo) => "取り消せません",
            (En, ModalTargetFiles) => "Target files",
            (Ja, ModalTargetFiles) => "対象ファイル",
            (En, ModalWarningsSection) => "Warnings",
            (Ja, ModalWarningsSection) => "警告",
            (En, DiscardSkippedSection) => "Skipped (untracked / conflicted)",
            (Ja, DiscardSkippedSection) => "対象外(untracked / conflict)",
            (En, PlanOverlapSummary) => {
                "{} file(s) also modified by the target. Stash or commit them first."
            }
            (Ja, PlanOverlapSummary) => "切り替え先も変更する {} 件のファイルに変更があります。先に stash か commit してください。",
            (En, PlanRestoreConflictSummary) => {
                "{} file(s) will conflict when your stashed changes are restored. The stash is kept."
            }
            (Ja, PlanRestoreConflictSummary) => "stash を戻すときに {} 件のファイルが conflict します。stash は保持されます。",
            (En, PlanRestoreConflictMaybeSummary) => {
                "{} path(s) changed on both sides that Kagi could not merge in advance — the stash restore may conflict. The stash is kept."
            }
            (Ja, PlanRestoreConflictMaybeSummary) => "両方で変更され、事前に merge を判定できなかった {} 件のパスです。stash 復元は conflict する可能性があります(stash は保持されます)。",
            (En, PlanHeadingSetUpstream) => "set upstream",
            (Ja, PlanHeadingSetUpstream) => "upstream を設定",
            (En, PlanHeadingApprove) => "approve",
            (Ja, PlanHeadingApprove) => "承認",
            (En, PlanHeadingComment) => "comment",
            (Ja, PlanHeadingComment) => "コメント",
            (En, PlanHeadingRequestChanges) => "request changes",
            (Ja, PlanHeadingRequestChanges) => "修正を依頼",
            (En, PlanHeadingBehind) => "{} behind",
            (Ja, PlanHeadingBehind) => "{} 件遅れ",
            (En, PlanHeadingFiles) => "{} files",
            (Ja, PlanHeadingFiles) => "{} 件のファイル",
            (En, PlanHeadingStashes) => "{} stashes",
            (Ja, PlanHeadingStashes) => "stash {} 件",
            (En, PlanHeadingBranches) => "{} branches",
            (Ja, PlanHeadingBranches) => "{} 件の branch",
            (En, PlanSetUpstreamFor) => "Set upstream for {}",
            (Ja, PlanSetUpstreamFor) => "{} の upstream を設定",
            (En, PlanRenameBranch) => "Rename {}",
            (Ja, PlanRenameBranch) => "{} をリネーム",
            (En, PlanMerge) => "Merge",
            (Ja, PlanMerge) => "merge",
            (En, PlanRebaseOnto) => "Rebase {}",
            (Ja, PlanRebaseOnto) => "{} を rebase",
            (En, PlanRebaseCandidates) => "Replay candidates",
            (Ja, PlanRebaseCandidates) => "replay 候補の commit",
            (En, PlanRebaseMore) => "… and {} more",
            (Ja, PlanRebaseMore) => "…ほか {} 個の commit",
            (En, PlanRebaseResultUnknown) => "Result OID is not yet known",
            (Ja, PlanRebaseResultUnknown) => "実行後の OID は未確定",
            (En, PlanBranchesToDelete) => "Branches to delete",
            (Ja, PlanBranchesToDelete) => "削除する branch",
            (En, PlanCommitsToPush) => "Commits to push",
            (Ja, PlanCommitsToPush) => "push する commit",
            (En, PlanTagPublishScope) => "Tag to publish",
            (Ja, PlanTagPublishScope) => "公開する tag",
            (En, PlanTagName) => "Tag",
            (Ja, PlanTagName) => "tag",
            (En, PlanTagKind) => "Kind",
            (Ja, PlanTagKind) => "種類",
            (En, PlanTagObject) => "Tag object OID",
            (Ja, PlanTagObject) => "tag object OID",
            (En, PlanTagTarget) => "Peeled target OID",
            (Ja, PlanTagTarget) => "peel 後の対象 OID",
            (En, PlanTagRemote) => "Remote",
            (Ja, PlanTagRemote) => "remote",
            (En, PlanTagPushDestination) => "Push destination",
            (Ja, PlanTagPushDestination) => "push 先",
            (En, PlanTagLocalUnchanged) => "Local HEAD, index and working tree stay unchanged.",
            (Ja, PlanTagLocalUnchanged) => "ローカルの HEAD、index、working tree は変更しません。",
            (En, A11yCommitList) => "Commits",
            (Ja, A11yCommitList) => "commit 一覧",
            (En, A11yCommitRow) => "{}, by {}, {}, commit {}",
            (Ja, A11yCommitRow) => "{}、{}、{}、commit {}",
            (En, A11yCommitRefs) => ", refs: {}",
            (Ja, A11yCommitRefs) => "、ref: {}",
            (En, A11yWipRow) => "Uncommitted changes in {}, {}",
            (Ja, A11yWipRow) => "{} の未 commit の変更、{}",
            (En, A11ySidebarLocal) => "Local Branches",
            (Ja, A11ySidebarLocal) => "ローカル branch",
            (En, A11ySidebarRemote) => "Remote Branches",
            (Ja, A11ySidebarRemote) => "remote branch",
            (En, A11ySidebarWorktrees) => "Worktrees",
            (Ja, A11ySidebarWorktrees) => "worktree",
            (En, A11ySidebarTags) => "Tags",
            (Ja, A11ySidebarTags) => "tag",
            (En, A11ySidebarStashes) => "Stashes",
            (Ja, A11ySidebarStashes) => "stash",
            (En, A11ySidebarGroup) => "{}, {} items",
            (Ja, A11ySidebarGroup) => "{}、{} 件",
            (En, A11ySidebarBranch) => "Branch {}",
            (Ja, A11ySidebarBranch) => "branch {}",
            (En, A11ySidebarCurrentBranch) => "Branch {}, current",
            (Ja, A11ySidebarCurrentBranch) => "branch {}、現在の branch",
            (En, A11ySidebarRemoteBranch) => "Remote branch {}",
            (Ja, A11ySidebarRemoteBranch) => "remote branch {}",
            (En, A11ySidebarTag) => "Tag {}",
            (Ja, A11ySidebarTag) => "tag {}",
            (En, SidebarTagCommitNotLoaded) => "Commit not loaded; load more history.",
            (Ja, SidebarTagCommitNotLoaded) => "commit が未読込みです。履歴を追加で読み込んでください。",
            (En, SidebarTagHiddenBySolo) => "Commit is hidden by Branch Solo.",
            (Ja, SidebarTagHiddenBySolo) => "commit は Branch Solo により非表示です。",
            (En, A11ySidebarStash) | (Ja, A11ySidebarStash) => "stash@{{}}: {}",
            (En, A11ySidebarWorktree) => "Worktree {}, {}",
            (Ja, A11ySidebarWorktree) => "worktree {}、{}",
            (En, A11ySidebarCurrentSuffix) => ", current",
            (Ja, A11ySidebarCurrentSuffix) => "、現在の worktree",
            (En, A11ySidebarLockedSuffix) => ", locked",
            (Ja, A11ySidebarLockedSuffix) => "、lock 中",
            (En, A11yPrList) => "Pull requests",
            (Ja, A11yPrList) => "pull request 一覧",
            (En, A11yPrRow) => "Pull request #{}, {}, {}, by @{}, {} into {}, checks {}, updated {}",
            (Ja, A11yPrRow) => "pull request #{}、{}、{}、@{}、{} を {} へ、check {}、更新 {}",
            (En, A11yFileTree) => "Files",
            (Ja, A11yFileTree) => "ファイル一覧",
            (En, A11yUnstagedTree) => "Unstaged files",
            (Ja, A11yUnstagedTree) => "未 stage のファイル",
            (En, A11yStagedTree) => "Staged files",
            (Ja, A11yStagedTree) => "stage 済みのファイル",
            (En, CommitPanelTitle) => "Commit Panel",
            (Ja, CommitPanelTitle) => "Commit Panel",
            (En, CommitPanelUnstaged) => "Unstaged",
            (Ja, CommitPanelUnstaged) => "未 stage",
            (En, CommitPanelStaged) => "Staged",
            (Ja, CommitPanelStaged) => "stage 済み",
            (En, CommitPanelList) => "List",
            (Ja, CommitPanelList) => "一覧",
            (En, CommitPanelTree) => "Tree",
            (Ja, CommitPanelTree) => "ツリー",
            (En, CommitPanelStageAll) => "Stage all",
            (Ja, CommitPanelStageAll) => "Stage all",
            (En, CommitPanelUnstageAll) => "Unstage all",
            (Ja, CommitPanelUnstageAll) => "Unstage all",
            (En, CommitPanelDiscardAll) => "Discard all",
            (Ja, CommitPanelDiscardAll) => "Discard all",
            (En, CommitPanelNoStageTargets) => "No files can be staged while conflicts remain",
            (Ja, CommitPanelNoStageTargets) => "conflict 中のファイルは stage できません",
            (En, CommitPanelNothingUnstaged) => "No unstaged changes",
            (Ja, CommitPanelNothingUnstaged) => "未 stage の変更がありません",
            (En, CommitPanelConflict) => "Conflict",
            (Ja, CommitPanelConflict) => "conflict",
            (En, CommitPanelStage) | (Ja, CommitPanelStage) => "Stage",
            (En, CommitPanelUnstage) | (Ja, CommitPanelUnstage) => "Unstage",
            (En, A11yFileTreeDir) => "Folder {}",
            (Ja, A11yFileTreeDir) => "フォルダー {}",
            (En, A11yFileTreeFile) => "File {}, {}",
            (Ja, A11yFileTreeFile) => "ファイル {}、{}",
            (En, A11yFileTreeUnsaved) => ", unsaved changes",
            (Ja, A11yFileTreeUnsaved) => "、未保存の変更",
            (En, A11yFileAdded) => "added",
            (Ja, A11yFileAdded) => "追加",
            (En, A11yFileModified) => "modified",
            (Ja, A11yFileModified) => "変更",
            (En, A11yFileDeleted) => "deleted",
            (Ja, A11yFileDeleted) => "削除",
            (En, A11yFileRenamed) => "renamed",
            (Ja, A11yFileRenamed) => "名前変更",
            (En, A11yFileTypeChanged) => "type changed",
            (Ja, A11yFileTypeChanged) => "型変更",
            (En, A11yFileUnchanged) => "unchanged",
            (Ja, A11yFileUnchanged) => "変更なし",
            (En, A11yFileConflicted) => "conflicted",
            (Ja, A11yFileConflicted) => "conflict",
            (En, A11yStashRow) => "Stash: {}",
            (Ja, A11yStashRow) => "stash: {}",
            (En, ThemeColorVision) => "Color Vision (Blue/Orange)",
            (Ja, ThemeColorVision) => "色覚対応（青 / 橙）",
            (En, A11yDialogTwoStage) => "This operation needs two confirmations. Press Confirm twice to run it.",
            (Ja, A11yDialogTwoStage) => "この操作は 2 回の確認が必要です。1 回目の Confirm では実行されません。",
            (En, A11yDialogArmed) => "Ready: the next Confirm runs this operation.",
            (Ja, A11yDialogArmed) => "確認済み: 次の Confirm で実行されます。",
            (En, A11yDialogBlocked) => "This operation cannot run; see the blockers.",
            (Ja, A11yDialogBlocked) => "この操作は実行できません。blocker を確認してください。",
            (En, PlanAmendConfirm) => "Amend",
            (Ja, PlanAmendConfirm) => "amend",
            (En, PlanAmendConfirmArmed) => "Rewrite history",
            (Ja, PlanAmendConfirmArmed) => "履歴を書き換える",
            (En, AmendArmedNotice) => "⚠ This replaces a published commit. Confirm the history rewrite.",
            (Ja, AmendArmedNotice) => "⚠ 公開済みの commit を置き換えます。履歴の書き換えを確認してください。",
            (En, AmendArmedRecovery) => "The old commit remains recoverable locally through the reflog. This does not publish the replacement or repair anyone else's history.",
            (Ja, AmendArmedRecovery) => "元の commit は local の reflog から復旧できます。この操作では変更を公開せず、他の人の履歴も修復しません。",
            (En, PlanDiscardConfirm) => "Discard {} file(s)",
            (Ja, PlanDiscardConfirm) => "{} 個のファイルを破棄",
            (En, PlanDiscardConfirmArmed) => "Permanently discard {} file(s)",
            (Ja, PlanDiscardConfirmArmed) => "{} 個のファイルを完全に破棄",
            (En, DiscardArmedNotice) => "⚠ This plan has no file backup. Confirm permanent deletion of the selected working-tree changes.",
            (Ja, DiscardArmedNotice) => "⚠ この計画にはファイルのバックアップがありません。選択した working tree の変更を完全に削除することを確認してください。",
            (En, DiscardBackedOutcome) => "Discard the listed working-tree changes after retaining a backup of their current contents.",
            (Ja, DiscardBackedOutcome) => "一覧の working tree の変更を破棄します。現在の内容は先に backup に保持します。",
            (En, DiscardAllTitle) => "Discard all changes ({})",
            (Ja, DiscardAllTitle) => "すべての変更を破棄（{}）",
            (En, DiscardUnsafePath) => "Cannot discard '{}': this path cannot be represented safely on this platform. No files were changed.",
            (Ja, DiscardUnsafePath) => "「{}」はこの環境で元のパスを安全に扱えないため破棄できません。ファイルは変更していません。",
            (En, PlanReplayOnto) => "Replay {}",
            (Ja, PlanReplayOnto) => "{} を replay",
            (En, PlanReplayOntoArmed) => "\u{26a0} Really replay — rewrites the branch",
            (Ja, PlanReplayOntoArmed) => "\u{26a0} 本当に replay しますか。branch の履歴を書き換えます",
            (En, PlanSyncToRemote) => "Sync to remote (keep local)",
            (Ja, PlanSyncToRemote) => "remote に揃える（ローカルは保全）",
            (En, PlanSyncToRemoteArmed) => "\u{26a0} Really sync — replaces branch, index and files (backups kept)",
            (Ja, PlanSyncToRemoteArmed) => "\u{26a0} 本当に揃えますか。branch・index・ファイルを置き換えます（backup は残ります）",
            (En, PlanDeleteBranch) => "Delete 1 branch",
            (Ja, PlanDeleteBranch) => "1 branch を削除",
            (En, PlanDeleteBranchArmed) => "Really delete 1 branch — keep recovery ref",
            (Ja, PlanDeleteBranchArmed) => "1 branch を本当に削除 — 復元用 ref を保持",
            (En, PlanDeleteRemoteBranch) => "Delete remote branch",
            (Ja, PlanDeleteRemoteBranch) => "remote branch を削除",
            (En, PlanDeleteRemoteBranchArmed) => "\u{26a0} Really delete — cannot be undone",
            (Ja, PlanDeleteRemoteBranchArmed) => "\u{26a0} 本当に削除しますか。取り消せません",
            (En, PlanResetCurrent) => "Reset current branch (1)",
            (Ja, PlanResetCurrent) => "現在の branch (1 件) を reset",
            (En, PlanForcePush) => "Force-with-lease push (1 branch)",
            (Ja, PlanForcePush) => "1 branch を force-with-lease で push",
            (En, PlanCancel) => "Cancel",
            (Ja, PlanCancel) => "キャンセル",
            (En, InputBranchTitle) => "Create branch",
            (Ja, InputBranchTitle) => "branch を作成",
            (En, InputTagTitle) => "Create tag",
            (Ja, InputTagTitle) => "tag を作成",
            (En, InputWorktreeTitle) => "Create worktree",
            (Ja, InputWorktreeTitle) => "worktree を作成",
            (En, InputStashTitle) => "Stash changes",
            (Ja, InputStashTitle) => "変更を stash する",
            (En, InputBranchName) => "Branch name",
            (Ja, InputBranchName) => "branch 名",
            (En, InputNewBranchName) => "New branch name",
            (Ja, InputNewBranchName) => "新しい branch 名",
            (En, InputTagName) => "Tag name",
            (Ja, InputTagName) => "tag 名",
            (En, InputWorktreePath) => "Path",
            (Ja, InputWorktreePath) => "パス",
            // #1043 — Add Worktree path: the folder dialog's button.
            (En, InputChooseFolder) => "Choose folder",
            (Ja, InputChooseFolder) => "フォルダを選択",
            (En, InputStashMessage) => "Message (optional)",
            (Ja, InputStashMessage) => "メッセージ（省略可）",
            (En, InputStashCurrent) => "CURRENT",
            (Ja, InputStashCurrent) => "現在",
            (En, InputStashAfter) => "AFTER",
            (Ja, InputStashAfter) => "実行後",
            (En, PlanStateClean) => "clean",
            (Ja, PlanStateClean) => "変更なし",
            (En, PlanStateUntrackedRetained) => "{} untracked retained",
            (Ja, PlanStateUntrackedRetained) => "未追跡ファイル {} 件を保持",
            (En, InputStashUntrackedWarning) => "{} untracked file(s) will be included in the stash",
            (Ja, InputStashUntrackedWarning) => "未追跡ファイル {} 件も stash に含まれます",
            (En, InputUpstream) => "Upstream",
            (Ja, InputUpstream) => "upstream",
            (En, InputCheckoutAfterCreate) => "Checkout after create",
            (Ja, InputCheckoutAfterCreate) => "作成後に checkout",
            (En, InputCreate) => "Create",
            (Ja, InputCreate) => "作成",
            (En, InputStash) => "Stash",
            (Ja, InputStash) => "Stash",
            (En, InputSetUpstream) => "Set upstream",
            (Ja, InputSetUpstream) => "upstream を設定",
            (En, InputRename) => "Rename",
            (Ja, InputRename) => "リネーム",
            (En, InputPlanPending) => "Checking…",
            (Ja, InputPlanPending) => "確認中…",
            (En, InputRenameEnterNewName) => "Enter a new name",
            (Ja, InputRenameEnterNewName) => "新しい名前を入力",
            (En, TagPushTo) => "Push tag to {}",
            (Ja, TagPushTo) => "tag を {} に push",
            (En, TagPush) => "Push tag",
            (Ja, TagPush) => "tag を push",
            (En, TagNoRemote) => "no remote configured",
            (Ja, TagNoRemote) => "remote が設定されていません",
            (En, TagCopyName) => "Copy tag name",
            (Ja, TagCopyName) => "tag 名をコピー",
            (En, PushTagConfirm) => "Push tag",
            (Ja, PushTagConfirm) => "tag を push",
            (En, BusyPushTag) => "Pushing tag…",
            (Ja, BusyPushTag) => "tag を push 中…",
            (En, PushTagDone) => "push-tag: done",
            (Ja, PushTagDone) => "tag を push しました",
            (En, CleanupColPr) => "PR",
            (Ja, CleanupColPr) => "PR",
            (En, CleanupColAuthor) => "Author",
            (Ja, CleanupColAuthor) => "作成者",
            (En, CleanupDeleteSelected) => "Delete selected",
            (Ja, CleanupDeleteSelected) => "選択した branch を削除",
            (En, CleanupScanning) => "Scanning branches…",
            (Ja, CleanupScanning) => "branch を調べています…",
            (En, CleanupEmpty) => "No merged or stale branches — all clean.",
            (Ja, CleanupEmpty) => "merge 済み・休眠 branch はありません。クリーンです。",
            (En, CleanupNoRepo) => "Open a repository to see merged branches.",
            (Ja, CleanupNoRepo) => "リポジトリを開くと merge 済み branch が表示されます。",

            // ── Code Ecosystem / hot-spots (ADR-0119) ───────────────
            (En, Ecosystem) => "Analyze",
            (Ja, Ecosystem) => "解析",
            (En, EcoCopyDiagnostic) => "Copy diagnostic",
            (Ja, EcoCopyDiagnostic) => "診断をコピー",
            (En, EcoDiagnosticCopied) => "Diagnostic copied to clipboard",
            (Ja, EcoDiagnosticCopied) => "診断をクリップボードにコピーしました",
            (En, EcoLoading) => "Analyzing repository…",
            (Ja, EcoLoading) => "リポジトリを解析中…",
            (En, EcoLoadingHint) => "Large repositories can take a minute.",
            (Ja, EcoLoadingHint) => "大きいリポジトリでは1分ほどかかることがあります。",
            (En, EcoLoadFailed) => "analysis failed",
            (Ja, EcoLoadFailed) => "解析に失敗しました",
            (En, EcoEmpty) => "No activity to analyze",
            (Ja, EcoEmpty) => "解析できる変更がありません",
            (En, EcoList) => "List",
            (Ja, EcoList) => "リスト",
            (En, EcoMap) => "Map",
            (Ja, EcoMap) => "マップ",
            (En, EcoCouplesWith) => "couples with",
            (Ja, EcoCouplesWith) => "と結合:",
            (En, EcoGraph) => "Graph",
            (Ja, EcoGraph) => "グラフ",
            (En, EcoMermaid) | (Ja, EcoMermaid) => "Mermaid",
            (En, EcoOpenMermaidLive) => "Open in mermaid.live ↗",
            (Ja, EcoOpenMermaidLive) => "mermaid.live で開く ↗",
            (En, EcoMermaidHint) => {
                "Native view is text-only; open mermaid.live to render the diagram (the whole graph travels in the URL — nothing is uploaded)."
            }
            (Ja, EcoMermaidHint) => {
                "アプリ内ではテキスト表示です。mermaid.live を開くと図として描画されます(グラフ全体は URL に載るだけで、アップロードはされません)。"
            }
            (En, EcoResetView) => "Reset",
            (Ja, EcoResetView) => "reset",
            (En, EcoHelpTitle) => "How to read Analyze",
            (Ja, EcoHelpTitle) => "Analyze の見方",
            (En, EcoMode) => "Analyze mode",
            (Ja, EcoMode) => "Analyze の評価軸",
            (En, EcoPeriod) => "Analysis period",
            (Ja, EcoPeriod) => "解析期間",
            (En, EcoView) => "Analysis view",
            (Ja, EcoView) => "解析の表示方法",
            (En, EcoExportFormat) => "Diagnostic export format",
            (Ja, EcoExportFormat) => "診断の出力形式",

            // ── Editor Workspace (T-WS-EDITOR-001) ──────────────────
            (En, EditorWorkspaceLoading) => "Loading…",
            (Ja, EditorWorkspaceLoading) => "読み込み中…",
            (En, EditorWorkspaceEmpty) => "No changed files in the working tree.",
            (Ja, EditorWorkspaceEmpty) => "working tree に変更されたファイルはありません。",
            (En, EditorWorkspaceSelectFile) => "Select a file to view its contents.",
            (Ja, EditorWorkspaceSelectFile) => "内容を表示するファイルを選択してください。",
            (En, EditorWorkspaceBinary) => "Binary file — preview not available.",
            (Ja, EditorWorkspaceBinary) => "バイナリファイルのためプレビューできません。",
            (En, EditorWorkspaceTooLarge) => "File too large to preview (over 10 MiB or 50,000 lines).",
            (Ja, EditorWorkspaceTooLarge) => "ファイルが大きすぎるためプレビューできません(10 MiB または5万行超)。",
            (En, EditorWorkspaceNoDiff) => "No changes to show for this file.",
            (Ja, EditorWorkspaceNoDiff) => "このファイルに表示する変更はありません。",
            (En, EditorWorkspaceSourceChanges) => "Changes",
            (Ja, EditorWorkspaceSourceChanges) => "変更",
            (En, EditorWorkspaceSourceAll) => "All",
            (Ja, EditorWorkspaceSourceAll) => "すべて",
            (En, EditorWorkspaceDeleted) => "File does not exist in the working tree (deleted).",
            (Ja, EditorWorkspaceDeleted) => "working tree に存在しません(削除済み)。",
            (En, EditorWorkspaceUndecodable) => "Cannot decode as UTF-8 — preview not available.",
            (Ja, EditorWorkspaceUndecodable) => "UTF-8として読めないためプレビューできません。",
            (En, FileHistoryPathRenamed) => "File was renamed; open {path} instead.",
            (Ja, FileHistoryPathRenamed) => "ファイル名が変更されています。{path} を開いてください。",

            // ── Editor Workspace editable buffer (T-WS-EDITOR-002) ──
            (En, EditorWorkspaceExternalChanged) => "File changed on disk.",
            (Ja, EditorWorkspaceExternalChanged) => "ファイルがディスク上で変更されました。",
            (En, EditorWorkspaceReload) => "Reload",
            (Ja, EditorWorkspaceReload) => "再読み込み",
            (En, EditorWorkspaceOverwrite) => "Overwrite",
            (Ja, EditorWorkspaceOverwrite) => "上書き保存",
            (En, EditorWorkspaceSaveBlocked) => {
                "File changed on disk — not saved. Choose Overwrite or Reload."
            }
            (Ja, EditorWorkspaceSaveBlocked) => {
                "ファイルがディスク上で変更されたため保存しませんでした。上書き保存か再読み込みを選んでください。"
            }
            (En, EditorWorkspaceUnsavedTitle) => "Unsaved changes — discard?",
            (Ja, EditorWorkspaceUnsavedTitle) => "未保存の変更があります。破棄しますか?",
            (En, EditorHostUnsavedTitle) => "Save changes before closing?",
            (Ja, EditorHostUnsavedTitle) => "閉じる前に変更を保存しますか?",
            (En, EditorHostUnsavedCount) => "Files with unsaved changes: {count}",
            (Ja, EditorHostUnsavedCount) => "未保存の変更があるファイル: {count}",
            (En, EditorHostSaveAll) => "Save All",
            (Ja, EditorHostSaveAll) => "すべて保存",
            (En, EditorHostSaveWaiting) => {
                "Another operation is still running. Wait for it to finish, then try Save All again."
            }
            (Ja, EditorHostSaveWaiting) => {
                "別の操作がまだ実行中です。完了を待ってから、もう一度「すべて保存」を選んでください。"
            }
            (En, EditorHostRecoveryRequired) => {
                "The result of an operation is unknown. Choose Cancel and review recovery in the Operation Log before trying again."
            }
            (Ja, EditorHostRecoveryRequired) => {
                "操作の結果が不明です。「キャンセル」を選び、操作ログで復旧方法を確認してから再試行してください。"
            }
            (En, ConflictDraftSaveFailed) => "Draft could not be saved",
            (Ja, ConflictDraftSaveFailed) => "下書きを保存できていません",
            (En, ConflictDraftRetry) => "Retry",
            (Ja, ConflictDraftRetry) => "再試行",
            (En, ConflictDraftDiscardClose) => "Discard and Close",
            (Ja, ConflictDraftDiscardClose) => "破棄して閉じる",
            (En, ConflictDraftDiscardConfirm) => "Confirm Discard and Close",
            (Ja, ConflictDraftDiscardConfirm) => "破棄して閉じることを確認",
            (En, ConflictDraftUnsavedDetail) => "Conflict decisions exist only in this window. Retry saving or explicitly discard them before closing.",
            (Ja, ConflictDraftUnsavedDetail) => "conflict の解決内容はこのウィンドウにのみ残っています。保存を再試行するか、明示的に破棄してから閉じてください。",
            (En, ConflictDraftSavingTitle) => "Saving Conflict Decisions",
            (Ja, ConflictDraftSavingTitle) => "conflict の解決内容を保存中",
            (En, ConflictDraftSaving) => "Saving conflict decisions…",
            (Ja, ConflictDraftSaving) => "conflict の解決内容を保存しています…",
            (En, ConflictDraftDiscardDetail) => "These unsaved conflict decisions will be lost. Discard them and close?",
            (Ja, ConflictDraftDiscardDetail) => "未保存の conflict 解決内容が失われます。破棄して閉じますか？",
            (En, ConflictDraftMixedDiscardDetail) => "These unsaved conflict decisions and file changes will be lost. Discard them and close?",
            (Ja, ConflictDraftMixedDiscardDetail) => "未保存の conflict 解決内容とファイルの変更が失われます。破棄して閉じますか？",
            (En, EditorWorkspaceSaveInterrupted) => {
                "Saving was interrupted. Your changes are still open. Review the Operation Log before trying again."
            }
            (Ja, EditorWorkspaceSaveInterrupted) => {
                "保存が中断されました。未保存の変更は開いたままです。再試行する前に操作ログを確認してください。"
            }
            (En, EditorWorkspaceDiscard) => "Discard",
            (Ja, EditorWorkspaceDiscard) => "破棄",
            (En, EditorWorkspaceCancel) => "Cancel",
            (Ja, EditorWorkspaceCancel) => "キャンセル",
            (En, EditorWorkspaceBackToGraph) => "\u{2190} Graph",
            (Ja, EditorWorkspaceBackToGraph) => "\u{2190} グラフ",
            (En, EditorWorkspaceTitle) => "Editor Workspace",
            (Ja, EditorWorkspaceTitle) => "エディタワークスペース",
            (En, EditorDetailsShow) => "Show Editor Diff/History",
            (Ja, EditorDetailsShow) => "エディタの差分・変更履歴を表示",
            (En, EditorDetailsHide) => "Hide Editor Diff/History",
            (Ja, EditorDetailsHide) => "エディタの差分・変更履歴を非表示",
            (En, EditorDetailsUnavailable) => "Open the Editor workspace first",
            (Ja, EditorDetailsUnavailable) => "先にエディタワークスペースを開いてください",

            // ── Repository owner-trust prompt (ADR-0160) ──
            (En, TrustRepoTitle) => "This repository is owned by another user",
            (Ja, TrustRepoTitle) => "このリポジトリは別のユーザーが所有しています",
            (En, TrustRepoBody) => {
                "It can be inspected safely, but writes are blocked until you confirm you trust it. Trust only repositories whose config you would run yourself."
            }
            (Ja, TrustRepoBody) => {
                "閲覧は安全に行えますが、信頼を確認するまで書き込みはブロックされます。設定を自分で実行してもよいと思えるリポジトリだけを信頼してください。"
            }
            (En, TrustRepoConfirm) => "Trust this repository",
            (Ja, TrustRepoConfirm) => "このリポジトリを信頼する",
            (En, TrustRepoSaveFailed) => "Could not save trust for repository {}.",
            (Ja, TrustRepoSaveFailed) => "リポジトリ {} の信頼設定を保存できませんでした。",
            (En, TrustRepoReopenFailed) => "Trust was saved, but repository {} could not be reopened. It stays open as before.",
            (Ja, TrustRepoReopenFailed) => "信頼設定は保存しましたが、リポジトリ {} を開き直せませんでした。表示は以前のままです。",
            (En, TrustRepoRetry) => "Retry",
            (Ja, TrustRepoRetry) => "再試行",

            // ── Editor Workspace History/Snapshot tabs (T-WS-EDITOR-008) ──
            (En, EditorRightTabDiff) => "Diff",
            (Ja, EditorRightTabDiff) => "差分",
            (En, EditorRightTabHistory) => "History",
            (Ja, EditorRightTabHistory) => "変更履歴",
            (En, EditorMiddleTabDiff) => "Diff",
            (Ja, EditorMiddleTabDiff) => "差分",
            (En, EditorMiddleTabSnapshot) => "Snapshot",
            (Ja, EditorMiddleTabSnapshot) => "スナップショット",
            (En, EditorHistoryEmpty) => "No history for this file.",
            (Ja, EditorHistoryEmpty) => "このファイルの変更履歴はありません。",
            (En, EditorHistoryFailed) => "Could not load History.",
            (Ja, EditorHistoryFailed) => "History を取得できませんでした。",
            (En, EditorHistoryRetry) => "Retry",
            (Ja, EditorHistoryRetry) => "再試行",
            (En, EditorSnapshotUnavailable) => "Content not available for this commit.",
            (Ja, EditorSnapshotUnavailable) => "この commit 時点のコンテンツは取得できません。",
            (En, EditorSnapshotReadOnly) => "Snapshot — read only",
            (Ja, EditorSnapshotReadOnly) => "スナップショット — 読み取り専用",
            (En, EditorOpenCurrent) => "Open current",
            (Ja, EditorOpenCurrent) => "現在のファイルを開く",

            // ── Editor Workspace tree context menu (T-WS-EDITOR-007) ──
            (En, EditorTreeRename) => "Rename…",
            (Ja, EditorTreeRename) => "名前を変更…",
            (En, EditorTreeDelete) => "Delete…",
            (Ja, EditorTreeDelete) => "削除…",
            (En, EditorTreeCopyPath) => "Copy Path",
            (Ja, EditorTreeCopyPath) => "パスをコピー",
            (En, EditorTreeCopyRelativePath) => "Copy Relative Path",
            (Ja, EditorTreeCopyRelativePath) => "相対パスをコピー",
            (En, EditorTreeRevealFinder) => "Reveal in Finder",
            (Ja, EditorTreeRevealFinder) => "Finderで表示",
            (En, EditorTreeRevealFile) => "Reveal in File Manager",
            (Ja, EditorTreePreviewMarkdown) => "Markdownをプレビュー",
            (En, EditorTreePreviewMarkdown) => "Preview Markdown",
            (Ja, EditorWorkspacePreviewShow) => "プレビュー",
            (En, EditorWorkspacePreviewShow) => "Preview",
            (Ja, EditorWorkspacePreviewEdit) => "編集に戻る",
            (En, EditorWorkspacePreviewEdit) => "Back to editor",
            (En, EditorFindPrevious) => "Previous match",
            (Ja, EditorFindPrevious) => "前の一致",
            (En, EditorFindNext) => "Next match",
            (Ja, EditorFindNext) => "次の一致",
            (En, EditorFindReplace) => "Replace",
            (Ja, EditorFindReplace) => "置換",
            (En, EditorFindReplaceAll) => "Replace all",
            (Ja, EditorFindReplaceAll) => "すべて置換",
            (En, EditorFindClose) => "Close",
            (Ja, EditorFindClose) => "閉じる",
            (En, EditorFindCase) => "Match case",
            (Ja, EditorFindCase) => "大文字と小文字を区別",
            (En, EditorFindReplaceMode) => "Show replacement field",
            (Ja, EditorFindReplaceMode) => "置換欄を表示",
            (En, EditorFindField) => "Find",
            (Ja, EditorFindField) => "検索",
            (En, EditorFindReplacementField) => "Replacement",
            (Ja, EditorFindReplacementField) => "置換文字列",
            (Ja, EditorWorkspaceMermaidRendering) => "図を描画中…",
            (En, EditorWorkspaceMermaidRendering) => "Rendering diagram…",
            (Ja, EditorWorkspaceMermaidNeedsCli) => "図の描画には mermaid-cli (mmdc) が必要です",
            (En, EditorWorkspaceMermaidNeedsCli) => "Install mermaid-cli (mmdc) to render diagrams",
            (Ja, EditorWorkspaceMermaidFailed) => "図の描画に失敗しました",
            (En, EditorWorkspaceMermaidFailed) => "Diagram render failed",
            (Ja, EditorWorkspaceMermaidNoDetails) => {
                "図の描画に失敗しました。詳細なエラー情報はありません。"
            }
            (En, EditorWorkspaceMermaidNoDetails) => {
                "Diagram rendering failed. No error details were provided."
            }
            (Ja, EditorTreeRevealFile) => "ファイルマネージャで表示",
            (En, EditorTreeHistory) => "History",
            (Ja, EditorTreeHistory) => "履歴",
            (En, EditorTreeStage) => "Stage",
            (Ja, EditorTreeStage) => "stage",
            (En, EditorTreeUnstage) => "Unstage",
            (Ja, EditorTreeUnstage) => "unstage",
            (En, EditorTreeDiscard) => "Discard Changes…",
            (Ja, EditorTreeDiscard) => "変更を破棄…",
            (En, EditorTreeAddGitignore) => "Add to .gitignore",
            (Ja, EditorTreeAddGitignore) => ".gitignoreに追加",
            (En, EditorTreeNewFile) => "New File…",
            (Ja, EditorTreeNewFile) => "新規ファイル…",
            (En, EditorTreeNewFolder) => "New Folder…",
            (Ja, EditorTreeNewFolder) => "新規フォルダ…",
            (En, EditorFsPromptRenameTitle) => "Rename",
            (Ja, EditorFsPromptRenameTitle) => "名前を変更",
            (En, EditorFsPromptNewFileTitle) => "New File",
            (Ja, EditorFsPromptNewFileTitle) => "新規ファイル",
            (En, EditorFsPromptNewFolderTitle) => "New Folder",
            (Ja, EditorFsPromptNewFolderTitle) => "新規フォルダ",
            (En, EditorFsPromptNameLabel) => "Name",
            (Ja, EditorFsPromptNameLabel) => "名前",
            (En, EditorFsPromptCreateButton) => "Create",
            (Ja, EditorFsPromptCreateButton) => "作成",
            (En, EditorDeleteConfirmTitleFile) => "Delete file?",
            (Ja, EditorDeleteConfirmTitleFile) => "ファイルを削除しますか?",
            (En, EditorDeleteConfirmTitleFolder) => "Delete folder?",
            (Ja, EditorDeleteConfirmTitleFolder) => "フォルダを削除しますか?",
            (En, EditorDeleteConfirmTrashNote) => {
                "Moved to the Trash — recoverable from ~/.Trash."
            }
            (Ja, EditorDeleteConfirmTrashNote) => {
                "ゴミ箱に移動します(~/.Trash から復元できます)。"
            }
            (En, EditorDeleteConfirmUnsavedWarning) => {
                "Unsaved editor changes under this path will be discarded."
            }
            (Ja, EditorDeleteConfirmUnsavedWarning) => {
                "このパス配下の未保存のエディタ変更は破棄されます。"
            }
            (En, EditorDeleteConfirmButton) => "Move to Trash",
            (Ja, EditorDeleteConfirmButton) => "ゴミ箱に移動",
            (En, GeneratedFilesSection) => "Generated",
            (Ja, GeneratedFilesSection) => "生成ファイル",
            (En, AgentArtifactsSection) => "Agent artifacts",
            (Ja, AgentArtifactsSection) => "エージェント成果物",
            (En, AgentConventionBadge) => "Rule",
            (Ja, AgentConventionBadge) => "Rule",

            // ── issue #356: unsafe-unicode badge ────────────────────
            (En, UnsafeUnicodeBadge) => "hidden chars",
            (Ja, UnsafeUnicodeBadge) => "不可視文字",
            (En, UnsafeUnicodeTooltip) => {
                "This line contains bidirectional-control or zero-width characters that can hide or reorder text."
            }
            (Ja, UnsafeUnicodeTooltip) => {
                "この行には表示順を偽装できる双方向制御文字、または不可視のゼロ幅文字が含まれています。"
            }
            (En, BlameIgnoredMarkTip) => "* commit is in .git-blame-ignore-revs",
            (Ja, BlameIgnoredMarkTip) => "* commit は .git-blame-ignore-revs に含まれます",
            (En, BlameUnblamableMarkTip) => "? line is not yet committed",
            (Ja, BlameUnblamableMarkTip) => "? まだ commit されていない行",
            (En, BlameUncommitted) => "Uncommitted",
            (Ja, BlameUncommitted) => "未 commit",
            (En, EditorWorkspaceBlame) => "Blame",
            (Ja, EditorWorkspaceBlame) => "Blame",
            (En, EditorPdfPrevious) => "Previous PDF page",
            (Ja, EditorPdfPrevious) => "PDF の前のページ",
            (En, EditorPdfNext) => "Next PDF page",
            (Ja, EditorPdfNext) => "PDF の次のページ",
            (En, EditorPdfZoomOut) => "Zoom out PDF",
            (Ja, EditorPdfZoomOut) => "PDF を縮小",
            (En, EditorPdfZoomIn) => "Zoom in PDF",
            (Ja, EditorPdfZoomIn) => "PDF を拡大",
            (En, EditorPdfResetZoom) => "Reset PDF zoom",
            (Ja, EditorPdfResetZoom) => "PDF の倍率をリセット",
            (En, EditorPdfFitWidth) => "Fit Width",
            (Ja, EditorPdfFitWidth) => "幅に合わせる",
            (En, EditorPdfPage) => "Page",
            (Ja, EditorPdfPage) => "ページ",
            (En, EditorPdfLoading) => "Rendering PDF page…",
            (Ja, EditorPdfLoading) => "PDF ページを描画中…",
            (En, EditorPdfInvalid) => "Invalid PDF — this document could not be read.",
            (Ja, EditorPdfInvalid) => "無効な PDF のため読み込めません。",
            (En, EditorPdfPassword) => "This PDF requires a password. Password entry is not supported.",
            (Ja, EditorPdfPassword) => "この PDF にはパスワードが必要です。パスワードの入力には対応していません。",
            (En, EditorPdfEncryption) => "This PDF uses unsupported encryption.",
            (Ja, EditorPdfEncryption) => "この PDF の暗号化方式には対応していません。",
            (En, EditorPdfDimensions) => "This PDF page has invalid or unsupported dimensions.",
            (Ja, EditorPdfDimensions) => "この PDF ページの寸法は無効か、対応範囲外です。",
            (En, EditorPdfNoDiff) => "PDF viewing is read-only. PDF diff is not supported.",
            (Ja, EditorPdfNoDiff) => "PDF は読み取り専用です。PDF の差分表示には対応していません。",
            (En, EditorPdfPartial) => "Some page content could not be rendered.",
            (Ja, EditorPdfPartial) => "ページの一部を描画できませんでした。",
            (En, EditorPdfReadOnly) => "Read-only PDF",
            (Ja, EditorPdfReadOnly) => "読み取り専用 PDF",
            // #1016 PR B — modal controls, validation and AFTER-state chips.
            (En, CloneChangeFolderWhileRunning) => "Cannot change the folder while cloning.",
            (Ja, CloneChangeFolderWhileRunning) => "clone 中はフォルダーを変更できません。",
            (En, RemoteConnectTitle) => "Connect to a remote host (SSH)",
            (Ja, RemoteConnectTitle) => "remote ホストに接続 (SSH)",
            (En, RemoteSshNotice) => "Uses your system ssh (~/.ssh/config, keys, ssh-agent, known_hosts). New or password-only hosts must be set up in a terminal first.",
            (Ja, RemoteSshNotice) => "システムの ssh (~/.ssh/config、鍵、ssh-agent、known_hosts) を使用します。新しいホストやパスワードのみのホストは先にターミナルで設定してください。",
            (En, RemoteHostLabel) => "Host — user@host or a ~/.ssh/config alias",
            (Ja, RemoteHostLabel) => "ホスト — user@host または ~/.ssh/config の別名",
            (En, RemotePortLabel) => "Port (optional)",
            (Ja, RemotePortLabel) => "ポート (任意)",
            (En, RemoteIdentityLabel) => "Identity file (optional)",
            (Ja, RemoteIdentityLabel) => "認証鍵ファイル (任意)",
            (En, RemoteHostInvalid) => "Enter a host like user@host, host, or a ~/.ssh/config alias",
            (Ja, RemoteHostInvalid) => "user@host、host、または ~/.ssh/config の別名を入力してください",
            (En, RemotePortInvalid) => "Port must be a number 1–65535",
            (Ja, RemotePortInvalid) => "ポートには 1～65535 の数値を入力してください",
            (En, RemoteConnect) => "Connect",
            (Ja, RemoteConnect) => "接続",
            (En, RemoteConnecting) => "Connecting…",
            (Ja, RemoteConnecting) => "接続中…",
            (En, RemoteRequestBusy) => "Wait for the current SSH request to finish.",
            (Ja, RemoteRequestBusy) => "現在の SSH 処理が終わるまでお待ちください。",
            (En, RemoteChangeHost) => "Change host",
            (Ja, RemoteChangeHost) => "ホストを変更",
            (En, RemoteGitRepository) => "Git repository",
            (Ja, RemoteGitRepository) => "Git リポジトリ",
            (En, RemoteDetached) => "(detached)",
            (Ja, RemoteDetached) => "(detached)",
            (En, RemoteNoCommits) => "(no commits yet)",
            (Ja, RemoteNoCommits) => "(commit はまだありません)",
            (En, RemoteOpenRepository) => "Open repository (read-only)",
            (Ja, RemoteOpenRepository) => "リポジトリを開く (読み取り専用)",
            (En, RepositoryPickerOpen) => "Open Repository",
            (Ja, RepositoryPickerOpen) => "リポジトリを開く",
            (En, PathPickerFailed) => "{picker} failed: {error}",
            (Ja, PathPickerFailed) => "{picker} に失敗しました: {error}",
            (En, RemoteOpeningRepository) => "Opening…",
            (Ja, RemoteOpeningRepository) => "開いています…",
            (En, RemoteClose) => "Close",
            (Ja, RemoteClose) => "閉じる",
            (En, RemoteDirectoryList) => "Remote directory",
            (Ja, RemoteDirectoryList) => "remote ディレクトリ",
            (En, RemoteParentDirectory) => "Parent directory",
            (Ja, RemoteParentDirectory) => "親ディレクトリ",
            (En, RemoteDirectoryRow) => "Directory: {}",
            (Ja, RemoteDirectoryRow) => "ディレクトリ: {}",
            (En, RemoteFileRow) => "File: {}",
            (Ja, RemoteFileRow) => "ファイル: {}",
            (En, RemoteDirectoryEmpty) => "This directory is empty.",
            (Ja, RemoteDirectoryEmpty) => "このディレクトリは空です。",
            (En, SmartConsentTitle) => "Enable Local LLM generation?",
            (Ja, SmartConsentTitle) => "ローカル LLM による生成を有効にしますか？",
            (En, SmartConsentIntro) => "Pressing Generate sends your staged diff to a local Ollama model on this machine. Please review:",
            (Ja, SmartConsentIntro) => "生成を押すと、このマシンのローカル Ollama モデルに staged diff を送ります。以下を確認してください。",
            (En, SmartConsentEnable) => "Enable & continue",
            (Ja, SmartConsentEnable) => "有効にして続行",
            (En, SmartConsentStagedOnly) => "Only staged diff will be sent",
            (Ja, SmartConsentStagedOnly) => "送信されるのは stage 済みの差分だけです",
            (En, SmartConsentUnstagedExcluded) => "Unstaged changes will not be included",
            (Ja, SmartConsentUnstagedExcluded) => "stage されていない変更は含まれません",
            (En, SmartConsentLocalhost) => "The request stays on localhost Ollama",
            (Ja, SmartConsentLocalhost) => "リクエストは localhost の Ollama にだけ送られます",
            (En, SmartConsentSecretsWarning) => "Secrets may still exist in staged diff; review before generating",
            (Ja, SmartConsentSecretsWarning) => "stage 済みの差分に秘密情報が含まれる可能性があります。生成前に確認してください",
            (En, SmartModelTitle) => "Select a local model",
            (Ja, SmartModelTitle) => "ローカルモデルを選択",
            (En, SmartModelRemembered) => "Your choice is remembered.",
            (Ja, SmartModelRemembered) => "選んだモデルは次回も使用されます。",
            (En, SmartRuleBasedInserted) => "Rule-based suggestion inserted",
            (Ja, SmartRuleBasedInserted) => "ルールベースの下書きを挿入しました",
            (En, SmartNoModelsInserted) => "No local models found — rule-based draft inserted",
            (Ja, SmartNoModelsInserted) => "ローカルモデルがありません — ルールベースの下書きを挿入しました",
            (En, SmartGeneratingLocal) => "Generating with local LLM…",
            (Ja, SmartGeneratingLocal) => "ローカルLLMで生成中…",
            (En, SmartGeneratingProvider) => "Generating with {}…",
            (Ja, SmartGeneratingProvider) => "{} で生成中…",
            (En, SmartGeneratedLocal) => "Generated with local LLM",
            (Ja, SmartGeneratedLocal) => "ローカルLLMで生成しました",
            (En, SmartGenerationFallback) => "LLM unavailable — used rule-based",
            (Ja, SmartGenerationFallback) => "LLMを利用できません — ルールベースで生成しました",
            (En, SmartGenerationFailed) => "Generation failed — edit manually",
            (Ja, SmartGenerationFailed) => "生成に失敗しました — 手動で編集してください",
            (En, UpdateAvailableTitle) => "Update available",
            (Ja, UpdateAvailableTitle) => "アップデートがあります",
            (En, UpdateSkipVersion) => "Skip this version",
            (Ja, UpdateSkipVersion) => "このバージョンをスキップ",
            (En, UpdateReleasePage) => "Release page",
            (Ja, UpdateReleasePage) => "リリースページ",
            (En, UpdateLater) => "Later",
            (Ja, UpdateLater) => "後で",
            (En, UpdateHideContinuing) => "Hide — update continues",
            (Ja, UpdateHideContinuing) => "非表示 — 更新は続行します",
            (En, UpdateInstalling) => "Updating…",
            (Ja, UpdateInstalling) => "更新中…",
            (En, UpdateInstallingReason) => "An update is currently being installed.",
            (Ja, UpdateInstallingReason) => "更新をインストール中です。",
            (En, UpdateRestartReady) => "Update installed. Restart to finish updating.",
            (Ja, UpdateRestartReady) => "更新をインストールしました。再起動すると更新が完了します。",
            (En, UpdateRestart) => "Restart Kagi",
            (Ja, UpdateRestart) => "Kagi を再起動",
            (En, UpdateRestartWaiting) => {
                "Another operation is still running. Wait for it to finish, then choose Restart Kagi."
            }
            (Ja, UpdateRestartWaiting) => {
                "別の操作がまだ実行中です。完了を待ってから「Kagi を再起動」を選んでください。"
            }
            (En, UpdateFailed) => "Update failed",
            (Ja, UpdateFailed) => "更新に失敗しました",
            (En, UpdateNow) => "Update now",
            (Ja, UpdateNow) => "今すぐ更新",
            (En, PrFieldsNoChanges) => "No changes to apply",
            (Ja, PrFieldsNoChanges) => "適用する変更がありません",
            (En, EditorFsNameEmpty) => "Name cannot be empty",
            (Ja, EditorFsNameEmpty) => "名前を入力してください",
            (En, EditorFsNameInvalid) => "Invalid name",
            (Ja, EditorFsNameInvalid) => "無効な名前です",
            (En, EditorFsNameSeparator) => "Name cannot contain a path separator",
            (Ja, EditorFsNameSeparator) => "名前にパス区切り文字を含めることはできません",
            (En, EditorFsRenameGitBlocked) => "Cannot rename inside .git",
            (Ja, EditorFsRenameGitBlocked) => ".git 内の名前は変更できません",
            (En, EditorFsCreateGitBlocked) => "Cannot create inside .git",
            (Ja, EditorFsCreateGitBlocked) => ".git 内には作成できません",
            (En, EditorFsAlreadyExists) => "Already exists",
            (Ja, EditorFsAlreadyExists) => "既に存在します",
            (En, EditorDeleteUnsupported) => "Delete is only supported on macOS currently",
            (Ja, EditorDeleteUnsupported) => "現在、削除は macOS でのみ利用できます",
            (En, EditorDeleteGitBlocked) => "Cannot delete .git",
            (Ja, EditorDeleteGitBlocked) => ".git は削除できません",
            (En, EditorTrashUnsupported) => "Trash is only supported on macOS",
            (Ja, EditorTrashUnsupported) => "ゴミ箱は macOS でのみ利用できます",
            (En, EditorTrashHomeUnset) => "Can't find your home folder, so the Trash is unavailable.",
            (Ja, EditorTrashHomeUnset) => "ホームフォルダーが見つからないため、ゴミ箱を使えません。",
            (En, EditorTrashCreateDirFailed) => "cannot create ~/.Trash",
            (Ja, EditorTrashCreateDirFailed) => "~/.Trash を作成できません",
            (En, EditorTrashPathMissingName) => "path has no file name",
            (Ja, EditorTrashPathMissingName) => "パスにファイル名がありません",
            (En, EditorTrashTooManyNames) => "~/.Trash has too many same-named items",
            (Ja, EditorTrashTooManyNames) => "~/.Trash に同名の項目が多すぎます",
            (En, EditorTrashSameVolumeOnly) => "same-volume only",
            (Ja, EditorTrashSameVolumeOnly) => "同じボリューム内のみ対応しています",
            (En, PrMergeMethodCommit) => "Merge commit",
            (Ja, PrMergeMethodCommit) => "merge commit",
            (En, PrMergeMethodSquash) => "Squash and merge",
            (Ja, PrMergeMethodSquash) => "squash して merge",
            (En, PrMergeMethodRebase) => "Rebase and merge",
            (Ja, PrMergeMethodRebase) => "rebase して merge",
            (En, PrMergeSessionUnavailable) => "The repository is no longer open. Reopen it and try again.",
            (Ja, PrMergeSessionUnavailable) => "リポジトリが開かれていません。開き直してからやり直してください。",
            (En, AfterStashDrop) => "stash dropped",
            (Ja, AfterStashDrop) => "stash 削除",
            (En, AfterStashPop) => "restored, stash removed",
            (Ja, AfterStashPop) => "復元・stash 削除",
            (En, AfterHistoryUndo) => "commit changes staged",
            (Ja, AfterHistoryUndo) => "commit の変更を stage",
            (En, AfterHistoryAmend) => "new commit",
            (Ja, AfterHistoryAmend) => "新しい commit",
            (En, AfterHistoryMove) => "moved, changes kept",
            (Ja, AfterHistoryMove) => "移動・変更は保持",
            (En, AfterSwitchLatest) => "switched",
            (Ja, AfterSwitchLatest) => "切り替え済み",
            (En, AfterReviewApproved) => "approved",
            (Ja, AfterReviewApproved) => "承認済み",
            (En, AfterReviewCommented) => "commented",
            (Ja, AfterReviewCommented) => "コメント済み",
            (En, AfterReviewChanges) => "changes requested",
            (Ja, AfterReviewChanges) => "修正を依頼",
            (En, AfterPrMerged) => "merged",
            (Ja, AfterPrMerged) => "merge 済み",
        }
    }
}

/// issue #350: the "N revisions ignored" indicator shown on a blame view when a
/// `.git-blame-ignore-revs` file took effect. `n` is the number of distinct
/// ignored commits that actually appear in the blamed file.
pub fn blame_revisions_ignored(n: usize) -> String {
    match lang() {
        Lang::En => {
            let plural = if n == 1 { "" } else { "s" };
            format!("{n} revision{plural} ignored")
        }
        Lang::Ja => format!("{n} 件の revision を無視中"),
    }
}

/// T-WS-EDITOR-007: the delete-confirm modal's "N file(s)" note for a
/// directory target. `None` count (a file target) isn't passed here — the
/// caller only calls this for `is_dir` targets. Parameterized strings get a
/// plain helper `fn` here (same convention as `wip_row_note`) rather than a
/// `format!` at the call site.
pub fn editor_delete_file_count_note(count: usize, truncated: bool) -> String {
    let suffix = if truncated { "+" } else { "" };
    match lang() {
        Lang::En => format!("{count}{suffix} file(s) inside"),
        Lang::Ja => format!("内包ファイル数: {count}{suffix}"),
    }
}

/// Commit Panel section count; file names and diffstats remain user/Git data.
pub fn commit_panel_section(staged: bool, count: usize) -> String {
    let label = if staged {
        Msg::CommitPanelStaged
    } else {
        Msg::CommitPanelUnstaged
    };
    format!("{} ({count})", label.t())
}

/// Read-only Issues detail comment heading.
pub fn issue_comments(count: usize) -> String {
    match lang() {
        Lang::En => format!("Comments ({count})"),
        Lang::Ja => format!("コメント ({count})"),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Parameterized helpers (format! lives here, not at the call sites)
// ──────────────────────────────────────────────────────────────────────────

/// Palette section and unavailable reason share language-appropriate punctuation.
pub fn command_palette_description(language: Lang, section: &str, reason: Option<&str>) -> String {
    match (language, reason) {
        (Lang::En, Some(reason)) => format!("{section}. {reason}"),
        (Lang::Ja, Some(reason)) => format!("{section}。{reason}"),
        (_, None) => section.to_string(),
    }
}

/// Help-overlay sections for the Analyze view: `(heading, body)` in the active
/// language. Kept here so EN/JA stay side by side (ADR-0119).
pub fn eco_help_sections() -> Vec<(&'static str, &'static str)> {
    match lang() {
        Lang::En => vec![
            (
                "What is Analyze?",
                "A read-only look at your Git history — no changes are made. Pick a time window (Day … All) top-right; every view re-ranks for that window. Set excluded files in Settings → Analyze ignore.",
            ),
            (
                "Hotspots",
                "Files ranked by risk = how often a file changes (commits) × how big it is (LOC). Risk is shown numerically on a 0.0–1.0 scale. List bars are relative to the highest risk; Map colour shows absolute risk (green → red), with tile size = LOC (commits when LOC is zero). Busy AND complex code is worth examining — this is a hint to look, not a verdict.",
            ),
            (
                "Coupling",
                "Pairs of files that tend to change in the same commit — hidden dependencies. 'N×' = times they changed together; '%' = how strongly they overlap. Click a row, or focus it and press Enter/Space, to expand or collapse one file's full set of partners (1:many). 'Graph' draws it as a network: drag to pan, scroll to zoom toward the pointer, 'Reset' to refit. 'Mermaid' shows the same graph as Mermaid source with one-click open in mermaid.live.",
            ),
            (
                "Ownership",
                "Who maintains each file: the primary author, their share of the commits, and how many distinct authors have touched it. '1 author' (highlighted) is a bus-factor risk — only one person knows that file.",
            ),
            (
                "Copy diagnostic",
                "Copies the current view (Hotspots ranking, Coupling pairs, or Ownership) ready to paste into an AI chat as context (\"here is where the risk is — help me refactor\"). Switch modes to export that view. Pick the format next to the button: MD (Markdown table) or JSON for any mode, plus Mermaid in Coupling mode — a flowchart that makes the 1:many co-change structure visible.",
            ),
        ],
        Lang::Ja => vec![
            (
                "Analyze とは",
                "Git 履歴を分析します(変更は加えません)。右上で期間(Day … All)を選ぶと、各ビューがその期間で再集計されます。除外ファイルは 設定 → Analyze ignore で指定します。",
            ),
            (
                "Hotspots(ホットスポット)",
                "リスク = 変更頻度(commit 数)× 規模(行数 LOC)でファイルを順位付け。risk は 0.0–1.0 の数値で表示します。List のバーは最大 risk に対する相対値、Map の色は絶対 risk(緑→赤)、面積は LOC(0 LOC の場合は commit 数)です。「よく触られて、かつ複雑」な場所を調べるための目安で、断定ではありません。",
            ),
            (
                "Coupling(結合)",
                "同じ commit で一緒に変わりがちなファイルの組＝隠れた依存。『N×』= 一緒に変わった回数、『%』= 結びつきの強さ。行をクリックするか、focus して Enter/Space を押すと、全パートナー(1:多)の展開・折りたたみができます。『グラフ』はネットワーク表示：ドラッグで移動、スクロールでカーソル位置を中心に拡大、『reset』で初期表示。『Mermaid』は同じグラフを Mermaid ソースで表示し、ワンクリックで mermaid.live を開けます。",
            ),
            (
                "Ownership(所有)",
                "各ファイルの担当：主著者・その commit 占有率・関わった著者数。『1 author』(強調表示)はバス係数リスク＝そのファイルを知るのが 1 人だけ、という危険信号です。",
            ),
            (
                "診断をコピー",
                "現在表示中のビュー(Hotspots ランキング / Coupling の組 / Ownership)をコピーします。AI チャットに貼って文脈として渡せます(「ここがリスク。リファクタを手伝って」)。モードを切り替えると、そのビューが書き出されます。ボタン横で形式を選べます：どのモードでも MD(Markdown 表)か JSON、Coupling では加えて Mermaid(1:多の共変化構造が見えるフローチャート)。",
            ),
        ],
    }
}

/// WIP row note shown above the commit list when the working tree is dirty.
/// Was the hardcoded `"// WIP — N change(s)(クリックで commit panel)"`.
pub fn wip_row_note(n: usize) -> String {
    let plural = if n == 1 { "" } else { "s" };
    match lang() {
        Lang::En => format!(
            "// WIP — {} change{} (click to open commit panel)",
            n, plural
        ),
        Lang::Ja => format!("// WIP — {} change{}(クリックで Commit Panel)", n, plural),
    }
}

/// WIP row note for a *linked* worktree (not the one kagi has open). #473:
/// clicking the row shows that worktree's changes in the commit panel, in
/// place — it no longer opens the worktree as a tab, so the hint says so.
pub fn wip_row_other(n: usize) -> String {
    let plural = if n == 1 { "" } else { "s" };
    match lang() {
        Lang::En => format!(
            "// WIP — {} change{} (click to show its changes)",
            n, plural
        ),
        Lang::Ja => format!("// WIP — {} change{}(クリックで変更を表示)", n, plural),
    }
}

/// Commit-panel warning shown when unstaged changes exist and won't be included.
/// The swimlane's elision row between the window and the PR's root commit.
pub fn pr_lane_elided(n: usize) -> String {
    match lang() {
        Lang::En => format!("{} commit{} elided", n, if n == 1 { "" } else { "s" }),
        Lang::Ja => format!("{} commit 省略", n),
    }
}

/// The checks card's second line: how many ran, and how many failed (mock 7a).
pub fn pr_checks_counts(total: usize, failed: usize) -> String {
    match (lang(), failed) {
        (Lang::En, 0) => format!("{} successful checks", total),
        (Lang::En, f) => format!("{} checks · {} failed", total, f),
        (Lang::Ja, 0) => format!("チェック {} 件すべて成功", total),
        (Lang::Ja, f) => format!("チェック {} 件中 {} 件が失敗", total, f),
    }
}

/// Was the hardcoded `"⚠ N unstaged change(s) not included"`.
pub fn unstaged_not_included(n: usize) -> String {
    let plural = if n == 1 { "" } else { "s" };
    match lang() {
        Lang::En => format!("⚠ {} unstaged change{} not included", n, plural),
        Lang::Ja => format!("⚠ unstaged な変更 {} 件は含まれません", n),
    }
}

/// Toast after ⌘C on a diff line selection.
pub fn diff_lines_copied(n: usize) -> String {
    match lang() {
        Lang::En => format!("Copied {} line{}", n, if n == 1 { "" } else { "s" }),
        Lang::Ja => format!("{} 行をコピーしました", n),
    }
}

/// Commit-button label naming its destination — the branch is on the button
/// instead of in a separate preview line (ADR-0134).
/// Label for the Commit button.
///
/// Deliberately without the destination branch, which used to be interpolated
/// here (ADR-0134's "one place to look"): a real branch name overflowed the
/// button. The destination is still on screen — the header carries
/// `branch → upstream ↑A ↓B` at all times — and unlike the button it has room
/// for a long name.
pub fn commit_button() -> &'static str {
    match lang() {
        Lang::En => "Commit",
        Lang::Ja => "commit",
    }
}

/// Analyze settings failure; the filesystem path and OS cause stay verbatim.
pub fn analyze_ignore_failed_fmt(detail: &str) -> String {
    format!("{}: {detail}", Msg::SettingsAnalyzeIgnoreFailed.t())
}

/// A missing pre-rename history path, with the current filename left verbatim.
pub fn file_history_renamed_fmt(path: &str) -> String {
    Msg::FileHistoryPathRenamed.t().replace("{path}", path)
}

// ──────────────────────────────────────────────────────────────────────────
// W29-I18N-WAVE2: keyed git-layer validation → localized text
// ──────────────────────────────────────────────────────────────────────────

/// "A branch named '<name>' already exists in this repository." (localized).
/// The branch name stays verbatim per ADR-0048.
pub fn branch_exists_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!(
            "A branch named '{}' already exists in this repository.",
            name
        ),
        Lang::Ja => format!("branch '{}' は既に存在します。", name),
    }
}

/// "Branch '<name>' already exists." (rename path, localized).
pub fn branch_rename_exists_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!("Branch '{}' already exists.", name),
        Lang::Ja => format!("branch '{}' は既に存在します。", name),
    }
}

/// "Branch name '<name>' is not a valid git ref name …" (localized).
pub fn branch_invalid_ref_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!(
            kagi_domain::advice_template_display_en!(CommonBranchInvalidRef),
            name
        ),
        Lang::Ja => plan::advice_text(Msg::AdviceCommonBranchInvalidRef, &[&name]),
    }
}

/// "'<name>' is not a valid branch name." (rename path, localized).
pub fn branch_rename_invalid_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!("'{}' is not a valid branch name.", name),
        Lang::Ja => format!("'{}' は有効な branch 名ではありません。", name),
    }
}

/// "Branch name '<name>' must not start with '-'." (localized).
pub fn branch_leading_dash_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!("Branch name '{}' must not start with '-'.", name),
        Lang::Ja => format!("branch 名 '{}' は '-' で始められません。", name),
    }
}

/// "Worktree path '<path>' already exists." (localized). Path stays verbatim.
pub fn worktree_exists_fmt(path: &str) -> String {
    match lang() {
        Lang::En => format!("Worktree path '{}' already exists.", path),
        Lang::Ja => format!("worktree のパス '{}' は既に存在します。", path),
    }
}

impl Msg {
    /// Name the browser action, not merely the port it points at.
    pub fn worktree_inspection_open_url(port: u16) -> String {
        match lang() {
            Lang::En => format!("Open http://localhost:{port} in browser"),
            Lang::Ja => format!("http://localhost:{port} をブラウザで開く"),
        }
    }
}

/// Map a keyed [`kagi_domain::plan::BranchNameError`] to localized text.
pub fn branch_name_error(e: &kagi_domain::plan::BranchNameError) -> String {
    use kagi_domain::plan::BranchNameError::*;
    match e {
        EmptyCreate => Msg::BranchNameEmpty.t().to_string(),
        Required => Msg::BranchNameRequired.t().to_string(),
        Whitespace => Msg::BranchNameWhitespace.t().to_string(),
        SameName => Msg::BranchNameSame.t().to_string(),
        RenameExists(name) => branch_rename_exists_fmt(name),
        RenameInvalid(name) => branch_rename_invalid_fmt(name),
        CreateInvalidRef(name) => branch_invalid_ref_fmt(name),
        CreateLeadingDash(name) => branch_leading_dash_fmt(name),
        CreateExists(name) => branch_exists_fmt(name),
        CreateNamespaceCollision { name, existing } => match lang() {
            Lang::En => e.to_string(),
            Lang::Ja => format!(
                "既存の ref `{}` と名前空間が競合しています。\nbranch `{}`",
                existing, name
            ),
        },
    }
}

/// Accessible action name for the Inspector's short SHA chip.
pub fn inspector_copy_hash(short_sha: &str) -> String {
    match lang() {
        Lang::En => format!("Copy commit hash {short_sha}"),
        Lang::Ja => format!("commit hash {short_sha} をコピー"),
    }
}

/// Inspector files-list truncation indicator: "… and N more".
pub fn and_n_more(n: usize) -> String {
    match lang() {
        Lang::En => format!("\u{2026} and {} more", n),
        Lang::Ja => format!("\u{2026} ほか {} 件", n),
    }
}

/// Tab loading placeholder: "Loading <name>…". The repo/branch name stays
/// verbatim per ADR-0048.
pub fn loading_fmt(name: &str) -> String {
    match lang() {
        Lang::En => format!("Loading {}\u{2026}", name),
        Lang::Ja => format!("{} を読み込み中\u{2026}", name),
    }
}

/// Branch-menu copy toast: "Copied <value>". The copied value stays verbatim.
pub fn copied_fmt(value: &str) -> String {
    match lang() {
        Lang::En => format!("Copied {}", value),
        Lang::Ja => format!("{} をコピーしました", value),
    }
}

/// Toast for one theme file that failed to load (#922). `detail` is the
/// loader's `"<file name>: <reason>"`, kept verbatim.
pub fn theme_load_failed_fmt(detail: &str) -> String {
    match lang() {
        Lang::En => format!("Couldn't load theme {}", detail),
        Lang::Ja => format!("テーマを読み込めませんでした: {}", detail),
    }
}

/// Toast after an explicit theme reload (#922): how many custom themes loaded.
pub fn themes_reloaded_fmt(custom: usize) -> String {
    match lang() {
        Lang::En => format!("Themes reloaded ({} custom)", custom),
        Lang::Ja => format!("テーマを再読み込みしました（自作 {} 件）", custom),
    }
}

/// Error toast after trying to create or open the theme folder (#922).
pub fn themes_folder_failed_fmt(detail: &str) -> String {
    match lang() {
        Lang::En => format!("Couldn't open theme folder: {detail}"),
        Lang::Ja => format!("テーマフォルダーを開けませんでした: {detail}"),
    }
}

/// Smart Commit model-picker note when a model is selected but Ollama is not
/// running: "<model> — start Ollama to switch". The model name stays verbatim.
pub fn smart_model_switch_note(model: &str) -> String {
    match lang() {
        Lang::En => format!("{} — start Ollama to switch", model),
        Lang::Ja => format!("{}。切り替えるには Ollama を起動してください", model),
    }
}

/// Commit footer help names the actual backend; only Ollama owns a Kagi model.
pub fn smart_footer_provider(provider: &str, model: Option<&str>, generating: bool) -> String {
    let provider = kagi_domain::text_safety::sanitize_control_bytes(provider);
    let name = match model.filter(|model| !model.is_empty()) {
        Some(model) => format!(
            "{} ({})",
            provider,
            kagi_domain::text_safety::sanitize_control_bytes(model)
        ),
        None => provider,
    };
    if generating {
        Msg::SmartGeneratingProvider.t().replace("{}", &name)
    } else {
        match lang() {
            Lang::En => format!("Generate commit message with {name}"),
            Lang::Ja => format!("{name} で commit メッセージを生成"),
        }
    }
}

/// Smart Commit provider chip hint when a CLI provider is not on `$PATH`:
/// "<name> (not found on PATH)". The provider display name stays verbatim.
pub fn provider_not_found_hint(name: &str) -> String {
    match lang() {
        Lang::En => format!("{} (not found on PATH)", name),
        Lang::Ja => format!("{}(PATH に見つかりません)", name),
    }
}

/// Smart Commit CLI-provider warning heading (ADR-0099). The provider display
/// name stays verbatim.
pub fn smart_cli_warning_title(name: &str) -> String {
    match lang() {
        Lang::En => format!("⚠ {} sends your staged diff to an external service", name),
        Lang::Ja => format!("⚠ {} は staged な diff を外部サービスに送信します", name),
    }
}

/// Smart Commit CLI-provider warning bullet lines (ADR-0099). `name` is the
/// provider display name; `bin` its CLI binary — both stay verbatim.
pub fn smart_cli_warning_lines(name: &str, bin: &str) -> [String; 4] {
    match lang() {
        Lang::En => [
            format!(
                "Your staged diff is sent to the external `{}` CLI, which may send it outside this computer.",
                bin
            ),
            format!(
                "It uses YOUR {} account and consumes YOUR usage quota — each generation may incur cost.",
                name
            ),
            "The CLI can't change your repository."
                .to_string(),
            format!("Requires the `{}` CLI to be installed and logged in.", bin),
        ],
        Lang::Ja => [
            format!(
                "stage 済みの diff は外部の `{}` CLI に送られ、このコンピューターの外に送信される可能性があります。",
                bin
            ),
            format!(
                "あなたの {} アカウントを使い、利用量を消費します。生成ごとに費用が発生する場合があります。",
                name
            ),
            "CLI がリポジトリを変更することはありません。"
                .to_string(),
            format!("`{}` CLI のインストールとログインが必要です。", bin),
        ],
    }
}

/// Map a keyed [`kagi_domain::plan::WorktreePathError`] to localized text.
pub fn worktree_path_error(e: &kagi_domain::plan::WorktreePathError) -> String {
    use kagi_domain::plan::WorktreePathError::*;
    match e {
        Empty => Msg::WorktreePathEmpty.t().to_string(),
        Exists(path) => worktree_exists_fmt(path),
    }
}

/// Localized graph label for a detached worktree HEAD.
pub fn graph_detached_worktree_label(short_oid: &str) -> String {
    format!("{} {}", Msg::GraphDetachedWorktree.t(), short_oid)
}

/// Localized display heading; canonical section names remain stable identifiers.
pub fn menu_heading(section: &str, language: Lang) -> &str {
    if language == Lang::En {
        return section;
    }
    match section {
        "File" => "ファイル",
        "Edit" => "編集",
        "View" => "表示",
        "Repository" => "リポジトリ",
        "Window" => "ウインドウ",
        "Help" => "ヘルプ",
        "Theme" => "テーマ",
        "Language" => "言語",
        _ => section,
    }
}

/// Japanese label for a command-registry id (issue #352, command palette).
///
/// Returns `None` for ids whose English label is a domain word that stays
/// English in both languages per ADR-0048 (Fetch / Pull / Push / Cherry-pick /
/// Revert / theme + language names), or for unknown ids — the caller then falls
/// back to the registry's English `label`. Kept here (not in `commands.rs`)
/// because the crate layering (ADR-0121) keeps this pure crate below the UI's
/// command table, and translations belong with the rest of the i18n table.
pub fn command_label_ja(id: &str) -> Option<&'static str> {
    Some(match id {
        "app.about" => Msg::AboutTitle.t_for(Lang::Ja),
        "app.settings" => "設定…",
        "app.quit" => "kagi を終了",
        "app.hide" => Msg::MenuHideApp.t_for(Lang::Ja),
        "app.hideOthers" => Msg::MenuHideOthers.t_for(Lang::Ja),
        "app.showAll" => Msg::MenuShowAll.t_for(Lang::Ja),
        "file.newTab" => "新規タブ",
        "file.closeTab" => "タブを閉じる",
        "file.cloneRepository" => "リポジトリを clone…",
        "file.openRepository" => "リポジトリを開く…",
        "file.openInTerminal" => "リポジトリをターミナルで開く",
        "file.connectRemote" => "remote ホストに接続…",
        "file.refresh" => "リポジトリを再読み込み",
        "repo.conflictAbort" => "操作を中止…",
        "repo.conflictSkip" => "このステップをスキップ…",
        "repo.conflictContinue" => "確認へ進む…",
        "view.zoomIn" => "拡大",
        "view.zoomOut" => "縮小",
        "view.zoomReset" => "実際のサイズ",
        "view.fullScreen" => "フルスクリーンにする",
        "view.toggleSidebar" => "サイドバーの表示切替",
        "view.togglePrSwimlane" => "PR Graph の表示切替",
        "view.toggleTerminal" => "ターミナルの表示切替",
        "view.toggleCommitDetails" => "commit 詳細の表示切替",
        "view.toggleEditorDetails" => "エディタの差分・変更履歴の表示切替",
        "view.toggleDiffView" => "diff 表示の切替",
        "view.toggleEditorWorkspace" => "エディタワークスペースの切替",
        "view.togglePrMode" => "Pull Request モード",
        "view.showGraph" => "グラフモード",
        "view.commandPalette" => "コマンドパレット…",
        "view.reloadThemes" => "テーマを再読み込み",
        "repo.openInFinder" => "Finder で開く",
        "branch.new" => "新規 branch…",
        "branch.checkout" => "branch を checkout…",
        "branch.rename" => "branch の名前を変更…",
        "branch.delete" => "branch を削除…",
        "branch.copySelectedName" => "選択した commit の branch 名をコピー",
        "commit.copyHash" => "commit ハッシュをコピー",
        "commit.checkout" => "commit を checkout",
        "commit.createBranch" => "commit から branch を作成…",
        "commit.reset" => "HEAD を commit に reset…",
        "commit.compareWorkingTree" => "working tree と比較",
        "window.minimize" => "最小化",
        "window.zoom" => "ズーム",
        "window.previousTab" => Msg::MenuPreviousTab.t_for(Lang::Ja),
        "window.nextTab" => Msg::MenuNextTab.t_for(Lang::Ja),
        "window.bringAllToFront" => Msg::MenuBringAllToFront.t_for(Lang::Ja),
        "window.new" => "新規ウィンドウ",
        "window.close" => "ウィンドウを閉じる",
        "help.shortcuts" => "キーボードショートカット",
        "help.documentation" => "ドキュメント",
        "help.reportIssue" => "問題を報告",
        // Domain words / proper nouns stay English (ADR-0048): repo.fetch,
        // repo.pull, repo.push, commit.cherryPick, commit.revert, theme.*,
        // lang.* — return None so the caller uses the English registry label.
        _ => return None,
    })
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────
#[cfg(test)]
#[path = "i18n_tests.rs"]
mod tests;

#[cfg(test)]
mod input_name_tests {
    use super::*;

    #[test]
    fn input_names_are_nonempty_and_localized() {
        for message in [
            Msg::A11yCommandPaletteQuery,
            Msg::A11yCommitSummary,
            Msg::A11yCommitDescription,
            Msg::A11yConflictResult,
            Msg::A11yIssueTitle,
            Msg::A11yIssueBody,
            Msg::A11yCommentBody,
            Msg::A11yHomeFilter,
            Msg::A11yListTitleFilter,
            Msg::A11ySidebarFilter,
            Msg::A11ySidebarClearSearch,
            Msg::A11yPrReviewersFilter,
            Msg::A11yPrAssigneesFilter,
            Msg::A11yPrLabelsFilter,
            Msg::A11yStashMessage,
            Msg::A11yAnalyzeIgnorePatterns,
            Msg::A11yEditorFileContents,
        ] {
            assert!(!message.t_for(Lang::En).trim().is_empty());
            assert!(!message.t_for(Lang::Ja).trim().is_empty());
            assert_ne!(message.t_for(Lang::En), message.t_for(Lang::Ja));
        }
    }
}

#[cfg(test)]
mod standard_app_menu_tests {
    use super::*;

    #[test]
    fn standard_app_menu_labels_en_ja() {
        for (message, en, ja) in [
            (Msg::MenuHideApp, "Hide Kagi", "Kagiを隠す"),
            (Msg::MenuHideOthers, "Hide Others", "ほかを隠す"),
            (Msg::MenuShowAll, "Show All", "すべてを表示"),
            (Msg::MenuServices, "Services", "サービス"),
        ] {
            assert_eq!(message.t_for(Lang::En), en);
            assert_eq!(message.t_for(Lang::Ja), ja);
        }
    }
}

/// Read-opening list rows preserve literal braces in external titles/names.
pub fn issue_row_name(number: u64, title: &str, state: &str) -> String {
    use std::fmt::Write as _;
    let template = Msg::A11yIssueOpenRow.t();
    let mut parts = template.split("{}");
    let mut name = String::with_capacity(template.len() + 20 + title.len() + state.len());
    name.push_str(parts.next().unwrap_or_default());
    let _ = write!(name, "{number}");
    name.push_str(parts.next().unwrap_or_default());
    name.push_str(title);
    name.push_str(parts.next().unwrap_or_default());
    name.push_str(state);
    name.push_str(parts.next().unwrap_or_default());
    name
}

pub fn recent_row_name(name: &str) -> String {
    Msg::A11yRecentOpenRow.t().replace("{name}", name)
}

#[cfg(test)]
mod rebase_plan_label_tests {
    use super::*;

    #[test]
    fn rebase_plan_labels_name_source_not_destination() {
        for branch in ["feature", "main"] {
            assert_eq!(
                Msg::PlanRebaseOnto.t_for(Lang::En).replace("{}", branch),
                format!("Rebase {branch}")
            );
            assert_eq!(
                Msg::PlanRebaseOnto.t_for(Lang::Ja).replace("{}", branch),
                format!("{branch} を rebase")
            );
            assert_eq!(
                Msg::PlanReplayOnto.t_for(Lang::En).replace("{}", branch),
                format!("Replay {branch}")
            );
            assert_eq!(
                Msg::PlanReplayOnto.t_for(Lang::Ja).replace("{}", branch),
                format!("{branch} を replay")
            );
        }
    }
}

#[cfg(test)]
mod toast_count_tests {
    use super::*;

    #[test]
    fn older_notification_caption_has_a_singular_english_form() {
        assert_eq!(Msg::ToastMoreOne.t_for(Lang::En), "1 more notification");
        assert_eq!(Msg::ToastMoreOne.t_for(Lang::Ja), "ほか1件の通知");
        assert_eq!(
            Msg::ToastMore.t_for(Lang::En).replace("{count}", "3"),
            "3 more notifications"
        );
    }
}
