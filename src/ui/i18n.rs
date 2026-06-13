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

use super::theme::{read_setting, write_setting};

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
        if i == 1 { Lang::Ja } else { Lang::En }
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
    eprintln!("[kagi] lang: {}", l.slug());
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
    ZoomUnimplemented,
    RenameBranchUnimplemented,
    MultiWindowUnsupported,
    ResetUnimplemented,

    // ── Busy footers (op in flight) ─────────────────────────────────
    BusyCheckout,
    BusyPull,
    BusyPush,
    BusyStash,
    BusyStashPop,
    BusyCherryPick,
    BusyRevert,
    BusyAmend,
    BusyDeleteBranch,
    BusyDiscard,
    BusyCommit,
    BusyCreateWorktree,

    // ── Operation-started toasts ────────────────────────────────────
    StartedCheckout,
    StartedPull,
    StartedPush,
    StartedStash,
    StartedStashPop,
    StartedCherryPick,
    StartedRevert,
    StartedAmend,
    StartedDeleteBranch,
    StartedDiscard,
    StartedCommit,
    StartedCreateWorktree,

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
    UndoDetached,
    UndoUnborn,
    UndoAhead0,

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
    BcmNothingToPull,
    BcmNothingToPush,

    // ── Empty states ────────────────────────────────────────────────
    NoLocalBranches,
    NoOperationsYet,

    // ── Misc footers ────────────────────────────────────────────────
    Refreshed,
    OpenedInFinder,
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
            (En, ZoomUnimplemented) => "zoom is not implemented yet",
            (Ja, ZoomUnimplemented) => "zoom は未実装です",
            (En, RenameBranchUnimplemented) => "rename branch is not implemented yet",
            (Ja, RenameBranchUnimplemented) => "rename branch は未実装です",
            (En, MultiWindowUnsupported) => "multiple windows are not supported",
            (Ja, MultiWindowUnsupported) => "複数ウィンドウは未対応です",
            (En, ResetUnimplemented) => "reset is not implemented (ADR-0024)",
            (Ja, ResetUnimplemented) => "reset は未実装です (ADR-0024)",

            // ── Busy footers ────────────────────────────────────────
            (En, BusyCheckout) => "checkout in progress…",
            (Ja, BusyCheckout) => "checkout 実行中…",
            (En, BusyPull) => "pull in progress…",
            (Ja, BusyPull) => "pull 実行中…",
            (En, BusyPush) => "push in progress…",
            (Ja, BusyPush) => "push 実行中…",
            (En, BusyStash) => "stash in progress…",
            (Ja, BusyStash) => "stash 実行中…",
            (En, BusyStashPop) => "stash pop in progress…",
            (Ja, BusyStashPop) => "stash pop 実行中…",
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

            // ── Started toasts ──────────────────────────────────────
            (En, StartedCheckout) => "checkout: started",
            (Ja, StartedCheckout) => "checkout: 開始しました",
            (En, StartedPull) => "pull: started",
            (Ja, StartedPull) => "pull: 開始しました",
            (En, StartedPush) => "push: started",
            (Ja, StartedPush) => "push: 開始しました",
            (En, StartedStash) => "stash: started",
            (Ja, StartedStash) => "stash: 開始しました",
            (En, StartedStashPop) => "stash pop: started",
            (Ja, StartedStashPop) => "stash pop: 開始しました",
            (En, StartedCherryPick) => "cherry-pick: started",
            (Ja, StartedCherryPick) => "cherry-pick: 開始しました",
            (En, StartedRevert) => "revert: started",
            (Ja, StartedRevert) => "revert: 開始しました",
            (En, StartedAmend) => "amend: started",
            (Ja, StartedAmend) => "amend: 開始しました",
            (En, StartedDeleteBranch) => "delete-branch: started",
            (Ja, StartedDeleteBranch) => "delete-branch: 開始しました",
            (En, StartedDiscard) => "discard: started",
            (Ja, StartedDiscard) => "discard: 開始しました",
            (En, StartedCommit) => "commit: started",
            (Ja, StartedCommit) => "commit: 開始しました",
            (En, StartedCreateWorktree) => "create-worktree: started",
            (Ja, StartedCreateWorktree) => "create-worktree: 開始しました",

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
            (En, UndoDetached) => "Undo: detached HEAD — cannot undo",
            (Ja, UndoDetached) => "Undo: detached HEAD — undo できません",
            (En, UndoUnborn) => "Undo: no commits yet — cannot undo",
            (Ja, UndoUnborn) => "Undo: no commits yet — undo できません",
            (En, UndoAhead0) => "Undo: ahead=0 — pushed commits cannot be undone here",
            (Ja, UndoAhead0) => "Undo: ahead=0 — push 済みの commit はここでは undo できません",

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
        Lang::En => format!("// WIP — {} change{} (click to open commit panel)", n, plural),
        Lang::Ja => format!("// WIP — {} change{}(クリックで commit panel)", n, plural),
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
        set_lang_no_persist(Lang::En);
    }

    #[test]
    fn parameterized_helpers_switch() {
        let _g = LOCK.lock().unwrap();
        set_lang_no_persist(Lang::En);
        assert_eq!(wip_row_note(1), "// WIP — 1 change (click to open commit panel)");
        assert_eq!(wip_row_note(3), "// WIP — 3 changes (click to open commit panel)");
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
}
