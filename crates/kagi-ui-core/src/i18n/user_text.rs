//! Presentation of Kagi-authored legacy receipt text. Never use these functions
//! to decide an operation's outcome or to serialize a receipt. Unknown text and
//! user-provided values are deliberately left intact.
use std::borrow::Cow;

use crate::i18n::{lang, Lang};

#[path = "user_text_git.rs"]
mod git;
#[path = "user_text_github.rs"]
mod github;
#[path = "user_text_note.rs"]
mod note;
#[path = "user_text_operations.rs"]
pub mod operations;
#[path = "user_text_state.rs"]
mod state;
#[path = "user_text_theme.rs"]
mod theme;
#[path = "user_text_ui.rs"]
mod ui;

pub(super) fn text(en: &'static str, ja: &'static str) -> &'static str {
    match lang() {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

pub(crate) fn plan_note_payload(
    note: &kagi_domain::plan_note::PlanNote,
) -> Cow<'_, kagi_domain::plan_note::PlanNote> {
    note::note_payload(note)
}

/// A theme filename is user data; only its structured reason is translated.
pub fn theme_load_error(error: &crate::theme_custom::ThemeLoadError) -> String {
    let name = error
        .path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| error.path.to_string_lossy());
    format!("{name}: {}", theme::theme_reason(&error.reason))
}

pub fn commit_not_loaded(sha: &str) -> String {
    match lang() {
        Lang::En => format!("Commit {sha} is not loaded yet. Load more history and try again."),
        Lang::Ja => format!(
            "commit {sha} はまだ読み込まれていません。古い履歴を読み込んでからやり直してください。"
        ),
    }
}

pub fn branch_tip_not_loaded(branch: &str) -> String {
    match lang() {
        Lang::En => format!("The latest commit on '{branch}' is not loaded yet. Load more history and try again."),
        Lang::Ja => format!("「{branch}」の最新 commit はまだ読み込まれていません。古い履歴を読み込んでからやり直してください。"),
    }
}

/// Translate known diagnostic formats, not arbitrary words in paths or Git output.
pub fn diagnostic(value: &str) -> Cow<'_, str> {
    if value == kagi_domain::advice_template_en!(ConflictsRepositoryBusy) {
        return Cow::Borrowed(crate::i18n::Msg::AdviceConflictsRepositoryBusy.t());
    }
    if value.starts_with("Draft could not be read; the damaged file was moved aside as ") {
        return Cow::Owned(crate::i18n::Msg::conflict_draft_rescued());
    }
    if let Some(result) = ui::diagnostic(value)
        .or_else(|| github::diagnostic(value))
        .or_else(|| state::state_text(value))
        .or_else(|| git::diagnostic(value))
        .or_else(|| diagnostic_frame(value))
    {
        Cow::Owned(result)
    } else {
        Cow::Borrowed(value)
    }
}

/// State descriptions are read from the unchanged durable receipt at display time.
pub fn state_text(value: &str) -> Cow<'_, str> {
    diagnostic(value)
}

