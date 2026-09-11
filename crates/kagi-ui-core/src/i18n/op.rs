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
    Stage,
    Unstage,
    RecordOperation,
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
            Stage => ("Stage", "stage"),
            Unstage => ("Unstage", "unstage"),
            RecordOperation => ("Record operation", "操作ログの記録"),
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

/// The operation finished, but Kagi could not persist its oplog record.
///
/// Kagi's promise is that every operation leaves a record to recover and audit
/// from. Swallowing this failure leaves the user trusting a log that is missing
/// the entry — and the in-session panel still shows it, so nothing looks wrong
/// until the next launch (#643 A1).
pub fn oplog_write_failed(err: impl std::fmt::Display) -> String {
    match lang() {
        Lang::En => format!(
            "The operation finished, but its oplog record could not be written ({err}). \
             Recovery and audit history for it is missing."
        ),
        Lang::Ja => format!(
            "操作は完了しましたが、oplog への記録に失敗しました({err})。この操作の復旧・監査履歴が残っていません。"
        ),
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
/// Rebase failed before starting while Kagi had neutralised dynamically named
/// repository settings.
pub fn rebase_repository_settings_may_block_start() -> &'static str {
    match lang() {
        Lang::En => {
            "Rebase did not start. Run `git status` in a terminal. If it is clean, Kagi disabling some repository settings for safety may be involved. If you trust the repository, run `git rebase` in a terminal."
        }
        Lang::Ja => {
            "rebase を開始できませんでした。ターミナルで `git status` を確認してください。clean なら、Kagi が安全のため一部の repository 設定を無効化したことが影響している可能性があります。repository を信頼する場合は、ターミナルで `git rebase` を実行してください。"
        }
    }
}

/// #625: the working tree moved between the Stash & Pull confirmation and the
/// stash, so what the user approved is no longer what would happen. Nothing
/// was stashed and nothing was pulled.
pub fn auto_stash_plan_stale() -> &'static str {
    match lang() {
        Lang::En => {
            "The working tree changed after this confirmation was shown, so the paths Kagi would stash — and what restoring them would do — are no longer the ones you approved. Nothing was stashed or pulled; press Pull again for a fresh confirmation."
        }
        Lang::Ja => {
            "確認を表示した後に作業ツリーが変わったため、stash 対象と復元結果が承認内容と一致しません。stash も pull も実行していません。Pull をもう一度押すと最新の確認を表示します。"
        }
    }
}

pub fn auto_stash_identity_unverified() -> &'static str {
    match lang() {
        Lang::En => {
            "Your changes are stored in the stash entry named `kagi: auto-stash before pull`, but Kagi could not verify its identity. Pull was not started; inspect the stash before continuing."
        }
        Lang::Ja => {
            "変更内容は `kagi: auto-stash before pull` という名前の stash に保存されていますが、Kagi はその識別情報を確認できませんでした。Pull は開始していません。続行前に stash を確認してください。"
        }
    }
}

pub fn pull_failed_stash_restored(pull_error: &str) -> String {
    match lang() {
        Lang::En => format!("{pull_error}. Auto-stashed changes were restored."),
        Lang::Ja => format!("{pull_error}。auto-stash の変更は復元しました。"),
    }
}

pub fn auto_stash_restore_conflicted(pull_error: Option<&str>, files: &str) -> String {
    match (lang(), pull_error) {
        (Lang::En, None) => format!(
            "Pull completed, but auto-stash restoration conflicted in: {files}. The stash was kept."
        ),
        (Lang::Ja, None) => format!(
            "Pull は完了しましたが、auto-stash の復元が次のファイルで conflict しました: {files}。stash は保持されています。"
        ),
        (Lang::En, Some(error)) => format!(
            "{error}. Restoring the auto-stash conflicted in: {files}. The stash was kept."
        ),
        (Lang::Ja, Some(error)) => format!(
            "{error}。auto-stash の復元が次のファイルで conflict しました: {files}。stash は保持されています。"
        ),
    }
}

pub fn auto_stash_restore_failed(pull_error: Option<&str>, restore_error: &str) -> String {
    match (lang(), pull_error) {
        (Lang::En, None) => format!(
            "Pull completed, but auto-stash restoration failed: {restore_error}. Inspect the stash before continuing."
        ),
        (Lang::Ja, None) => format!(
            "Pull は完了しましたが、auto-stash の復元に失敗しました: {restore_error}。続行前に stash を確認してください。"
        ),
        (Lang::En, Some(error)) => format!(
            "{error}. Auto-stash restoration also failed: {restore_error}. Inspect the stash before continuing."
        ),
        (Lang::Ja, Some(error)) => format!(
            "{error}。auto-stash の復元にも失敗しました: {restore_error}。続行前に stash を確認してください。"
        ),
    }
}

pub fn auto_stash_missing() -> &'static str {
    match lang() {
        Lang::En => "The auto-stash moved or disappeared before restoration.",
        Lang::Ja => "復元前に auto-stash が移動または消失しました。",
    }
}
