//! Operation-failure text (ADR-0048 addendum).
//!
//! ~120 hard-coded English strings ("Pull failed: {e}", "Repo open error: {e}",
//! …) reached the user through a modal *and* the oplog. A `Msg` variant per
//! call site would be absurd for one sentence shape, so the **operation** is
//! the key and this module owns the sentence.

use super::{lang, Lang};

/// The operation named in a failure message shown to the user (modal + oplog).
///
/// A `Msg` variant per call site would mean ~120 arms for one sentence shape,
/// so the *operation* is the key instead and [`op_failed`] / [`op_plan_failed`]
/// own the sentence. Exhaustiveness is still compiler-checked, so a new
/// operation cannot ship without a Japanese label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Abort,
    Amend,
    Checkout,
    CheckoutTracking,
    CherryPick,
    Cleanup,
    Commit,
    Create,
    CreateBranch,
    CreateTag,
    CreateWorktree,
    Delete,
    Discard,
    Drop,
    ExternalTool,
    Fetch,
    GitignoreWrite,
    Merge,
    MergeCommit,
    MoveToTrash,
    OpenWorktree,
    Pop,
    Preflight,
    PrPeek,
    Pull,
    Push,
    PushTag,
    Rebase,
    Rename,
    Reset,
    Reveal,
    Revert,
    Save,
    SetUpstream,
    Skip,
    Snapshot,
    StageAll,
    Stash,
    StashApply,
    StashPush,
    SwitchToLatest,
    UnlockWorktree,
    RemoveWorktree,
    LockWorktree,
    PruneWorktrees,
    RepairWorktrees,
    UnstageAll,
    RepoOpen,
}

impl Op {
    /// `(English, Japanese)` label. Git domain words (pull / push / merge /
    /// commit / branch / stash / rebase / checkout / tag / worktree …) stay
    /// English in the Japanese label per ADR-0048 §Domain words.
    fn label(self) -> (&'static str, &'static str) {
        use Op::*;
        match self {
            Abort => ("Abort", "abort"),
            Amend => ("Amend", "amend"),
            Checkout => ("Checkout", "checkout"),
            CheckoutTracking => ("Checkout tracking", "tracking branch の checkout"),
            CherryPick => ("Cherry-pick", "cherry-pick"),
            Cleanup => ("Cleanup", "cleanup"),
            Commit => ("Commit", "commit"),
            Create => ("Create", "作成"),
            CreateBranch => ("Create branch", "branch の作成"),
            CreateTag => ("Create tag", "tag の作成"),
            CreateWorktree => ("Create worktree", "worktree の作成"),
            Delete => ("Delete", "削除"),
            Discard => ("Discard", "discard"),
            Drop => ("Drop", "drop"),
            ExternalTool => ("External tool", "外部ツール"),
            Fetch => ("Fetch", "fetch"),
            GitignoreWrite => (".gitignore write", ".gitignore の書き込み"),
            Merge => ("Merge", "merge"),
            MergeCommit => ("Merge-commit", "merge commit"),
            MoveToTrash => ("Move to Trash", "ゴミ箱への移動"),
            OpenWorktree => ("Open worktree", "worktree を開く操作"),
            Pop => ("Pop", "pop"),
            Preflight => ("Preflight", "preflight"),
            PrPeek => ("PR peek", "PR peek"),
            Pull => ("Pull", "pull"),
            Push => ("Push", "push"),
            PushTag => ("Push tag", "tag の push"),
            Rebase => ("Rebase", "rebase"),
            Rename => ("Rename", "名前の変更"),
            RepoOpen => ("Repo open", "リポジトリのオープン"),
            Reset => ("Reset", "reset"),
            Reveal => ("Reveal", "Reveal"),
            Revert => ("Revert", "revert"),
            Save => ("Save", "保存"),
            SetUpstream => ("Set upstream", "upstream の設定"),
            Skip => ("Skip", "skip"),
            Snapshot => ("Snapshot", "スナップショットの取得"),
            StageAll => ("Stage all", "全ファイルの stage"),
            Stash => ("Stash", "stash"),
            StashApply => ("Stash apply", "stash apply"),
            StashPush => ("Stash push", "stash push"),
            SwitchToLatest => ("Switch to latest", "最新への切り替え"),
            UnlockWorktree => ("Unlock worktree", "worktree の unlock"),
            RemoveWorktree => ("Remove worktree", "worktree の削除"),
            LockWorktree => ("Lock worktree", "worktree の lock"),
            PruneWorktrees => ("Prune worktrees", "worktree の prune"),
            RepairWorktrees => ("Repair worktrees", "worktree の repair"),
            UnstageAll => ("Unstage all", "全ファイルの unstage"),
        }
    }

    /// The localized operation label on its own.
    pub fn t(self) -> &'static str {
        let (en, ja) = self.label();
        match lang() {
            Lang::En => en,
            Lang::Ja => ja,
        }
    }
}

/// `"Pull failed: <err>"` / `"pull に失敗しました: <err>"`.
///
/// The single sentence every operation-failure string in the UI goes through.
/// `err` is the raw backend error and is never translated.
pub fn op_failed(op: Op, err: impl std::fmt::Display) -> String {
    match lang() {
        Lang::En => format!("{} failed: {}", op.t(), err),
        Lang::Ja => format!("{} に失敗しました: {}", op.t(), err),
    }
}

/// `"Pull plan failed: <err>"` / `"pull の plan に失敗しました: <err>"` — the
/// planning step of [`op_failed`]'s operation (plan → confirm → preflight → …).
pub fn op_plan_failed(op: Op, err: impl std::fmt::Display) -> String {
    match lang() {
        Lang::En => format!("{} plan failed: {}", op.t(), err),
        Lang::Ja => format!("{} の plan に失敗しました: {}", op.t(), err),
    }
}