/// The stable operation identifier is data, never a label. Unknown identifiers
/// remain intact so an old/new receipt does not acquire an invented meaning.
pub fn operation_label(op: &str) -> Cow<'_, str> {
    if let Some((operation, number)) = op.rsplit_once(" #") {
        if !number.is_empty()
            && number.bytes().all(|byte| byte.is_ascii_digit())
            && known_operation(operation)
        {
            return Cow::Owned(format!("{} #{number}", operation_label(operation)));
        }
    }
    let label = match op {
        "abort" => ("Abort", "abort"),
        "analyze" => ("Analyze repository", "repository を分析"),
        "amend" => ("Amend", "amend"),
        "checkout" | "checkout-commit" => ("Checkout", "checkout"),
        "checkout-tracking" | "checkout tracking" => {
            ("Checkout tracking branch", "tracking branch を checkout")
        }
        "cherry-pick" => ("Cherry-pick", "cherry-pick"),
        "branch operation" => ("Branch operation", "branch の操作"),
        "branch-cleanup" | "cleanup" => ("Delete merged branches", "merge 済み branch を削除"),
        "clone" => ("Clone", "clone"),
        "commit" => ("Commit", "commit"),
        "merge-commit" => ("Merge commit", "merge commit"),
        "create" => ("Create", "作成"),
        "create-branch" => ("Create branch", "branch を作成"),
        "create-tag" => ("Create tag", "tag を作成"),
        "create-worktree" => ("Create worktree", "worktree を作成"),
        "open-worktree" => ("Open worktree", "worktree を開く"),
        "delete" => ("Delete", "削除"),
        "delete-branch" => ("Delete branch", "branch を削除"),
        "delete-remote-branch" => ("Delete remote branch", "remote branch を削除"),
        "discard" => ("Discard changes", "変更を破棄"),
        "fetch" => ("Fetch", "fetch"),
        "fetch-remote-branch" => ("Fetch branch", "branch を fetch"),
        "fetch-pr" => ("Fetch pull request", "pull request を fetch"),
        "create-pr" => ("Create pull request", "pull request を作成"),
        "pr-merge" => ("Merge pull request", "pull request を merge"),
        "pr-comment" => ("Post comment", "コメントを投稿"),
        "pr-edit" => ("Edit pull request", "pull request を編集"),
        "pr-review" | "pr-review:approve" | "pr-review:comment" | "pr-review:request-changes" => {
            ("Submit review", "レビューを送信")
        }
        "issue-create" => ("Create issue", "Issue を作成"),
        "issue-comment" => ("Reply to issue", "Issue に返信"),
        "merge" | "merge-into" | "merge-plan" => ("Merge", "merge"),
        "pull" | "branch-pull-ff" => ("Pull", "pull"),
        "push" | "branch-push" => ("Push", "push"),
        "branch-push-set-upstream" => ("Push and set upstream", "push して upstream を設定"),
        "push-tag" => ("Push tag", "tag を push"),
        "force-with-lease-push" => ("Push with --force-with-lease", "--force-with-lease で push"),
        "rebase" => ("Rebase", "rebase"),
        "replay" | "replay-onto" => ("Replay", "replay"),
        "sync-to-remote" => ("Sync to remote", "remote に揃える"),
        "rename" | "rename-branch" => ("Rename branch", "branch 名を変更"),
        "reset" | "reset-current" => ("Reset current branch", "現在の branch を reset"),
        "revert" => ("Revert", "revert"),
        "save" | "editor-save" => ("Save file", "ファイルを保存"),
        "conflict-draft-autosave" => ("Save conflict draft", "conflict の下書きを保存"),
        "set-upstream" => ("Set upstream", "upstream を設定"),
        "write-commit-graph" => ("Optimize history", "履歴を最適化"),
        "enable-fsmonitor" => ("Speed up file checks", "ファイルの確認を高速化"),
        "apply-suggestion" => ("Apply suggestion", "提案を適用"),
        "op-revert" => ("Revert operation", "操作を取り消す"),
        "restore-to-point" => ("Restore to this point", "この時点に復元"),
        "snapshot" | "create-snapshot" => ("Save snapshot", "スナップショットを保存"),
        "restore-snapshot" => ("Restore snapshot", "スナップショットを復元"),
        "stage" | "stage-file" | "stage-hunk" => ("Stage changes", "変更を stage"),
        "stage-all" => ("Stage all files", "全ファイルを stage"),
        "unstage" | "unstage-file" | "unstage-hunk" => ("Unstage changes", "変更の stage を解除"),
        "unstage-all" => ("Unstage all files", "全ファイルの stage を解除"),
        "stash" | "stash-push" => ("Save changes to stash", "変更を stash に保存"),
        "stash-apply" => ("Apply stash", "stash を適用"),
        "stash-pop" => ("Pop stash", "stash を pop"),
        "stash-drop" | "remote-stash-drop" => ("Drop stash", "stash を削除"),
        "switch" | "switch-to-latest" => ("Switch to latest branch", "最新の branch に切り替え"),
        "lock-worktree" | "auto-lock-worktree" => ("Lock worktree", "worktree を lock"),
        "unlock-worktree" | "auto-unlock-worktree" => ("Unlock worktree", "worktree を unlock"),
        "remove-worktree" => ("Remove worktree", "worktree を削除"),
        "prune-worktrees" => ("Prune worktrees", "worktree を整理"),
        "repair-worktrees" => ("Repair worktree links", "worktree のリンクを修復"),
        "repo-open" => ("Open repository", "repository を開く"),
        "open-repository-picker" => ("Choose repository", "repository を選択"),
        "clone-folder-picker" => ("Choose clone destination", "clone 先を選択"),
        "terminal-start" => ("Start terminal", "ターミナルを起動"),
        "reconcile-release-unobservable" => ("Continue without checking", "確認せずに続行"),
        "undo" | "undo-commit" | "undo-checkout" | "undo-merge" | "undo-rebase" | "undo-reset"
        | "undo-cherry-pick" | "undo-revert" | "undo-amend" | "undo-undo-commit" => {
            ("Undo", "元に戻す")
        }
        "redo" | "redo-commit" | "redo-checkout" | "redo-merge" | "redo-rebase" | "redo-reset"
        | "redo-cherry-pick" | "redo-revert" | "redo-amend" | "redo-undo-commit" => {
            ("Redo", "やり直す")
        }
        "conflict-save" => ("Save resolution", "解決内容を保存"),
        "conflict-draft-restore" => ("Read conflict draft", "conflict の下書きを読み込み"),
        "conflict-draft-preserve" => ("Preserve conflict edits", "conflict の編集内容を保全"),
        "conflict-continue" => ("Continue conflict resolution", "conflict の解決を続行"),
        "conflict-skip" => ("Skip commit", "commit をスキップ"),
        "conflict-abort" => ("Abort conflict resolution", "conflict の解決を中止"),
        _ if op.starts_with("conflict-save:") => ("Save resolution", "解決内容を保存"),
        _ if op.starts_with("conflict-dir-file:") => (
            "Resolve file or folder conflict",
            "ファイルとフォルダーの conflict を解決",
        ),
        "merge-continue"
        | "rebase-continue"
        | "cherry-pick-continue"
        | "revert-continue"
        | "stash-continue" => {
            let operation = op.strip_suffix("-continue").unwrap();
            return Cow::Owned(match lang() {
                Lang::En => format!("Continue {operation}"),
                Lang::Ja => format!("{operation} を続行"),
            });
        }
        "merge-abort" | "rebase-abort" | "cherry-pick-abort" | "revert-abort" | "stash-abort" => {
            let operation = op.strip_suffix("-abort").unwrap();
            return Cow::Owned(match lang() {
                Lang::En => format!("Abort {operation}"),
                Lang::Ja => format!("{operation} を中止"),
            });
        }
        "rebase-skip" | "cherry-pick-skip" | "revert-skip" => {
            let operation = op.strip_suffix("-skip").unwrap();
            return Cow::Owned(match lang() {
                Lang::En => format!("Skip commit in {operation}"),
                Lang::Ja => format!("{operation} の commit をスキップ"),
            });
        }
        _ => return Cow::Borrowed(op),
    };
    Cow::Borrowed(text(label.0, label.1))
}

