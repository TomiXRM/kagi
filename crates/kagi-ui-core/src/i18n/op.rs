//! Operation-failure text (ADR-0048 addendum).
//!
//! ~120 hard-coded English strings ("Pull failed: {e}", "Repo open error: {e}",
//! …) reached the user through a modal *and* the oplog. A `Msg` variant per
//! call site would be absurd for one sentence shape, so the **operation** is
//! the key and this module owns the sentence.

use super::{lang, Lang};

#[path = "user_text.rs"]
pub mod user_text;

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
    Continue,
    Undo,
    Redo,
    Clone,
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
    PrComment,
    IssueCreate,
    IssueComment,
    PrEdit,
    PrReview,
    PrPeek,
    Pull,
    Push,
    PushTag,
    Rebase,
    Replay,
    SyncToRemote,
    Rename,
    Reset,
    Reveal,
    Revert,
    Save,
    SetUpstream,
    /// Repository-health fixes (#358).
    WriteCommitGraph,
    EnableFsmonitor,
    /// A PR review suggestion written to the working tree (#351).
    ApplySuggestion,
    /// Operation Log revert / restore (#334).
    OpRevert,
    RestoreToPoint,
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
            Abort => ("Abort", "中止"),
            Amend => ("Amend", "amend"),
            Checkout => ("Checkout", "checkout"),
            Continue => ("Continue", "続行"),
            Undo => ("Undo", "元に戻す"),
            Redo => ("Redo", "やり直す"),
            Clone => ("Clone", "clone"),
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
            Preflight => ("Pre-run check", "実行前の確認"),
            PrComment => ("PR comment", "PR コメント"),
            IssueCreate => ("Create issue", "Issue 作成"),
            IssueComment => ("Issue reply", "Issue 返信"),
            PrEdit => ("PR edit", "PR 編集"),
            PrReview => ("PR review", "PR レビュー"),
            PrPeek => ("PR peek", "PR peek"),
            Pull => ("Pull", "pull"),
            Push => ("Push", "push"),
            PushTag => ("Push tag", "tag の push"),
            Rebase => ("Rebase", "rebase"),
            Replay => ("Replay", "replay"),
            SyncToRemote => ("Sync to remote", "remote に揃える"),
            Rename => ("Rename", "名前の変更"),
            RepoOpen => ("Repo open", "リポジトリのオープン"),
            Reset => ("Reset", "reset"),
            Reveal => ("Reveal", "Reveal"),
            Revert => ("Revert", "revert"),
            Save => ("Save", "保存"),
            SetUpstream => ("Set upstream", "upstream の設定"),
            WriteCommitGraph => ("Write commit-graph", "commit-graph の書き込み"),
            EnableFsmonitor => (
                "Enable filesystem monitor",
                "ファイルシステムモニターの有効化",
            ),
            ApplySuggestion => ("Apply suggestion", "コード提案の適用"),
            OpRevert => ("Revert operation", "操作の取り消し"),
            RestoreToPoint => ("Restore to point", "時点への復元"),
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

/// Stable diagnostic text for receipt data and the stderr contract, not UI.
pub fn op_failed_contract(op: Op, err: impl std::fmt::Display) -> String {
    let label = match (op, lang()) {
        (Op::Preflight, Lang::En) => "Preflight",
        (Op::Preflight, Lang::Ja) => "preflight",
        _ => op.t(),
    };
    match lang() {
        Lang::En => format!("{label} failed: {err}"),
        Lang::Ja => format!("{label} に失敗しました: {err}"),
    }
}

/// Stable planning diagnostic for receipt data and the stderr contract.
pub fn op_plan_failed_contract(op: Op, err: impl std::fmt::Display) -> String {
    let label = match (op, lang()) {
        (Op::Preflight, Lang::En) => "Preflight",
        (Op::Preflight, Lang::Ja) => "preflight",
        _ => op.t(),
    };
    match lang() {
        Lang::En => format!("{label} plan failed: {err}"),
        Lang::Ja => format!("{label} の plan に失敗しました: {err}"),
    }
}

