//! W22-I18N / ADR-0048: dependency-free UI localization (English / Japanese).
//!
//! Wave 1 covers the **UI layer** (`src/ui/`) only: modal explanatory /
//! confirmation / recovery prose, toasts, Busy-footer texts, guard messages,
//! the WIP row note, empty states, and the few hardcoded Japanese strings that
//! pre-dated this module.  `src/git/` plan blocker/warning/recovery strings are
//! pinned by tests and are **wave 2** — untouched here.
//!
//! # Design (same shape as [`super::theme`])
//!
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
//!
//! # Domain words stay English
//!
//! Per ADR-0048, domain words (Pull / Push / Branch / Stash / Pop / Undo /
//! Terminal / Commit / amend / checkout / cherry-pick / revert / discard /
//! worktree / tag …), single-word action buttons, column headers, SHAs and
//! branch names are **not** translated; they appear verbatim inside both the
//! `En` and `Ja` arms below.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::settings::{read_setting, write_setting};

// ──────────────────────────────────────────────────────────────────────────
// Lang + active-language atomic
// ──────────────────────────────────────────────────────────────────────────

/// UI language.  `En` is index 0 (the default), `Ja` is index 1.
pub mod op;
pub mod plan;
pub use op::{op_failed, op_plan_failed, Op};
pub use plan::{plan_note_text, plan_recovery_text, plan_title_text};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

impl Lang {
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
    // ── Generic guards / footers ────────────────────────────────────
    /// "another operation is in progress" (was "別の操作が実行中です").
    OpInProgress,
    NoRepoOpen,
    NoTabsOpen,
    NoCommitSelected,
    DiffNotOpen,
    /// ADR-0124: diff-header toggle tooltip → switch to side-by-side.
    DiffViewSplit,
    /// ADR-0124: diff-header toggle tooltip → switch back to unified.
    DiffViewUnified,
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
    CloneUnimplemented,
    MultiWindowUnsupported,
    ResetUnimplemented,

    // ── Busy footers (op in flight) ─────────────────────────────────
    BusyCheckout,
    BusySwitchToLatest,
    BusyPull,
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
    /// Footer label when a discard mutated the working tree but did not finish
    /// (issue #281). The backup blob SHAs are in the oplog entry.
    DiscardPartial,
    BusyCommit,
    BusyCreateWorktree,
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
    /// Default lock reason kagi records for a manual lock (issue #340).
    WorktreeLockDefaultReason,
    BusyMerge,

    // ── Operation no-op toasts ──────────────────────────────────────
    // (Per-op "started" toasts were removed: the unified busy snackbar —
    // driven by `busy_op` with a spinning sync icon — now signals progress.)
    AlreadyUpToDatePull,
    AlreadyUpToDatePush,

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
    PopEmpty,
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
    CmResetUnimplemented,
    BcmBusy,
    BcmNotImplementedYet,
    BcmCurrentBranch,
    BcmOnlyFromCurrentBranch,
    BcmNoUpstream,
    BcmDetachedHead,
    BcmCheckedOutElsewhere,
    BcmConflictMode,
    BcmNothingToPull,
    BcmNothingToPush,

    // ── Empty states ────────────────────────────────────────────────
    NoLocalBranches,
    NoOperationsYet,

    // ── Command palette (issue #352) ─────────────────────────────────
    /// Placeholder text in the palette's search box.
    CommandPalettePlaceholder,
    /// Shown when the query matches no command.
    CommandPaletteNoResults,

    // ── Misc footers ────────────────────────────────────────────────
    Refreshed,
    /// Toast after a manual working-tree snapshot (ADR-0154 / #335).
    SnapshotCreated,
    /// Toast prefix when a manual snapshot fails (ADR-0154 / #335).
    SnapshotFailed,
    OpenedInFinder,
    /// Label on the "load more commits" row at the bottom of the commit list.
    LoadMoreCommits,

