//! JA strings for `MergeNote` / `MergeTitle` / `MergeRecovery`
//! (ADR-0129 appendix §B-6 / §C / §D).

use kagi_domain::plan_note::{MergeNote, MergeRecovery, MergeTitle};

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
        MergeNote::WillConflict { count, files } => format!(
            "merge すると {} 件 conflict します。Conflict Mode で解決してください。\nfiles {}",
            count,
            capped_files_ja(*count, files)
        ),
        MergeNote::UnrelatedHistories { target } => format!(
            "`{}` と現在の branch に共通の履歴がありません。--allow-unrelated-histories なしでは拒否されます。",
            target
        ),
        MergeNote::OperationInProgress { op } => format!(
            "{} が進行中です。merge の前に完了か中止してください。",
            op.label_ja()
        ),
        MergeNote::UntrackedWouldBeOverwritten { count, files } => format!(
            "untracked ファイル {} 件が merge で上書きされます。先に移動か削除してください。\nfiles {}",
            count,
            capped_files_ja(*count, files)
        ),
        MergeNote::IntoUnrelatedHistories { target, source } => format!(
            "`{}` と `{}` に共通の履歴がありません。--allow-unrelated-histories なしでは拒否されます。",
            source, target
        ),
        MergeNote::IntoCheckedOutElsewhere { target, worktree } => format!(
            "branch は別の worktree で checkout 中です。その worktree 側で merge してください。\nbranch `{}` / worktree `{}`",
            target, worktree
        ),
        MergeNote::IntoAlreadyContains { target, source } => format!(
            "branch `{}` はすでに `{}` を含んでいます。merge 対象がありません。",
            target, source
        ),
        MergeNote::IntoWouldConflict {
            target,
            source,
            count,
        } => format!(
            "`{}` を `{}` に merge すると {} 件 conflict します。解決は作業ツリー上で行うため、`{}` を checkout してから実行してください。",
            source, target, count, target
        ),
        MergeNote::IntoFastForward { target, source } => format!(
            "`{}` に固有の commit が無いため `{}` へ fast-forward します。ref が動くだけで merge commit は作られません。",
            target, source
        ),
        MergeNote::IntoCreatesLocalBranch { local, remote_ref } => format!(
            "ローカルに `{}` が無いため `{}` の先頭に作成して merge します。push はしません。remote 側の `{}` は変わりません。",
            local, remote_ref, remote_ref
        ),
        MergeNote::IntoLocalDiffersFromRemote { local, remote_ref } => format!(
            "ローカルの `{}` は `{}` と位置が異なります。merge はローカル branch に対して行われ、remote の ref は触れません。",
            local, remote_ref
        ),
        MergeNote::IntoWorkingTreeUntouched { current } => format!(
            "作業ツリーには触れません。`{}` は checkout されたまま、ディスク上のファイルは変わりません。",
            current
        ),
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