fn diagnostic_frame(value: &str) -> Option<String> {
    // Only recognize frames emitted by Kagi. Do not split arbitrary Git output
    // or paths at punctuation and mistake the left half for an operation.
    for separator in [" plan error: ", ": plan error: ", ": repo open error: "] {
        if let Some((op, reason)) = value.split_once(separator) {
            let label = operation_label(op);
            if known_operation(op) {
                let reason = diagnostic(reason);
                return Some(match lang() {
                    Lang::En => format!("{label}: Could not prepare: {reason}"),
                    Lang::Ja => format!("{label}: 準備できませんでした: {reason}"),
                });
            }
        }
    }
    for separator in [
        " plan failed: ",
        " の plan に失敗しました: ",
        " failed: ",
        " に失敗しました: ",
    ] {
        if let Some((op, reason)) = value.split_once(separator) {
            // Op's finite labels, including historical Preflight, delimit this
            // frame. A Git error with its own `failed:` remains raw.
            if let Some(label) = known_failure_label(op) {
                let reason = diagnostic(reason);
                return Some(if separator.contains("plan") {
                    match lang() {
                        Lang::En => format!("{label}: Could not prepare: {reason}"),
                        Lang::Ja => format!("{label}: 準備できませんでした: {reason}"),
                    }
                } else {
                    match lang() {
                        Lang::En => format!("{label}: Failed: {reason}"),
                        Lang::Ja => format!("{label}: 失敗しました: {reason}"),
                    }
                });
            }
        }
    }
    None
}