    // ── W30-CONFLICT-UI: Conflict Mode (banner / list / choose / preview) ──
    // Operation headings (domain words rebase/merge/cherry-pick/revert kept
    // English per ADR-0048; only the surrounding prose is localized).
    // Banner buttons + progress.
    ConflictContinue,
    ConflictAbort,
    ConflictSkip,
    /// Conflict dashboard: armed label on the two-stage Skip button.
    ConflictConfirmSkip,
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
    ConflictDirFileHint,
    ConflictResultPreview,
    ConflictPreviewHint,
    ConflictBinaryNoPreview,
    // ── #321: binary / symlink / submodule side viewer ──
    ConflictSymlinkTarget,
    ConflictOpenBothExternal,
    ConflictBinaryCompareHint,
    ConflictImageTooLarge,
    ConflictSubmoduleCommit,
    // ── W32-CONFLICT-EDITOR: hunk-level Conflict Editor ──────
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
    EditorMarkerWarning,
    EditorSavedResolved,
    EditorNoTextMerge,
    // ── T-CONFLICT-UI/UX: 3-pane editor controls ──
    EditorResetAllConfirm,
    EditorPreviewMode,
    EditorEditMode,
    EditorEditingIndicator,
    // ── T-CONFLICT-UX-010/012: per-hunk accept controls ──
    EditorHunkLabel,
    EditorCurrentFirst,
    EditorIncomingFirst,
    // ── W33-CONFLICT-DASHBOARD: Right-panel dashboard + escape hatch ──
    ConflictDashHeader,
    ConflictRoleCurrent,
    ConflictRoleIncoming,
    ConflictGitTermHint,
    ConflictConflictedCount,
    ConflictResolvedCount,
    ConflictSectionConflicted,
    ConflictSectionResolved,
    ConflictConfirmAbort,
    ConflictConfirmAbortHint,
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
    /// Commit panel: tooltip on the sparkles (AI message) icon.
    GenerateMessage,
    /// Commit panel: tooltip on the person+ (co-author) icon.
    AddCoAuthor,
    /// Commit panel: tooltip on the amend icon.
    AmendLastCommit,
    /// Commit panel: co-author picker with nothing to offer.
    NoRecentAuthors,
    /// Conflict editor: pane-header pill that takes the whole file's side.
    EditorUseAll,
    /// Inspector file context menu: open the file in the Editor Workspace.
    MenuOpenInEditor,
    /// Inspector file context menu: show this file's history.
    MenuShowFileHistory,
    /// Context menus: open the file in the user's external editor.
    OpenInExternalEditor,
    /// Sidebar PR row: the head branch is not fetched locally.
    PrBranchNotFetched,
    /// PR context menu.
    PrOpenOnGitHub,
    PrCopyUrl,
    PrPeek,
    PrJumpToBranch,
    /// PR row tooltip fragments.
    PrDraft,
    PrStacked,
    /// PR pane (takeover).
    PrPaneTitle,
    PrPaneEmpty,
    PrGroupMine,
    PrGroupReview,
    PrGroupOthers,
    PrReviewApproved,
    PrReviewChanges,
    PrReviewRequired,
    /// PR mode.
    PrModeExit,
    PrModeSelectHint,
    PrModeNoFile,
    PrModeStack,
    PrModeFiles,
    PrModeShowDescription,
    PrModeDescription,
    PrModeCommits,
    /// Focus Queue buckets.
    PrQueueNeedsYou,
    PrQueueInProgress,
    PrQueueReady,
    PrQueueWaiting,
    PrQueueDormant,
    /// Focus Queue reasons.
    PrWhyChangesRequested,
    PrWhyConflicting,
    PrWhyCiRunning,
    PrWhyReadyToMerge,
    PrWhyReviewRequested,
    PrWhyAwaitingReview,
    /// Right rail + tabs.
    PrModeChecks,
    PrModeReview,
    PrModeOverview,
    PrModeNoReview,
    PrModeMerge,
    PrModeMergeDone,
    PrSuggestion,
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
    /// PR mode: header of a grouped stack of dependent PRs.
    PrStack,
    /// PR mode: toast shown while that fetch runs.
    PrRefreshing,
    /// Commit panel: tooltip on the commit.template toggle.
    ToggleCommitTemplate,
    /// Commit panel: label on the commit.template toggle.
    Template,
    /// Footer idle status.
    Ready,
    /// Welcome screen help line.
    NoRepositoryOpenWelcome,
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
    /// Settings sidebar: Appearance page title.
    SettingsAppearance,
    /// Settings sidebar: Language page title.
    SettingsLanguage,
    /// Appearance → Theme row title.
    SettingsTheme,
    /// Appearance → Theme row description.
    SettingsThemeDesc,
    /// Appearance → UI Zoom row title.
    SettingsZoom,
    /// Appearance → UI Zoom row description.
    SettingsZoomDesc,
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
    /// Confirm button on the set-upstream modal, `{}` = branch.
    PlanSetUpstreamFor,
    /// Confirm button on the rename-branch modal, `{}` = old name.
    PlanRenameBranch,
    /// Confirm button on the merge modal.
    PlanMerge,
    /// Confirm button on the rebase modal, `{}` = branch.
    PlanRebaseOnto,
    /// Delete-remote-branch confirm, and its armed second stage.
    PlanDeleteRemoteBranch,
    PlanDeleteRemoteBranchArmed,
    /// Reset-current confirm, and its armed second stage.
    PlanResetCurrent,
    PlanResetCurrentArmed,
    /// Force-with-lease push confirm, and its armed second stage.
    PlanForcePush,
    PlanForcePushArmed,
    /// Cancel button, shared by every modal.
    PlanCancel,
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
    /// Unsaved-changes confirmation: destructive "discard and proceed" button.
    EditorWorkspaceDiscard,
    /// Unsaved-changes confirmation: cancel button (stay on the buffer).
    EditorWorkspaceCancel,
    /// Editor Workspace header: the "← Graph" back button.
    EditorWorkspaceBackToGraph,
    /// Editor Workspace header: the "Editor Workspace" title label.
    EditorWorkspaceTitle,

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
    /// Snapshot tab placeholder when the load finished but found nothing
    /// (e.g. the path didn't exist yet at that commit).
    EditorSnapshotUnavailable,

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
    EditorWorkspaceMermaidRendering,
    EditorWorkspaceMermaidNeedsCli,
    EditorWorkspaceMermaidFailed,
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
}