/// The operation's result, independent of whether its receipt was saved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingOutcome {
    Succeeded,
    Failed,
    Refused,
    Partial,
    Unknown,
}

/// Keep the actual operation result separate from the recording error.
pub fn oplog_write_failed(outcome: RecordingOutcome, err: impl std::fmt::Display) -> String {
    let raw = err.to_string();
    let err = user_text::diagnostic(&raw);
    let result = match (lang(), outcome) {
        (Lang::En, RecordingOutcome::Succeeded) => "The operation completed.",
        (Lang::En, RecordingOutcome::Failed) => "The operation failed.",
        (Lang::En, RecordingOutcome::Refused) => "The operation was not started.",
        (Lang::En, RecordingOutcome::Partial) => "The operation only partly completed.",
        (Lang::En, RecordingOutcome::Unknown) => {
            "The operation's result could not be confirmed. Check the repository before retrying."
        }
        (Lang::Ja, RecordingOutcome::Succeeded) => "操作は完了しました。",
        (Lang::Ja, RecordingOutcome::Failed) => "操作は失敗しました。",
        (Lang::Ja, RecordingOutcome::Refused) => "操作は開始されませんでした。",
        (Lang::Ja, RecordingOutcome::Partial) => "操作は一部のみ完了しました。",
        (Lang::Ja, RecordingOutcome::Unknown) => {
            "操作の結果を確認できませんでした。再試行する前に repository を確認してください。"
        }
    };
    let recovery = match (lang(), outcome) {
        (Lang::En, RecordingOutcome::Succeeded | RecordingOutcome::Partial) => {
            "\nYou cannot undo this operation or restore from its record in the Operation Log."
        }
        (Lang::Ja, RecordingOutcome::Succeeded | RecordingOutcome::Partial) => {
            "\nこの操作は操作ログからは取り消せません。この操作の記録からの復元もできません。"
        }
        _ => "",
    };
    match lang() {
        Lang::En => format!(
            "{result}\nOperation Log recording failed: {err}\n\
             This result will not be available in the Operation Log after restart.{recovery}"
        ),
        Lang::Ja => format!(
            "{result}\n操作ログへの記録に失敗しました: {err}\n\
             この結果は再起動後の操作ログには残りません。{recovery}"
        ),
    }
}

/// Display-only failure text; keep labels separate from the status sentence.
/// Known Kagi diagnostics are localized; unknown Git details remain intact.
pub fn op_failed(op: Op, err: impl std::fmt::Display) -> String {
    let raw = err.to_string();
    let err = user_text::diagnostic(&raw);
    match lang() {
        Lang::En => format!("{}: Failed: {}", op.t(), err),
        Lang::Ja => format!("{}: 失敗しました: {}", op.t(), err),
    }
}

/// #852: the terminal started without `KAGI_*` because `range` has no free
/// block of `per` ports left for `worktree`, and how to give it one.
pub fn terminal_ports_exhausted(worktree: &str, range: (u16, u16), per: u16) -> String {
    let (start, end) = range;
    match lang() {
        Lang::En => format!(
            "No free port block for {worktree} in {start}-{end} ({per} per worktree): \
             the terminal started without KAGI_PORT. Widen worktree.port_range in \
             settings.json to give it one."
        ),
        Lang::Ja => format!(
            "{start}-{end} に {worktree} 用の空き port block がありません(1 worktree あたり \
             {per}): KAGI_PORT なしで terminal を起動しました。settings.json の \
             worktree.port_range を広げると割り当てられます。"
        ),
    }
}

/// #859: `worktree_run_mode` is `nonconcurrent` and `running` (another worktree
/// of the same repository) has a live terminal shell, so none starts in
/// `worktree` until that one exits.
pub fn terminal_nonconcurrent_blocked(worktree: &str, running: &str) -> String {
    match lang() {
        Lang::En => format!(
            "Only one terminal can run at a time in this repository. \
             Exit the shell in {running} before starting one in {worktree}."
        ),
        Lang::Ja => format!(
            "このリポジトリではターミナルを同時に 1 つしか起動できません。\
             {worktree} で起動する前に {running} の shell を終了してください。"
        ),
    }
}

