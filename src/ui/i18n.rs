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

use super::settings::{read_setting, write_setting};

// ──────────────────────────────────────────────────────────────────────────
// Lang + active-language atomic
// ──────────────────────────────────────────────────────────────────────────

/// UI language.  `En` is index 0 (the default), `Ja` is index 1.
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

    // ── Placeholder / unimplemented menu reasons ────────────────────
    CloneUnimplemented,
    RenameBranchUnimplemented,
    MultiWindowUnsupported,
    ResetUnimplemented,

    // ── Busy footers (op in flight) ─────────────────────────────────
    BusyCheckout,
    BusySwitchToLatest,
    BusyPull,
    BusyPush,
    BusyStash,
    BusyStashPop,
    BusyStashDrop,
    BusyCherryPick,
    BusyRevert,
    BusyAmend,
    BusyDeleteBranch,
    BusyDiscard,
    BusyCommit,
    BusyCreateWorktree,
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
    BcmNoUpstream,
    BcmDetachedHead,
    BcmCheckedOutElsewhere,
    BcmConflictMode,
    BcmNothingToPull,
    BcmNothingToPush,

    // ── Empty states ────────────────────────────────────────────────
    NoLocalBranches,
    NoOperationsYet,

    // ── Misc footers ────────────────────────────────────────────────
    Refreshed,
    OpenedInFinder,

    // ── W30-CONFLICT-UI: Conflict Mode (banner / list / choose / preview) ──
    // Operation headings (domain words rebase/merge/cherry-pick/revert kept
    // English per ADR-0048; only the surrounding prose is localized).
    ConflictRebasing,
    ConflictOnto,
    ConflictCommit,
    ConflictMerging,
    ConflictCherryPicking,
    ConflictReverting,
    // Banner buttons + progress.
    ConflictContinue,
    ConflictAbort,
    ConflictSkip,
    ConflictResolved,
    // File list.
    ConflictUnresolved,
    ConflictResolvedShort,
    ConflictNeedsReview,
    ConflictKindContent,
    ConflictKindRenameDelete,
    ConflictKindModifyDelete,
    ConflictKindBinary,
    // Detail pane / choose buttons (role names appended at the call site).
    ConflictSelectFile,
    ConflictKeepCurrent,
    ConflictTakeIncoming,
    ConflictKeepBoth,
    ConflictResultPreview,
    ConflictPreviewHint,
    ConflictBinaryNoPreview,
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
    /// Appearance → Auto-fetch row title.
    SettingsAutoFetch,
    /// Appearance → Auto-fetch row description.
    SettingsAutoFetchDesc,
    /// Language → Interface language row title.
    SettingsInterfaceLang,
    /// Language → Interface language row description.
    SettingsInterfaceLangDesc,

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

            // ── Placeholders ────────────────────────────────────────
            (En, CloneUnimplemented) => "clone is not implemented yet",
            (Ja, CloneUnimplemented) => "clone は未実装です",
            (En, RenameBranchUnimplemented) => "rename branch is not implemented yet",
            (Ja, RenameBranchUnimplemented) => "rename branch は未実装です",
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
            (En, BusyDiscard) => "discard in progress…",
            (Ja, BusyDiscard) => "discard 実行中…",
            (En, BusyCommit) => "commit in progress…",
            (Ja, BusyCommit) => "commit 実行中…",
            (En, BusyCreateWorktree) => "create worktree in progress…",
            (Ja, BusyCreateWorktree) => "create worktree 実行中…",
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
            (En, NoOperationsYet) => "No operations yet",
            (Ja, NoOperationsYet) => "操作履歴はまだありません",

            // ── Misc footers ────────────────────────────────────────
            (En, Refreshed) => "Refreshed",
            (Ja, Refreshed) => "更新しました",
            (En, OpenedInFinder) => "Opened in Finder",
            (Ja, OpenedInFinder) => "Finder で開きました",

            // ── W30-CONFLICT-UI: Conflict Mode ──────────────────────
            (En, ConflictRebasing) => "Rebasing",
            (Ja, ConflictRebasing) => "rebase 中:",
            (En, ConflictOnto) => "onto",
            (Ja, ConflictOnto) => "→",
            (En, ConflictCommit) => "commit",
            (Ja, ConflictCommit) => "commit",
            (En, ConflictMerging) => "Merging",
            (Ja, ConflictMerging) => "merge 中",
            (En, ConflictCherryPicking) => "Cherry-picking",
            (Ja, ConflictCherryPicking) => "cherry-pick 中",
            (En, ConflictReverting) => "Reverting",
            (Ja, ConflictReverting) => "revert 中",
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
            (En, ConflictKindBinary) => "binary",
            (Ja, ConflictKindBinary) => "binary",
            (En, ConflictSelectFile) => "Select a conflicting file to resolve it",
            (Ja, ConflictSelectFile) => "解決する conflict ファイルを選択してください",
            (En, ConflictKeepCurrent) => "Keep current",
            (Ja, ConflictKeepCurrent) => "現在の側を採用",
            (En, ConflictTakeIncoming) => "Take incoming",
            (Ja, ConflictTakeIncoming) => "取り込む側を採用",
            (En, ConflictKeepBoth) => "Keep both (current first)",
            (Ja, ConflictKeepBoth) => "両方採用(現在の側を先)",
            (En, ConflictResultPreview) => "Result preview",
            (Ja, ConflictResultPreview) => "解決結果プレビュー",
            (En, ConflictPreviewHint) => "Choose a side above to preview the resolved file.",
            (Ja, ConflictPreviewHint) => "上のボタンで側を選ぶと解決後のファイルをプレビューします。",
            (En, ConflictBinaryNoPreview) => "Binary file — choose a side; no text preview is available.",
            (Ja, ConflictBinaryNoPreview) => "binary ファイル — 側を選択してください。テキストプレビューはありません。",
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
            (Ja, EditorReset) => "すべてリセット",
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
            (Ja, EditorResetAllConfirm) => "もう一度押すと全リセット",
            (En, EditorPreviewMode) => "Preview",
            (Ja, EditorPreviewMode) => "プレビュー",
            (En, EditorEditMode) => "Edit",
            (Ja, EditorEditMode) => "編集",
            (En, EditorEditingIndicator) => "editing",
            (Ja, EditorEditingIndicator) => "編集中",
            // ── T-CONFLICT-UX-010/012: per-hunk accept controls ──
            (En, EditorHunkLabel) => "Hunk",
            (Ja, EditorHunkLabel) => "Hunk",
            (En, EditorCurrentFirst) => "Current first",
            (Ja, EditorCurrentFirst) => "現在の側を先",
            (En, EditorIncomingFirst) => "Incoming first",
            (Ja, EditorIncomingFirst) => "取り込む側を先",
            // ── W33-CONFLICT-DASHBOARD ───────────────────────────────
            (En, ConflictDashHeader) => "Merge conflicts detected",
            (Ja, ConflictDashHeader) => "conflict が検出されました",
            (En, ConflictRoleCurrent) => "Current",
            (Ja, ConflictRoleCurrent) => "現在の側",
            (En, ConflictRoleIncoming) => "Incoming",
            (Ja, ConflictRoleIncoming) => "取り込む側",
            (En, ConflictGitTermHint) => "internal git stage",
            (Ja, ConflictGitTermHint) => "内部 git ステージ",
            (En, ConflictConflictedCount) => "conflicted",
            (Ja, ConflictConflictedCount) => "conflicted",
            (En, ConflictResolvedCount) => "resolved",
            (Ja, ConflictResolvedCount) => "resolved",
            (En, ConflictSectionConflicted) => "Conflicted Files",
            (Ja, ConflictSectionConflicted) => "Conflicted ファイル",
            (En, ConflictSectionResolved) => "Resolved Files",
            (Ja, ConflictSectionResolved) => "Resolved ファイル",
            (En, ConflictConfirmAbort) => "Click again to confirm abort",
            (Ja, ConflictConfirmAbort) => "もう一度押すと中止します",
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
            (Ja, CoAuthoredBy) => "共同作成者",
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
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Parameterized helpers (format! lives here, not at the call sites)
// ──────────────────────────────────────────────────────────────────────────

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