fn known_failure_label(label: &str) -> Option<&'static str> {
    use super::Op;
    const OPS: &[Op] = &[
        Op::Abort,
        Op::Amend,
        Op::Checkout,
        Op::CheckoutTracking,
        Op::CherryPick,
        Op::Cleanup,
        Op::Commit,
        Op::Continue,
        Op::Undo,
        Op::Redo,
        Op::Clone,
        Op::Create,
        Op::CreateBranch,
        Op::CreateTag,
        Op::CreateWorktree,
        Op::Delete,
        Op::Discard,
        Op::Drop,
        Op::ExternalTool,
        Op::Fetch,
        Op::GitignoreWrite,
        Op::Merge,
        Op::MergeCommit,
        Op::MoveToTrash,
        Op::OpenWorktree,
        Op::Pop,
        Op::Preflight,
        Op::PrComment,
        Op::IssueCreate,
        Op::IssueComment,
        Op::PrEdit,
        Op::PrReview,
        Op::PrPeek,
        Op::Pull,
        Op::Push,
        Op::PushTag,
        Op::Rebase,
        Op::Replay,
        Op::SyncToRemote,
        Op::Rename,
        Op::Reset,
        Op::Reveal,
        Op::Revert,
        Op::Save,
        Op::SetUpstream,
        Op::WriteCommitGraph,
        Op::EnableFsmonitor,
        Op::ApplySuggestion,
        Op::OpRevert,
        Op::RestoreToPoint,
        Op::Skip,
        Op::Snapshot,
        Op::StageAll,
        Op::Stage,
        Op::Unstage,
        Op::RecordOperation,
        Op::Stash,
        Op::StashApply,
        Op::StashPush,
        Op::SwitchToLatest,
        Op::UnlockWorktree,
        Op::RemoveWorktree,
        Op::LockWorktree,
        Op::PruneWorktrees,
        Op::RepairWorktrees,
        Op::UnstageAll,
        Op::RepoOpen,
    ];
    if matches!(label, "Preflight" | "preflight") {
        return Some(Op::Preflight.t());
    }
    OPS.iter().find_map(|op| {
        let (en, ja) = op.label();
        (label == en || label == ja).then(|| op.t())
    })
}

pub(super) fn known_operation(op: &str) -> bool {
    operation_label(op).as_ref() != op
        || matches!(
            op,
            "abort"
                | "amend"
                | "checkout"
                | "cherry-pick"
                | "clone"
                | "commit"
                | "fetch"
                | "merge"
                | "pull"
                | "push"
                | "rebase"
                | "replay"
                | "revert"
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{set_lang_no_persist, tests::LOCK};

    #[test]
    fn operation_names_and_legacy_diagnostics_are_display_only() {
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for operation in [
                "create-branch",
                "checkout-tracking",
                "conflict-save:merge",
                "conflict-draft-restore",
                "reconcile-release-unobservable",
                "auto-lock-worktree",
                "op-revert",
                "restore-to-point",
                "terminal-start",
                "pr-review:request-changes",
                "open-repository-picker",
                "clone-folder-picker",
            ] {
                assert_ne!(operation_label(operation), operation);
            }
            let unknown = "custom-operation/session=branch";
            assert!(matches!(operation_label(unknown), Cow::Borrowed(value) if value == unknown));
            let raw = "Preflight failed: repo.index() failed: Permission denied";
            let shown = diagnostic(raw);
            assert!(shown.contains("Permission denied"));
            for internal in ["Preflight", "repo.index", "plan failed"] {
                assert!(!shown.contains(internal), "{shown}");
            }
            let draft = "Draft could not be read; the damaged file was moved aside as buffer.json.corrupt.1";
            assert_eq!(
                diagnostic(draft),
                crate::i18n::Msg::conflict_draft_rescued()
            );
            let user_data = "feature_repo.index()_session=branch";
            assert!(matches!(diagnostic(user_data), Cow::Borrowed(value) if value == user_data));
        }
        set_lang_no_persist(previous);
    }
}