impl Msg {
    /// Resolve this message to a `&'static str` in the active [`lang()`].
    ///
    /// Domain words (Pull / Push / Branch / Stash / Pop / Undo / amend /
    /// checkout / cherry-pick / revert / discard / worktree / HEAD / branch /
    /// upstream / stash …) appear verbatim in both arms per ADR-0048.
    pub fn t(self) -> &'static str {
        use Lang::{En, Ja};
        use Msg::*;
        match (lang(), self) {
            // ── issue #346: ruleset badge ───────────────────────────
            (En, RulesetBadge) => "Ruleset",
            (Ja, RulesetBadge) => "Ruleset",
            (En, RulesetBadgeTooltip) => "This may violate the branch ruleset:",
            (Ja, RulesetBadgeTooltip) => "ブランチの ruleset に違反する可能性があります:",

            // ── Generic guards ──────────────────────────────────────
            (En, OpInProgress) => "another operation is in progress",
            (Ja, OpInProgress) => "別の操作が実行中です",
            (En, NoRepoOpen) => "no repository is open",
            (Ja, NoRepoOpen) => "リポジトリが開かれていません",
            (En, NoTabsOpen) => "no open tabs",
            (Ja, NoTabsOpen) => "開いているタブがありません",
            (En, NoCommitSelected) => "no commit selected",
            (Ja, NoCommitSelected) => "commit が選択されていません",
            (En, DiffNotOpen) => "no diff is open",
            (Ja, DiffNotOpen) => "diff が開かれていません",
            (En, DiffViewSplit) => "Side-by-side view",
            (Ja, DiffViewSplit) => "横並びで表示",
            (En, DiffViewUnified) => "Unified view",
            (Ja, DiffViewUnified) => "1カラムで表示",
            (En, CommitHeaderSelectPrompt) => "Select a commit to see its details.",
            (Ja, CommitHeaderSelectPrompt) => "commit を選択すると詳細が表示されます。",

            // ── Agent provenance (issue #337) ───────────────────────
            (En, AgentReviewed) => "reviewed",
            (Ja, AgentReviewed) => "レビュー済み",
            (En, AgentCreatedTooltip) => "Created by an AI agent",
            (Ja, AgentCreatedTooltip) => "AI エージェントによる作成",
            (En, AgentReviewedTooltip) => "Created by an AI agent, reviewed by a human",
            (Ja, AgentReviewedTooltip) => "AI エージェントが作成 / 人がレビュー済み",

            // ── Placeholders ────────────────────────────────────────
            (En, CloneUnimplemented) => "clone is not implemented yet",
            (Ja, CloneUnimplemented) => "clone は未実装です",
            (En, MultiWindowUnsupported) => "multiple windows are not supported",
            (Ja, MultiWindowUnsupported) => "複数ウィンドウは未対応です",
            (En, ResetUnimplemented) => "reset is not implemented (ADR-0024)",
            (Ja, ResetUnimplemented) => "reset は未実装です (ADR-0024)",

            // ── Busy footers ────────────────────────────────────────
            (En, BusyCheckout) => "checkout in progress…",
            (Ja, BusyCheckout) => "checkout 実行中…",
            (En, BusySwitchToLatest) => "switching to latest…",
            (Ja, BusySwitchToLatest) => "最新へ切り替え中…",
            (En, BusyPull) => "pull in progress…",
            (Ja, BusyPull) => "pull 実行中…",
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
                "コンフリクトありで適用しました — stash は保持されています。コンフリクトを解決してから手動で drop してください。"
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
            (En, DiscardPartial) => "discard only partially applied (backup blobs are in the oplog)",
            (Ja, DiscardPartial) => "discard は一部しか適用されていません (backup blob は oplog に記録済み)",
            (En, BusyCommit) => "commit in progress…",
            (Ja, BusyCommit) => "commit 実行中…",
            (En, BusyCreateWorktree) => "create worktree in progress…",
            (Ja, BusyCreateWorktree) => "create worktree 実行中…",
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
            (En, WorktreeLockDefaultReason) => "locked in kagi",
            (Ja, WorktreeLockDefaultReason) => "locked in kagi",
            (En, BusyMerge) => "merge in progress…",
            (Ja, BusyMerge) => "merge 実行中…",

            // ── No-op toasts ─────────────────────────────────────────
            (En, AlreadyUpToDatePull) => "Already up to date — nothing to pull",
            (Ja, AlreadyUpToDatePull) => "すでに最新です — pull するものはありません",
            (En, AlreadyUpToDatePush) => "Already up to date — nothing to push",
            (Ja, AlreadyUpToDatePush) => "すでに最新です — push するものはありません",

            // ── Toolbar guards ──────────────────────────────────────
            (En, PullBusy) => "Pull: another operation is in progress",
            (Ja, PullBusy) => "Pull: 別の操作が実行中です",
            (En, PullDetached) => "Pull: detached HEAD — switch to a branch first",
            (Ja, PullDetached) => "Pull: detached HEAD — branch に切り替えてください",
            (En, PullUnborn) => "Pull: no commits yet — no upstream",
            (Ja, PullUnborn) => "Pull: no commits yet — upstream がありません",
            (En, PullNoUpstream) => "Pull: no upstream is configured (no upstream)",
            (Ja, PullNoUpstream) => "Pull: upstream が設定されていません (no upstream)",
            (En, PullNothing) => "Pull: nothing to pull (behind=0)",
            (Ja, PullNothing) => "Pull: nothing to pull (behind=0)",
            (En, PushBusy) => "Push: another operation is in progress",
            (Ja, PushBusy) => "Push: 別の操作が実行中です",
            (En, PushDetached) => "Push: detached HEAD — switch to a branch first",
            (Ja, PushDetached) => "Push: detached HEAD — branch に切り替えてください",
            (En, PushUnborn) => "Push: no commits yet — no upstream",
            (Ja, PushUnborn) => "Push: no commits yet — upstream がありません",
            (En, PushNoRemote) => "Push: no upstream and no remote configured",
            (Ja, PushNoRemote) => "Push: no upstream and no remote configured",
            (En, PushNothing) => "Push: nothing to push (ahead=0)",
            (Ja, PushNothing) => "Push: nothing to push (ahead=0)",
            (En, StashClean) => "Stash: working tree is clean — nothing to stash",
            (Ja, StashClean) => "Stash: working tree is clean — nothing to stash",
            (En, PopEmpty) => "Pop: stash is empty",
            (Ja, PopEmpty) => "Pop: stash が空です",
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

            // ── Context-menu disabled reasons ───────────────────────
            (En, CmDetachedHead) => "detached HEAD",
            (Ja, CmDetachedHead) => "detached HEAD",
            (En, CmSameAsHead) => "same as HEAD",
            (Ja, CmSameAsHead) => "HEAD と同一",
            (En, CmMergeUnsupported) => "merge commit is out of MVP scope",
            (Ja, CmMergeUnsupported) => "merge commit は MVP 対象外",
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
            (En, CmResetUnimplemented) => "reset is not implemented in MVP",
            (Ja, CmResetUnimplemented) => "MVP では reset は未実装",
            (En, BcmBusy) => "another operation is in progress",
            (Ja, BcmBusy) => "別の操作が実行中です",
            (En, BcmNotImplementedYet) => "not implemented yet",
            (Ja, BcmNotImplementedYet) => "未実装です",
            (En, BcmCurrentBranch) => "current branch",
            (Ja, BcmCurrentBranch) => "現在 branch",
            (En, BcmOnlyFromCurrentBranch) => "only available from the current branch",
            (Ja, BcmOnlyFromCurrentBranch) => "現在の branch からのみ実行できます",
            (En, BcmNoUpstream) => "no upstream configured",
            (Ja, BcmNoUpstream) => "upstream が設定されていません",
            (En, BcmDetachedHead) => "detached HEAD",
            (Ja, BcmDetachedHead) => "detached HEAD",
            (En, BcmCheckedOutElsewhere) => "branch is checked out in another worktree",
            (Ja, BcmCheckedOutElsewhere) => "branch は別の worktree で checkout 済みです",
            (En, BcmConflictMode) => "resolve conflicts first",
            (Ja, BcmConflictMode) => "先に conflict を解決してください",
            (En, BcmNothingToPull) => "nothing to pull",
            (Ja, BcmNothingToPull) => "pull するものがありません",
            (En, BcmNothingToPush) => "nothing to push",
            (Ja, BcmNothingToPush) => "push するものがありません",

            // ── Empty states ────────────────────────────────────────
            (En, NoLocalBranches) => "No local branches",
            (Ja, NoLocalBranches) => "ローカル branch がありません",
            (En, CommandPalettePlaceholder) => "Search commands…",
            (Ja, CommandPalettePlaceholder) => "コマンドを検索…",
            (En, CommandPaletteNoResults) => "No matching commands",
            (Ja, CommandPaletteNoResults) => "一致するコマンドがありません",
            (En, NoOperationsYet) => "No operations yet",
            (Ja, NoOperationsYet) => "操作履歴はまだありません",

            // ── Misc footers ────────────────────────────────────────
            (En, Refreshed) => "Refreshed",
            (Ja, Refreshed) => "更新しました",
            (En, SnapshotCreated) => "Snapshot created",
            (Ja, SnapshotCreated) => "スナップショットを作成しました",
            (En, SnapshotFailed) => "Snapshot failed",
            (Ja, SnapshotFailed) => "スナップショットの作成に失敗しました",
            (En, LoadMoreCommits) => "Load more commits",
            (Ja, LoadMoreCommits) => "commit をさらに読み込む",
            (En, OpenedInFinder) => "Opened in Finder",
            (Ja, OpenedInFinder) => "Finder で開きました",

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
            (Ja, ConflictKindSubmodule) => "サブモジュール",
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
            (En, ConflictKeepDirectory) => "Keep directory",
            (Ja, ConflictKeepDirectory) => "ディレクトリを採用",
            (En, ConflictKeepFile) => "Keep file",
            (Ja, ConflictKeepFile) => "ファイルを採用",
            (En, ConflictDirFileHint) => {
                "One side is a directory, the other a file — keep exactly one."
            }
            (Ja, ConflictDirFileHint) => "片方はディレクトリ、片方はファイルです。どちらか一方を採用します。",
            (En, ConflictResultPreview) => "Result preview",
            (Ja, ConflictResultPreview) => "解決結果プレビュー",
            (En, ConflictPreviewHint) => "Choose a side above to preview the resolved file.",
            (Ja, ConflictPreviewHint) => "上のボタンで側を選ぶと解決後のファイルをプレビューします。",
            (En, ConflictBinaryNoPreview) => "Binary file — choose a side; no text preview is available.",
            (Ja, ConflictBinaryNoPreview) => "binary ファイル — 側を選択してください。テキストプレビューはありません。",
            (En, ConflictSymlinkTarget) => "Symlink target",
            (Ja, ConflictSymlinkTarget) => "シンボリックリンクの参照先",
            (En, ConflictOpenBothExternal) => "Open both sides in external editor",
            (Ja, ConflictOpenBothExternal) => "両方の側を外部エディタで開く",
            (En, ConflictBinaryCompareHint) => {
                "Binary file — no inline preview. Open both sides to compare."
            }
            (Ja, ConflictBinaryCompareHint) => {
                "binary ファイル — インラインプレビューはありません。両方の側を開いて比較してください。"
            }
            (En, ConflictImageTooLarge) => "Image too large to preview inline.",
            (Ja, ConflictImageTooLarge) => "画像が大きすぎてインラインプレビューできません。",
            (En, ConflictSubmoduleCommit) => "Submodule commit",
            (Ja, ConflictSubmoduleCommit) => "サブモジュールのコミット",
            // ── W32-CONFLICT-EDITOR ──────────────────────────────────
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
            (Ja, EditorReset) => "すべて reset",
            (En, EditorSave) => "Save resolution",
            (Ja, EditorSave) => "解決を保存",
            (En, EditorResultOutput) => "Result / Output",
            (Ja, EditorResultOutput) => "解決結果 / 出力",
            (En, EditorAllResolved) => "All hunks resolved",
            (Ja, EditorAllResolved) => "すべての hunk を解決しました",
            (En, EditorUnresolvedHunks) => "hunk(s) still unresolved",
            (Ja, EditorUnresolvedHunks) => "件の hunk が未解決です",
            (En, EditorMarkerWarning) => "Conflict markers remain — saved as a draft, but you cannot continue until they are removed.",
            (Ja, EditorMarkerWarning) => "conflict marker が残っています — 下書きとして保存しましたが、削除するまで continue できません。",
            (En, EditorSavedResolved) => "Saved. File marked as a resolved candidate.",
            (Ja, EditorSavedResolved) => "保存しました。ファイルを resolved candidate にしました。",
            (En, EditorNoTextMerge) => "No text merge is available for this file (binary or single-sided). Use the conflict list to choose a side.",
            (Ja, EditorNoTextMerge) => "このファイルはテキスト merge できません(binary / 片側のみ)。conflict 一覧で側を選択してください。",
            // ── T-CONFLICT-UI/UX: 3-pane editor controls ──
            (En, EditorResetAllConfirm) => "Click again to reset all",
            (Ja, EditorResetAllConfirm) => "もう一度押すと全 reset",
            (En, EditorPreviewMode) => "Preview",
            (Ja, EditorPreviewMode) => "プレビュー",
            (En, EditorEditMode) => "Edit",
            (Ja, EditorEditMode) => "編集",
            (En, EditorEditingIndicator) => "editing",
            (Ja, EditorEditingIndicator) => "編集中",
            // ── T-CONFLICT-UX-010/012: per-hunk accept controls ──
            (En, EditorHunkLabel) => "Hunk",
            (Ja, EditorHunkLabel) => "Hunk",
            // Deliberately English in BOTH languages: the JA reading ("現在の
            // 方を先") was more confusing than the English (user feedback), and
            // the arrow shows what "first" even means — the order the two
            // blocks land in the result when both sides are taken.
            (En | Ja, EditorCurrentFirst) => "Current → Incoming",
            (En | Ja, EditorIncomingFirst) => "Incoming → Current",
            // ── W33-CONFLICT-DASHBOARD ───────────────────────────────
            (En, ConflictDashHeader) => "Merge conflicts detected",
            (Ja, ConflictDashHeader) => "conflict が検出されました",
            (En, ConflictRoleCurrent) => "Current",
            (Ja, ConflictRoleCurrent) => "現在の側",
            (En, ConflictRoleIncoming) => "Incoming",
            (Ja, ConflictRoleIncoming) => "取り込む側",
            (En, ConflictGitTermHint) => "internal git stage",
            (Ja, ConflictGitTermHint) => "内部 git stage",
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
            (En, ConflictConfirmSkipHint) => {
                "Skip drops this step's changes and your in-progress resolution.                  Click again to confirm."
            }
            (Ja, ConflictConfirmSkipHint) => {
                "スキップするとこのステップの変更と編集中の解決内容が失われます。                 もう一度クリックすると確定します。"
            }
            (En, ConflictConfirmAbort) => "Confirm abort",
            (Ja, ConflictConfirmAbort) => "中止を確定",
            (En, ConflictConfirmAbortHint) => {
                "Aborting may discard your saved resolutions (they are preserved in the autosave directory)."
            }
            (Ja, ConflictConfirmAbortHint) => {
                "中止すると保存済みの resolution が失われる可能性があります(autosave に退避されます)。"
            }
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
            (En, ConflictBlockerIndex) => "The index still has untracked unmerged entries.",
            (Ja, ConflictBlockerIndex) => "index に未追跡の unmerged エントリが残っています。",
            (En, ConflictBlockerMessage) => "The merge commit message is empty.",
            (Ja, ConflictBlockerMessage) => "merge commit のメッセージが空です。",
            (En, ConflictBlockerChecklist) => "A commit checklist rule is blocking continue.",
            (Ja, ConflictBlockerChecklist) => "commit checklist のルールが continue を妨げています。",
            (En, ConflictContinueReady) => "All conflicts resolved — ready to continue.",
            (Ja, ConflictContinueReady) => "すべて解決済み — continue できます。",
            (En, ConflictMore) => "More",
            (Ja, ConflictMore) => "その他",
            (En, ConflictNextConflict) => "Next conflict",
            (Ja, ConflictNextConflict) => "次の衝突へ",
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
            (Ja, CommitBody) => "説明（任意）",
            (En, GenerateMessage) => "Generate commit message",
            (Ja, GenerateMessage) => "commit メッセージを生成",
            (En, AddCoAuthor) => "Add co-author",
            (Ja, AddCoAuthor) => "共同作成者を追加",
            (En, AmendLastCommit) => "Amend last commit",
            (Ja, AmendLastCommit) => "直前の commit を修正",
            (En, NoRecentAuthors) => "No other authors in recent history",
            (En, EditorUseAll) => "Use all",
            (En, MenuOpenInEditor) => "Open in Editor",
            (Ja, MenuOpenInEditor) => "エディタで開く",
            (En, MenuShowFileHistory) => "Show File History",
            (En, OpenInExternalEditor) => "Open in External Editor",
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
            (En, PrStacked) => "stacked on",
            (Ja, PrStacked) => "積み上げ先",
            (En, PrPaneTitle) => "Pull Requests",
            (Ja, PrPaneTitle) => "pull request",
            (En, PrPaneEmpty) => "No pull requests match this filter.",
            (Ja, PrPaneEmpty) => "このフィルタに該当する PR はありません。",
            (En, PrGroupMine) => "Mine",
            (Ja, PrGroupMine) => "自分",
            (En, PrGroupReview) => "Review requested",
            (Ja, PrGroupReview) => "レビュー依頼",
            (En, PrGroupOthers) => "Others",
            (Ja, PrGroupOthers) => "その他",
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
            (En, PrModeStack) => "STACK",
            (Ja, PrModeStack) => "スタック",
            (En, PrModeFiles) => "FILES",
            (Ja, PrModeFiles) => "ファイル",
            (En, PrModeShowDescription) => "Show description",
            (En, PrModeDescription) => "DESCRIPTION",
            (Ja, PrModeDescription) => "説明",
            (En, PrModeCommits) => "COMMITS",
            (Ja, PrModeCommits) => "commit",
            (En, PrQueueNeedsYou) => "NEEDS YOU",
            (Ja, PrQueueNeedsYou) => "対応が必要",
            (En, PrQueueInProgress) => "IN PROGRESS",
            (Ja, PrQueueInProgress) => "進行中",
            (En, PrQueueReady) => "READY",
            (Ja, PrQueueReady) => "merge 可能",
            (En, PrQueueWaiting) => "WAITING",
            (Ja, PrQueueWaiting) => "待ち",
            (En, PrQueueDormant) => "OTHERS",
            (Ja, PrQueueDormant) => "その他",
            (En, PrWhyChangesRequested) => "Changes requested",
            (Ja, PrWhyChangesRequested) => "修正依頼",
            (En, PrWhyConflicting) => "Conflicts",
            (Ja, PrWhyConflicting) => "コンフリクト",
            (En, PrWhyCiRunning) => "CI running",
            (Ja, PrWhyCiRunning) => "CI 実行中",
            (En, PrWhyReadyToMerge) => "Ready to merge",
            (Ja, PrWhyReadyToMerge) => "merge 可能",
            (En, PrWhyReviewRequested) => "Your review requested",
            (Ja, PrWhyReviewRequested) => "レビュー依頼",
            (En, PrWhyAwaitingReview) => "Awaiting review",
            (Ja, PrWhyAwaitingReview) => "レビュー待ち",
            (En, PrModeChecks) => "CHECKS",
            (Ja, PrModeChecks) => "チェック",
            (En, PrModeReview) => "REVIEW",
            (Ja, PrModeReview) => "レビュー",
            (En, PrModeOverview) => "Overview",
            (Ja, PrModeOverview) => "概要",
            (En, PrModeNoReview) => "No reviews or comments yet.",
            (Ja, PrModeNoReview) => "レビュー・コメントはまだありません。",
            (En, PrModeMerge) => "Merge",
            (Ja, PrModeMerge) => "merge",
            (En, PrModeMergeDone) => "Merged",
            (Ja, PrModeMergeDone) => "merge しました",
            (En, PrSuggestion) => "suggestion",
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
            (Ja, PrModeShowDescription) => "説明文を表示",
            (En, PrModeNoDescription) => "No description.",
            (Ja, PrModeNoDescription) => "説明文はありません。",
            (En, PrMergeStatusHeading) => "Merge status",
            (Ja, PrMergeStatusHeading) => "merge の状態",
            (En, PrMergeActionConflict) => "Conflicts with the base — open the conflict view.",
            (Ja, PrMergeActionConflict) => "base と conflict しています — conflict を確認してください。",
            (En, PrMergeActionUpdateBranch) => "Behind the base — rebase or update the branch.",
            (Ja, PrMergeActionUpdateBranch) => "base に遅れています — rebase または update してください。",
            (En, PrMergeActionBlocked) => "Blocked — required checks are not yet met:",
            (Ja, PrMergeActionBlocked) => "blocked — 必要な条件が未達です:",
            (En, PrMergeActionUnstable) => "A non-required check is failing.",
            (Ja, PrMergeActionUnstable) => "必須でない check が失敗しています。",
            (En, PrMergeActionReady) => "Ready to merge.",
            (Ja, PrMergeActionReady) => "merge できます。",
            (En, PrMergeActionDraft) => "Draft — mark ready for review first.",
            (Ja, PrMergeActionDraft) => "draft です — まず ready にしてください。",
            (En, PrMergeActionWait) => "Waiting on GitHub to compute mergeability.",
            (Ja, PrMergeActionWait) => "GitHub が mergeability を計算中です。",
            (En, PrMergeMissingApprovals) => "approving reviews still needed",
            (Ja, PrMergeMissingApprovals) => "件の approve が必要",
            (En, PrMergeMissingCodeowner) => "CODEOWNERS review required from",
            (Ja, PrMergeMissingCodeowner) => "CODEOWNERS のレビューが必要:",
            (En, PrMergeUnresolvedThreads) => "unresolved review threads",
            (Ja, PrMergeUnresolvedThreads) => "件の未解決スレッド",
            (En, PrMergeQueueLabel) => "In merge queue — position",
            (Ja, PrMergeQueueLabel) => "merge queue 内 — 順位",
            (En, PrAllPrs) => "All PRs",
            (Ja, PrAllPrs) => "PR 一覧",
            (En, PrStack) => "STACK",
            (Ja, PrStack) => "スタック",
            (En, PrFetchFailed) => "Couldn't reach GitHub — this is not an empty list.",
            (Ja, PrFetchFailed) => "GitHub に接続できませんでした（PR が 0 件なのではありません）。",
            (En, PrRefresh) => "Refresh",
            (Ja, PrRefresh) => "更新",
            (En, PrRefreshing) => "Refreshing pull requests…",
            (Ja, PrRefreshing) => "pull request を更新中…",
            (Ja, OpenInExternalEditor) => "外部エディタで開く",
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
            (En, NoRepositoryOpenWelcome) => {
                "No repository open. Choose a directory to get started."
            }
            (Ja, NoRepositoryOpenWelcome) => {
                "リポジトリが開かれていません。ディレクトリを選んで始めましょう。"
            }
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
            (En, SettingsAppearance) => "Appearance",
            (Ja, SettingsAppearance) => "外観",
            (En, SettingsLanguage) => "Language",
            (Ja, SettingsLanguage) => "言語",
            (En, SettingsTheme) => "Theme",
            (Ja, SettingsTheme) => "テーマ",
            (En, SettingsThemeDesc) => "Colour theme used across the whole app.",
            (Ja, SettingsThemeDesc) => "アプリ全体で使用するカラーテーマ。",
            (En, SettingsZoom) => "UI Zoom",
            (Ja, SettingsZoom) => "UI ズーム",
            (En, SettingsZoomDesc) => "Scale all text and layout (0.7×–1.5×).",
            (Ja, SettingsZoomDesc) => "テキストとレイアウト全体を拡大縮小します(0.7×〜1.5×)。",
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
                "オン: 各 commit のノードが著者アバター（レーン色の輪付き）になり、グラフ背後に branch 色の帯が付きます。オフ: 通常の色ドットになり、アバターはメッセージ行に戻り、帯は出ません。どちらでもグラフのレーン色（線・ノード・branch/tag のラベル）は維持されます。"
            }
            (En, SettingsAutoFetch) => "Auto-fetch",
            (Ja, SettingsAutoFetch) => "自動 fetch",
            (En, SettingsAutoFetchDesc) => {
                "Periodically fetch the remote in the background so the graph and ahead/behind stay current."
            }
            (Ja, SettingsAutoFetchDesc) => {
                "バックグラウンドで定期的に remote を fetch し、graph や ahead/behind を最新に保ちます。"
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
                "Files matching these patterns are excluded from Analyze (Hotspots / Coupling / Ownership). One pattern per line, .gitignore syntax — wildcards (* ** ?) and negation (!) work. Saved to the analyze_ignore file. Reload the repository to apply."
            }
            (Ja, SettingsAnalyzeIgnoreDesc) => {
                "これらのパターンに一致するファイルは Analyze(Hotspots / Coupling / Ownership)から除外されます。1 行 1 パターン、.gitignore と同じ書式でワイルドカード(* ** ?)や否定(!)が使えます。analyze_ignore ファイルに保存されます。反映にはリポジトリの再読み込みが必要です。"
            }
            (En, SettingsAnalyzeIgnoreSave) => "Save",
            (Ja, SettingsAnalyzeIgnoreSave) => "保存",
            (En, SettingsAnalyzeIgnoreReset) => "Reset to defaults",
            (Ja, SettingsAnalyzeIgnoreReset) => "デフォルトに戻す",

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
            (En, PlanSetUpstreamFor) => "Set upstream for {}",
            (Ja, PlanSetUpstreamFor) => "{} の upstream を設定",
            (En, PlanRenameBranch) => "Rename {}",
            (Ja, PlanRenameBranch) => "{} をリネーム",
            (En, PlanMerge) => "Merge",
            (Ja, PlanMerge) => "merge",
            (En, PlanRebaseOnto) => "Rebase {}",
            (Ja, PlanRebaseOnto) => "{} へ rebase",
            (En, PlanDeleteRemoteBranch) => "Delete remote branch",
            (Ja, PlanDeleteRemoteBranch) => "remote branch を削除",
            (En, PlanDeleteRemoteBranchArmed) => "\u{26a0} Really delete — cannot be undone",
            (Ja, PlanDeleteRemoteBranchArmed) => "\u{26a0} 本当に削除しますか — 取り消せません",
            (En, PlanResetCurrent) => "Reset current branch",
            (Ja, PlanResetCurrent) => "現在の branch を reset",
            (En, PlanResetCurrentArmed) => "\u{26a0} Really reset — cannot be undone",
            (Ja, PlanResetCurrentArmed) => "\u{26a0} 本当に reset しますか — 取り消せません",
            (En, PlanForcePush) => "Force-with-lease push",
            (Ja, PlanForcePush) => "force-with-lease で push",
            (En, PlanForcePushArmed) => "\u{26a0} Really force-push — cannot be undone",
            (Ja, PlanForcePushArmed) => "\u{26a0} 本当に force push しますか — 取り消せません",
            (En, PlanCancel) => "Cancel",
            (Ja, PlanCancel) => "キャンセル",
            (En, TagPushTo) => "Push tag to {}",
            (Ja, TagPushTo) => "tag を {} に push",
            (En, TagPush) => "Push tag",
            (Ja, TagPush) => "tag を push",
            (En, TagNoRemote) => "no remote configured",
            (Ja, TagNoRemote) => "リモートが設定されていません",
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
            (Ja, CleanupEmpty) => "merge 済み・休眠 branch はありません — クリーンです。",
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
                "アプリ内ではテキスト表示です。mermaid.live を開くと図として描画されます（グラフ全体は URL に載るだけで、アップロードはされません）。"
            }
            (En, EcoResetView) => "Reset",
            (Ja, EcoResetView) => "reset",
            (En, EcoHelpTitle) => "How to read Analyze",
            (Ja, EcoHelpTitle) => "Analyze の見方",

            // ── Editor Workspace (T-WS-EDITOR-001) ──────────────────
            (En, EditorWorkspaceLoading) => "Loading…",
            (Ja, EditorWorkspaceLoading) => "読み込み中…",
            (En, EditorWorkspaceEmpty) => "No changed files in the working tree.",
            (Ja, EditorWorkspaceEmpty) => "作業ツリーに変更されたファイルはありません。",
            (En, EditorWorkspaceSelectFile) => "Select a file to view its contents.",
            (Ja, EditorWorkspaceSelectFile) => "内容を表示するファイルを選択してください。",
            (En, EditorWorkspaceBinary) => "Binary file — preview not available.",
            (Ja, EditorWorkspaceBinary) => "バイナリファイルのためプレビューできません。",
            (En, EditorWorkspaceTooLarge) => "File too large to preview (over 50,000 lines).",
            (Ja, EditorWorkspaceTooLarge) => "ファイルが大きすぎるためプレビューできません(5万行超)。",
            (En, EditorWorkspaceNoDiff) => "No changes to show for this file.",
            (Ja, EditorWorkspaceNoDiff) => "このファイルに表示する変更はありません。",
            (En, EditorWorkspaceSourceChanges) => "Changes",
            (Ja, EditorWorkspaceSourceChanges) => "変更",
            (En, EditorWorkspaceSourceAll) => "All",
            (Ja, EditorWorkspaceSourceAll) => "すべて",
            (En, EditorWorkspaceDeleted) => "File does not exist in the working tree (deleted).",
            (Ja, EditorWorkspaceDeleted) => "作業ツリーに存在しません(削除済み)。",
            (En, EditorWorkspaceUndecodable) => "Cannot decode as UTF-8 — preview not available.",
            (Ja, EditorWorkspaceUndecodable) => "UTF-8として読めないためプレビューできません。",

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
            (Ja, EditorWorkspaceUnsavedTitle) => "未保存の変更があります — 破棄しますか?",
            (En, EditorWorkspaceDiscard) => "Discard",
            (Ja, EditorWorkspaceDiscard) => "破棄",
            (En, EditorWorkspaceCancel) => "Cancel",
            (Ja, EditorWorkspaceCancel) => "キャンセル",
            (En, EditorWorkspaceBackToGraph) => "\u{2190} Graph",
            (Ja, EditorWorkspaceBackToGraph) => "\u{2190} グラフ",
            (En, EditorWorkspaceTitle) => "Editor Workspace",
            (Ja, EditorWorkspaceTitle) => "エディタワークスペース",

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
            (En, EditorSnapshotUnavailable) => "Content not available for this commit.",
            (Ja, EditorSnapshotUnavailable) => "この commit 時点のコンテンツは取得できません。",

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
            (Ja, EditorWorkspaceMermaidRendering) => "図を描画中…",
            (En, EditorWorkspaceMermaidRendering) => "Rendering diagram…",
            (Ja, EditorWorkspaceMermaidNeedsCli) => "図の描画には mermaid-cli (mmdc) が必要です",
            (En, EditorWorkspaceMermaidNeedsCli) => "Install mermaid-cli (mmdc) to render diagrams",
            (Ja, EditorWorkspaceMermaidFailed) => "図の描画に失敗しました",
            (En, EditorWorkspaceMermaidFailed) => "Diagram render failed",
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
        }
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