/// Preparation failure, expressed as the user's action rather than a phase name.
pub fn op_plan_failed(op: Op, err: impl std::fmt::Display) -> String {
    let raw = err.to_string();
    let err = user_text::diagnostic(&raw);
    match lang() {
        Lang::En => format!("{}: Could not prepare: {}", op.t(), err),
        Lang::Ja => format!("{}: 準備できませんでした: {}", op.t(), err),
    }
}

/// Footer / toast for a refusal. Keep the action label separate from the
/// status sentence, and preserve every line of the reason in the preview.
pub fn op_refused(op: &str, reason: impl std::fmt::Display, more: usize) -> String {
    let op = user_text::operation_label(op);
    let reason = reason.to_string();
    let reason: Vec<&str> = reason
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let reason = reason.join(" ");
    let reason = user_text::diagnostic(&reason);
    match (lang(), more) {
        (Lang::En, 0) => format!("{op}: Cannot proceed: {reason}"),
        (Lang::En, n) => format!("{op}: Cannot proceed: {reason} (+{n} more in Operation Log)"),
        (Lang::Ja, 0) => format!("{op}: 実行できません: {reason}"),
        (Lang::Ja, n) => format!("{op}: 実行できません: {reason}（ほか {n} 件は操作ログ）"),
    }
}

/// A fresh plan lost the shared modal slot to a newer user decision. The plan
/// is invalidated rather than queued because its repository assumptions may
/// already be stale when the foreground modal closes.
pub fn plan_not_shown_retry(op: Op) -> String {
    match lang() {
        Lang::En => format!(
            "{}: Cancelled because another dialog opened. Try again.",
            op.t()
        ),
        Lang::Ja => format!(
            "別のダイアログが開いたため {} を中止しました。もう一度 {} を実行してください。",
            op.t(),
            op.t()
        ),
    }
}

pub fn recorded_outcome_notice(message: impl std::fmt::Display) -> String {
    match lang() {
        Lang::En => format!("{message}. Review Operation Log before deciding whether to retry."),
        Lang::Ja => {
            format!("{message}。再実行するか判断する前に操作ログを確認してください。")
        }
    }
}
/// Rebase failed before starting while Kagi had neutralised dynamically named
/// repository settings.
pub fn rebase_repository_settings_may_block_start() -> &'static str {
    match lang() {
        Lang::En => {
            "Rebase did not start. Check `git status` in a terminal; if it is clean and you trust the repository, run `git rebase` there."
        }
        Lang::Ja => {
            "rebase を開始できませんでした。ターミナルで `git status` を確認し、clean でリポジトリを信頼する場合は、そこで `git rebase` を実行してください。"
        }
    }
}

/// Stable receipt/diagnostic wording; not for user-facing display.
pub fn auto_stash_identity_unverified_contract() -> &'static str {
    match lang() {
        Lang::En => {
            "Your changes are stored in the stash entry named `kagi: auto-stash before pull`, but Kagi could not verify its identity. Pull was not started; inspect the stash before continuing."
        }
        Lang::Ja => {
            "変更内容は `kagi: auto-stash before pull` という名前の stash に保存されていますが、Kagi はその識別情報を確認できませんでした。Pull は開始していません。続行前に stash を確認してください。"
        }
    }
}

pub fn auto_stash_identity_unverified() -> &'static str {
    match lang() {
        Lang::En => "Your changes are saved in the stash `kagi: auto-stash before pull`, but Kagi could not confirm it is the right one. Pull was not started; check the stash before continuing.",
        Lang::Ja => "変更は `kagi: auto-stash before pull` という stash に保存されていますが、正しい stash か確認できませんでした。Pull は開始していません。続行前に stash を確認してください。",
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

// These messages were historically written into receipts or stderr via Msg.
// Keep both locale forms stable; user_text translates them only for display.
pub fn pull_identity_changed_contract(language: Lang) -> &'static str {
    match language {
        Lang::En => "The approved Pull target changed (branch, local tip, remote or upstream). Nothing was stashed or pulled; press Pull again for a fresh confirmation.",
        Lang::Ja => "確認後に Pull の branch・先端・remote・upstream が変わりました。stash も pull も実行していません。Pull をもう一度押して最新の内容を確認してください。",
    }
}

