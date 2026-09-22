//! JA strings for `MergeNote` / `MergeTitle` / `MergeRecovery`
//! (ADR-0129 appendix §B-6 / §C / §D).

use kagi_domain::plan_note::{MergeNote, MergeRecovery, MergeTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_MERGE_WILL_CONFLICT: &str =
    "merge すると {} 件 conflict します。Conflict Mode で解決してください。\nfiles {}";
pub(crate) const ADVICE_MERGE_UNRELATED_HISTORIES: &str =
    "`{}` と現在の branch に共通の履歴がありません。--allow-unrelated-histories なしでは拒否されます。";
pub(crate) const ADVICE_MERGE_OPERATION_IN_PROGRESS: &str =
    "{} が進行中です。merge の前に完了か中止してください。";
pub(crate) const ADVICE_MERGE_UNTRACKED_WOULD_BE_OVERWRITTEN: &str =
    "untracked ファイル {} 件が merge で上書きされます。先に移動か削除してください。\nfiles {}";
pub(crate) const ADVICE_MERGE_INTO_UNRELATED_HISTORIES: &str =
    "`{}` と `{}` に共通の履歴がありません。--allow-unrelated-histories なしでは拒否されます。";
pub(crate) const ADVICE_MERGE_INTO_CHECKED_OUT_ELSEWHERE: &str =
    "branch は別の worktree で checkout 中です。その worktree 側で merge してください。\nbranch `{}` / worktree `{}`";
pub(crate) const ADVICE_MERGE_INTO_WOULD_CONFLICT: &str =
    "`{}` を `{}` に merge すると {} 件 conflict します。解決は作業ツリー上で行うため、`{}` を checkout してから実行してください。";
pub(crate) const ADVICE_MERGE_INTO_FAST_FORWARD: &str =
    "`{}` に固有の commit が無いため `{}` へ fast-forward します。ref が動くだけで merge commit は作られません。";
pub(crate) const ADVICE_MERGE_INTO_WORKING_TREE_UNTOUCHED: &str =
    "作業ツリーには触れません。`{}` は checkout されたまま、ディスク上のファイルは変わりません。";
pub(crate) const ADVICE_MERGE_INTO_CREATES_LOCAL_BRANCH: &str =
    "ローカルに `{}` が無いため `{}` の先頭に作成して merge します。push はしません。remote 側の `{}` は変わりません。";
pub(crate) const ADVICE_MERGE_INTO_REMOTE_SOURCE: &str =
    "remote-tracking ref `{}` (tip: {}) は最後の fetch 時点の内容で、古い可能性があります。fetch は行わず、source のローカル branch も作成しません。";
pub(crate) const ADVICE_MERGE_INTO_LOCAL_DIFFERS_FROM_REMOTE: &str =
    "ローカルの `{}` は `{}` と位置が異なります。merge はローカル branch に対して行われ、remote の ref は触れません。";

/// Capped file list (issue #301): shown names joined by `", "`, with
/// "他 N 件" appended when the true `count` exceeds what is shown. Empty list
/// renders "(不明なファイル)".
fn capped_files_ja(count: usize, files: &[String]) -> String {
    if files.is_empty() {
        return "(不明なファイル)".to_string();
    }
    let shown = files.join(", ");
    let more = count.saturating_sub(files.len());
    if more > 0 {
        format!("{}、他 {} 件", shown, more)
    } else {
        shown
    }
}

/// Japanese rendering of one merge note.
pub fn note_ja(note: &MergeNote) -> String {
    match note {
        MergeNote::TargetIsCurrent { target } => {
            format!("branch `{}` はすでに現在の branch です。", target)
        }
        MergeNote::TargetIsHead { target } => {
            format!("`{}` はすでに HEAD です。merge 対象がありません。", target)
        }
        MergeNote::AlreadyContains { current, target } => format!(
            "現在の branch `{}` はすでに `{}` を含んでいます。merge 対象がありません。",
            current, target
        ),
        MergeNote::WillConflict { count, files } => {
            let files_label = capped_files_ja(*count, files);
            super::advice_text(Msg::AdviceMergeWillConflict, &[count, &files_label])
        }
        MergeNote::UnrelatedHistories { target } => {
            super::advice_text(Msg::AdviceMergeUnrelatedHistories, &[target])
        }
        MergeNote::OperationInProgress { op } => {
            super::advice_text(Msg::AdviceMergeOperationInProgress, &[&op.label_ja()])
        }
        MergeNote::UntrackedWouldBeOverwritten { count, files } => {
            let files_label = capped_files_ja(*count, files);
            super::advice_text(
                Msg::AdviceMergeUntrackedWouldBeOverwritten,
                &[count, &files_label],
            )
        }
        MergeNote::IntoUnrelatedHistories { target, source } => {
            super::advice_text(Msg::AdviceMergeIntoUnrelatedHistories, &[source, target])
        }
        MergeNote::IntoCheckedOutElsewhere { target, worktree } => {
            super::advice_text(Msg::AdviceMergeIntoCheckedOutElsewhere, &[target, worktree])
        }
        MergeNote::IntoAlreadyContains { target, source } => format!(
            "branch `{}` はすでに `{}` を含んでいます。merge 対象がありません。",
            target, source
        ),
        MergeNote::IntoWouldConflict {
            target,
            source,
            count,
        } => super::advice_text(
            Msg::AdviceMergeIntoWouldConflict,
            &[source, target, count, target],
        ),
        MergeNote::IntoFastForward { target, source } => {
            super::advice_text(Msg::AdviceMergeIntoFastForward, &[target, source])
        }
        MergeNote::IntoRemoteSource { reference, tip } => {
            super::advice_text(Msg::AdviceMergeIntoRemoteSource, &[reference, tip])
        }
        MergeNote::IntoCreatesLocalBranch { local, remote_ref } => super::advice_text(
            Msg::AdviceMergeIntoCreatesLocalBranch,
            &[local, remote_ref, remote_ref],
        ),
        MergeNote::IntoLocalDiffersFromRemote { local, remote_ref } => super::advice_text(
            Msg::AdviceMergeIntoLocalDiffersFromRemote,
            &[local, remote_ref],
        ),
        MergeNote::IntoWorkingTreeUntouched { current } => {
            super::advice_text(Msg::AdviceMergeIntoWorkingTreeUntouched, &[current])
        }
        MergeNote::NoChanges { target } => {
            format!("`{}` を merge しても変更はありません。", target)
        }
    }
}

/// Japanese rendering of one merge title.
pub fn title_ja(title: &MergeTitle) -> String {
    match title {
        MergeTitle::Into {
            target,
            current: Some(current),
        } => format!("`{}` を `{}` に merge", target, current),
        MergeTitle::Into {
            target,
            current: None,
        } => format!("`{}` を現在の branch に merge", target),
    }
}

/// Japanese rendering of one merge recovery block.
pub fn recovery_ja(recovery: &MergeRecovery) -> String {
    match recovery {
        MergeRecovery::AfterMergeIntoBranch {
            target,
            previous_sha,
        } => format!(
            "HEAD は動かないため git reflog には出ません。`{target}` を戻すには:\n  git branch -f {target} {previous_sha}\nbranch の reflog(git reflog {target})にも移動が記録されます。"
        ),
        MergeRecovery::AfterMerge => {
            "実行後に取り消すには git reflog で以前の HEAD を確認してください。\n\
             fast-forward merge は branch を戻せば取り消せます。merge commit は:\n  git revert -m 1 <merge-commit>"
                .to_string()
        }
    }
}