// ──────────────────────────────────────────────────────────────────────────
// Parameterized helpers (format! lives here, not at the call sites)
// ──────────────────────────────────────────────────────────────────────────

/// Help-overlay sections for the Analyze view: `(heading, body)` in the active
/// language. Kept here so EN/JA stay side by side (ADR-0119).
pub fn eco_help_sections() -> Vec<(&'static str, &'static str)> {
    match lang() {
        Lang::En => vec![
            (
                "What is Analyze?",
                "A read-only look at your Git history — no changes are made. Pick a time window (Day … All) top-right; every view re-ranks for that window. Excluded files are fully configurable in the \"analyze_ignore\" file (gitignore syntax, next to settings.json) — it is seeded with sensible defaults (images, PDF, CAD, KiCad, fonts, archives); edit or clear it freely.",
            ),
            (
                "Hotspots",
                "Files ranked by risk = how often a file changes (commits) × how big it is (LOC). The top of the list is where bugs are most likely — busy AND complex code. List shows the ranking with a risk bar; Map shows a treemap (tile size = LOC, colour = risk, green → red). It is a hint to look, not a verdict.",
            ),
            (
                "Coupling",
                "Pairs of files that tend to change in the same commit — hidden dependencies. 'N×' = times they changed together; '%' = how strongly they overlap. Click a row to expand one file's full set of partners (1:many). 'Graph' draws it as a network: drag to pan, scroll to zoom toward the pointer, 'Reset' to refit. 'Mermaid' shows the same graph as Mermaid source with one-click open in mermaid.live.",
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
                "Git 履歴を読み取り専用で分析します（変更は加えません）。右上で期間（Day … All）を選ぶと、各ビューがその期間で再集計されます。除外対象は settings.json の隣の \"analyze_ignore\" ファイル（gitignore 形式）で完全に設定可能で、初回に既定値（画像・PDF・CAD・KiCad・フォント・アーカイブ）が書き込まれます。自由に編集・全削除できます。",
            ),
            (
                "Hotspots（ホットスポット）",
                "リスク = 変更頻度（commit 数）× 規模（行数 LOC）でファイルを順位付け。上位ほどバグが出やすい＝「よく触られて、かつ複雑」な場所です。List はリスクバー付きランキング、Map はツリーマップ（面積=LOC、色=リスク、緑→赤）。断定ではなく「注目の目安」です。",
            ),
            (
                "Coupling（結合）",
                "同じ commit で一緒に変わりがちなファイルの組＝隠れた依存。『N×』= 一緒に変わった回数、『%』= 結びつきの強さ。行をクリックすると、そのファイルの全パートナー（1:多）が展開されます。『グラフ』はネットワーク表示：ドラッグで移動、スクロールでカーソル位置を中心に拡大、『reset』で初期表示。『Mermaid』は同じグラフを Mermaid ソースで表示し、ワンクリックで mermaid.live を開けます。",
            ),
            (
                "Ownership（所有）",
                "各ファイルの担当：主著者・その commit 占有率・関わった著者数。『1 author』（強調表示）はバス係数リスク＝そのファイルを知るのが 1 人だけ、という危険信号です。",
            ),
            (
                "診断をコピー",
                "現在表示中のビュー（Hotspots ランキング / Coupling の組 / Ownership）をコピーします。AI チャットに貼って文脈として渡せます（「ここがリスク。リファクタを手伝って」）。モードを切り替えると、そのビューが書き出されます。ボタン横で形式を選べます：どのモードでも MD（Markdown 表）か JSON、Coupling では加えて Mermaid（1:多の共変化構造が見えるフローチャート）。",
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
        Lang::Ja => format!("// WIP — {} change{}(クリックで commit panel)", n, plural),
    }
}

/// WIP row note for a *linked* worktree (not the one kagi has open). Clicking
/// the row switches the open repo to that worktree, so the note says so.
pub fn wip_row_other(n: usize) -> String {
    let plural = if n == 1 { "" } else { "s" };
    match lang() {
        Lang::En => format!(
            "// WIP — {} change{} (click to open this worktree)",
            n, plural
        ),
        Lang::Ja => format!(
            "// WIP — {} change{}(クリックで worktree を開く)",
            n, plural
        ),
    }
}

/// Commit-panel warning shown when unstaged changes exist and won't be included.
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
            "Branch name '{}' is not a valid git ref name \
             (no spaces, '..', or other invalid characters).",
            name
        ),
        Lang::Ja => format!(
            "branch 名 '{}' は有効な git ref 名ではありません(空白・'..' などは使えません)。",
            name
        ),
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

/// Smart Commit model-picker note when a model is selected but Ollama is not
/// running: "<model> — start Ollama to switch". The model name stays verbatim.
pub fn smart_model_switch_note(model: &str) -> String {
    match lang() {
        Lang::En => format!("{} — start Ollama to switch", model),
        Lang::Ja => format!("{} — 切り替えるには Ollama を起動してください", model),
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
                "Your staged diff is sent to the external `{}` CLI — it leaves kagi's local-Ollama sandbox.",
                bin
            ),
            format!(
                "It uses YOUR {} account and consumes YOUR usage quota — each generation may incur cost.",
                name
            ),
            "kagi runs the CLI non-interactively in read-only mode; it can never modify your repository."
                .to_string(),
            format!("Requires the `{}` CLI to be installed and logged in.", bin),
        ],
        Lang::Ja => [
            format!(
                "staged な diff は外部の `{}` CLI に送信され、kagi のローカル Ollama サンドボックスの外に出ます。",
                bin
            ),
            format!(
                "あなたの {} アカウントを使用し、あなたの利用量を消費します — 生成ごとに費用が発生する場合があります。",
                name
            ),
            "kagi は CLI を非対話・読み取り専用で実行します。リポジトリを変更することはありません。"
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
        "app.about" => "kagi について",
        "app.settings" => "設定…",
        "app.quit" => "kagi を終了",
        "file.newTab" => "新規タブ",
        "file.closeTab" => "タブを閉じる",
        "file.cloneRepository" => "リポジトリを clone…",
        "file.openRepository" => "リポジトリを開く…",
        "file.openInTerminal" => "リポジトリをターミナルで開く",
        "file.connectRemote" => "リモートホストに接続…",
        "file.refresh" => "リポジトリを再読み込み",
        "view.zoomIn" => "拡大",
        "view.zoomOut" => "縮小",
        "view.zoomReset" => "実際のサイズ",
        "view.fullScreen" => "フルスクリーンにする",
        "view.toggleSidebar" => "サイドバーの表示切替",
        "view.toggleTerminal" => "ターミナルの表示切替",
        "view.toggleCommitDetails" => "commit 詳細の表示切替",
        "view.toggleDiffView" => "diff 表示の切替",
        "view.toggleEditorWorkspace" => "エディタワークスペースの切替",
        "view.togglePrMode" => "Pull Request モード",
        "view.showGraph" => "グラフモード",
        "view.commandPalette" => "コマンドパレット…",
        "repo.openInFinder" => "Finder で開く",
        "branch.new" => "新規 branch…",
        "branch.checkout" => "branch を checkout…",
        "branch.rename" => "branch の名前を変更…",
        "branch.delete" => "branch を削除…",
        "commit.copyHash" => "commit ハッシュをコピー",
        "commit.checkout" => "commit を checkout",
        "commit.createBranch" => "commit から branch を作成…",
        "commit.reset" => "HEAD を commit に reset…",
        "commit.compareWorkingTree" => "作業ツリーと比較",
        "window.minimize" => "最小化",
        "window.zoom" => "ズーム",
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
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The active-language atomic is process-global; serialise the tests that
    // mutate it so they don't race.
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn slug_roundtrip() {
        assert_eq!(Lang::from_slug("en"), Some(Lang::En));
        assert_eq!(Lang::from_slug("JA"), Some(Lang::Ja));
        assert_eq!(Lang::from_slug(" ja "), Some(Lang::Ja));
        assert_eq!(Lang::from_slug("fr"), None);
        assert_eq!(Lang::En.slug(), "en");
        assert_eq!(Lang::Ja.slug(), "ja");
    }

    #[test]
    fn t_switches_with_set_lang() {
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::En);
        assert_eq!(Msg::OpInProgress.t(), "another operation is in progress");
        assert_eq!(Msg::BusyCheckout.t(), "checkout in progress…");
        set_lang_no_persist(Lang::Ja);
        assert_eq!(Msg::OpInProgress.t(), "別の操作が実行中です");
        assert_eq!(Msg::BusyCheckout.t(), "checkout 実行中…");
        set_lang_no_persist(Lang::En);
    }

    #[test]
    fn domain_words_stay_english_in_both_langs() {
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::Ja);
        // The toolbar guards keep the domain word "Pull" verbatim.
        assert!(Msg::PullDetached.t().starts_with("Pull:"));
        assert!(Msg::PushDetached.t().starts_with("Push:"));
        // ADR-0048: conflict-domain words stay English even in Japanese
        // (conflict / merge / resolved / unresolved are never translated).
        assert_eq!(Msg::EditorConflictNofM.t(), "conflict");
        assert_eq!(Msg::ConflictResolved.t(), "resolved");
        assert_eq!(Msg::ConflictResolvedShort.t(), "resolved");
        assert_eq!(Msg::ConflictUnresolved.t(), "unresolved");
        assert_eq!(Msg::ConflictConflictedCount.t(), "conflicted");
        assert_eq!(Msg::ConflictResolvedCount.t(), "resolved");
        for m in [
            Msg::ConflictSelectFile,
            Msg::ConflictCopyPath,
            Msg::ConflictSectionConflicted,
            Msg::ConflictSectionResolved,
            Msg::MergeAndResolveConflicts,
            Msg::MergeConflictWarning,
            Msg::EditorNoTextMerge,
        ] {
            assert!(!m.t().contains('衝'), "{:?} still contains 衝突", m);
            assert!(!m.t().contains("マージ"), "{:?} still contains マージ", m);
        }
        set_lang_no_persist(Lang::En);
    }

    #[test]
    fn parameterized_helpers_switch() {
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::En);
        assert_eq!(
            wip_row_note(1),
            "// WIP — 1 change (click to open commit panel)"
        );
        assert_eq!(
            wip_row_note(3),
            "// WIP — 3 changes (click to open commit panel)"
        );
        set_lang_no_persist(Lang::Ja);
        assert!(wip_row_note(2).contains("クリックで commit panel"));
        set_lang_no_persist(Lang::En);
    }

    #[test]
    fn op_failed_switches_and_keeps_domain_words() {
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::En);
        assert_eq!(op_failed(Op::Pull, "boom"), "Pull failed: boom");
        assert_eq!(op_failed(Op::RepoOpen, "boom"), "Repo open failed: boom");
        assert_eq!(op_plan_failed(Op::Push, "boom"), "Push plan failed: boom");
        set_lang_no_persist(Lang::Ja);
        assert_eq!(
            op_failed(Op::Pull, "boom"),
            "pull \u{306b}\u{5931}\u{6557}\u{3057}\u{307e}\u{3057}\u{305f}: boom"
        );
        // ADR-0048: git domain words stay English in the Japanese label too.
        for op in [
            Op::Pull,
            Op::Push,
            Op::Merge,
            Op::Commit,
            Op::Rebase,
            Op::Stash,
        ] {
            assert!(
                op.t().is_ascii(),
                "{:?} japanese label must keep the domain word in English",
                op
            );
        }
        set_lang_no_persist(Lang::En);
    }

    #[test]
    fn resolve_lang_env_override() {
        let _g = LOCK.lock().unwrap();
        // KAGI_LANG takes top priority and is deterministic for headless tests.
        std::env::set_var("KAGI_LANG", "ja");
        assert_eq!(resolve_lang(), Lang::Ja);
        std::env::set_var("KAGI_LANG", "en");
        assert_eq!(resolve_lang(), Lang::En);
        std::env::remove_var("KAGI_LANG");
    }

    // Wording lock for the GIT-LAYER test fixtures: those tests pin the exact
    // English `Display` text of these errors. It is NOT a UI guarantee - the UI
    // never calls `Display`, it goes through `branch_name_error` (covered by
    // `keyed_validation_localizes` below). Three representative variants:
    // unit, one-arg, and the multi-line one.
    #[test]
    fn keyed_validation_display_is_exact_english() {
        use kagi_domain::plan::{BranchNameError as B, WorktreePathError as W};
        assert_eq!(B::EmptyCreate.to_string(), "Branch name must not be empty.");
        assert_eq!(
            B::CreateInvalidRef("x y".into()).to_string(),
            "Branch name 'x y' is not a valid git ref name \
             (no spaces, '..', or other invalid characters)."
        );
        assert_eq!(
            W::Exists("/p".into()).to_string(),
            "Worktree path '/p' already exists."
        );
    }

    #[test]
    fn keyed_validation_localizes() {
        use kagi_domain::plan::{BranchNameError as B, WorktreePathError as W};
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::En);
        assert_eq!(
            branch_name_error(&B::EmptyCreate),
            "Branch name must not be empty."
        );
        assert_eq!(
            worktree_path_error(&W::Empty),
            "Worktree path must not be empty."
        );
        set_lang_no_persist(Lang::Ja);
        // Localized — no longer the English sentence, and the name stays verbatim.
        assert_ne!(
            branch_name_error(&B::EmptyCreate),
            "Branch name must not be empty."
        );
        assert!(branch_name_error(&B::CreateExists("feat".into())).contains("feat"));
        assert!(worktree_path_error(&W::Exists("/p".into())).contains("/p"));
        set_lang_no_persist(Lang::En);
    }
}