pub fn pull_files_changed_contract(language: Lang) -> &'static str {
    match language {
        Lang::En => "The working tree changed after this confirmation was shown, so the paths Kagi would stash — and what restoring them would do — are no longer the ones you approved. Nothing was stashed or pulled; press Pull again for a fresh confirmation.",
        Lang::Ja => "確認を表示した後に working tree が変わったため、stash 対象と復元結果が承認内容と一致しません。stash も pull も実行していません。Pull をもう一度押すと最新の確認を表示します。",
    }
}

pub fn pull_restore_changed_contract(language: Lang) -> &'static str {
    match language {
        Lang::En => "The stash restore preview changed after confirmation. Nothing was stashed or pulled; press Pull again to review the new restore preview.",
        Lang::Ja => "確認後に stash の復元予測が変わりました。stash も pull も実行していません。Pull をもう一度押して最新の復元予測を確認してください。",
    }
}

pub fn remote_completion_pending_contract(language: Lang) -> &'static str {
    match language {
        Lang::En => "Waiting to confirm that the remote operation stopped. Do not retry it; inspect the remote state and completion token.",
        Lang::Ja => "remote 操作の停止確認待ちです。再実行せず、remote state と completion token を確認してください。",
    }
}

pub fn terminal_lock_changed_contract(language: Lang) -> &'static str {
    match language {
        Lang::En => "Terminal lock owner, shell, or repository changed; recover the lock manually",
        Lang::Ja => "terminal のロック所有者・shell・repository が変わりました。ロックは手動で確認して解除してください",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abort_label_localizes_the_action_not_the_git_command() {
        assert_eq!(Op::Abort.label(), ("Abort", "中止"));
    }

    #[test]
    fn display_failures_are_unchanged_by_toast_normalization() {
        let _guard = super::super::tests::LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            super::super::set_lang_no_persist(language);
            for op in [Op::Pull, Op::CreateBranch, Op::Delete, Op::Merge] {
                for shown in [
                    op_failed(op, "remote: example_reason"),
                    op_plan_failed(op, "remote: example_reason"),
                ] {
                    assert!(shown.starts_with(&format!("{}: ", op.t())));
                    assert_eq!(user_text::diagnostic(&shown), shown);
                }
            }
        }
        super::super::set_lang_no_persist(previous);
    }

    #[test]
    fn async_slot_notices_switch_language() {
        let _guard = super::super::tests::LOCK.lock();
        super::super::set_lang_no_persist(Lang::En);
        assert!(!plan_not_shown_retry(Op::Merge).contains("plan"));
        assert!(recorded_outcome_notice("remote: failure_17").contains("remote: failure_17"));
        super::super::set_lang_no_persist(Lang::Ja);
        assert!(!plan_not_shown_retry(Op::Merge).contains("plan"));
        assert!(recorded_outcome_notice("remote: failure_17").contains("remote: failure_17"));
        super::super::set_lang_no_persist(Lang::En);
    }

    #[test]
    fn refusal_names_the_reason_and_counts_the_rest() {
        let _guard = super::super::tests::LOCK.lock();
        super::super::set_lang_no_persist(Lang::En);
        let preview = op_refused("delete-branch", "checked out.\nbranch `main`\n", 0);
        assert!(!preview.contains("delete-branch"));
        assert!(
            !preview.contains('\n'),
            "the entire reason fits the first-line preview"
        );
        assert!(preview.contains("branch `main`"), "retain user data");
        assert!(op_refused("checkout", "dirty", 2).contains("2"));
        super::super::set_lang_no_persist(Lang::Ja);
        assert!(op_refused("checkout", "未 commit", 0).contains("未 commit"));
        assert!(op_refused("checkout", "未 commit", 1).contains("1"));
        super::super::set_lang_no_persist(Lang::En);
    }
}