/// Map a keyed [`kagi::git::ops::BranchNameError`] to localized text.
pub fn branch_name_error(e: &kagi::git::ops::BranchNameError) -> String {
    use kagi::git::ops::BranchNameError::*;
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

/// Map a keyed [`kagi::git::ops::WorktreePathError`] to localized text.
pub fn worktree_path_error(e: &kagi::git::ops::WorktreePathError) -> String {
    use kagi::git::ops::WorktreePathError::*;
    match e {
        Empty => Msg::WorktreePathEmpty.t().to_string(),
        Exists(path) => worktree_exists_fmt(path),
    }
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
    fn resolve_lang_env_override() {
        let _g = LOCK.lock().unwrap();
        // KAGI_LANG takes top priority and is deterministic for headless tests.
        std::env::set_var("KAGI_LANG", "ja");
        assert_eq!(resolve_lang(), Lang::Ja);
        std::env::set_var("KAGI_LANG", "en");
        assert_eq!(resolve_lang(), Lang::En);
        std::env::remove_var("KAGI_LANG");
    }

    // W29-I18N-WAVE2: the keyed git-layer validation errors must `Display` the
    // exact English wording the git-layer tests pin, and the i18n mapping must
    // switch with the active language.
    #[test]
    fn keyed_validation_display_is_exact_english() {
        use kagi::git::ops::{BranchNameError as B, WorktreePathError as W};
        assert_eq!(B::EmptyCreate.to_string(), "Branch name must not be empty.");
        assert_eq!(B::Required.to_string(), "Branch name is required.");
        assert_eq!(
            B::Whitespace.to_string(),
            "Branch name must not start or end with whitespace."
        );
        assert_eq!(B::SameName.to_string(), "Branch already has that name.");
        assert_eq!(
            B::RenameExists("x".into()).to_string(),
            "Branch 'x' already exists."
        );
        assert_eq!(
            B::RenameInvalid("x y".into()).to_string(),
            "'x y' is not a valid branch name."
        );
        assert_eq!(
            B::CreateInvalidRef("x y".into()).to_string(),
            "Branch name 'x y' is not a valid git ref name \
             (no spaces, '..', or other invalid characters)."
        );
        assert_eq!(
            B::CreateLeadingDash("-x".into()).to_string(),
            "Branch name '-x' must not start with '-'."
        );
        assert_eq!(
            B::CreateExists("main".into()).to_string(),
            "A branch named 'main' already exists in this repository."
        );
        assert_eq!(W::Empty.to_string(), "Worktree path must not be empty.");
        assert_eq!(
            W::Exists("/p".into()).to_string(),
            "Worktree path '/p' already exists."
        );
    }

    #[test]
    fn keyed_validation_localizes() {
        use kagi::git::ops::{BranchNameError as B, WorktreePathError as W};
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
