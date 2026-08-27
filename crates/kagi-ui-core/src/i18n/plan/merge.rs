//! JA strings for `MergeNote` / `MergeTitle` / `MergeRecovery`
//! (ADR-0129 appendix §B-6 / §C / §D).

use kagi_domain::plan_note::{MergeNote, MergeRecovery, MergeTitle};

/// Japanese rendering of one merge note.
pub fn note_ja(note: &MergeNote) -> String {
    match note {
        MergeNote::TargetIsCurrent { target } => {
            format!("branch '{}' はすでに現在の branch です。", target)
        }
        MergeNote::TargetIsHead { target } => {
            format!("{} はすでに HEAD です。merge 対象がありません。", target)
        }
        MergeNote::AlreadyContains { current, target } => format!(
            "現在の branch '{}' はすでに '{}' を含んでいます。merge 対象がありません。",
            current, target
        ),
        MergeNote::WillConflict { count, files } => {
            let files_label = if files.is_empty() {
                "(不明なファイル)".to_string()
            } else {
                files.join(", ")
            };
            format!(
                "merge すると {} 件のコンフリクトが発生します: {}。Conflict Mode で解決してください。",
                count, files_label
            )
        }
        MergeNote::IntoCheckedOutElsewhere { target, worktree } => format!(
            "branch '{}' は worktree '{}' でチェックアウトされています。ここから merge すると、その worktree の足元で ref が動き、ファイルと index が別の commit を指した状態になります。その worktree 側で merge してください。",
            target, worktree
        ),
        MergeNote::IntoAlreadyContains { target, source } => format!(
            "branch '{}' はすでに '{}' を含んでいます。merge 対象がありません。",
            target, source
        ),
        MergeNote::IntoWouldConflict {
            target,
            source,
            count,
        } => format!(
            "'{}' を '{}' に merge すると {} 個のファイルが conflict します。conflict の解決は作業ツリー上で行うため、これは '{}' をチェックアウトしてから実行してください。",
            source, target, count, target
        ),
        MergeNote::IntoFastForward { target, source } => format!(
            "'{}' に固有の commit が無いため '{}' へ fast-forward します。ref が動くだけで、merge commit は作られません。",
            target, source
        ),
        MergeNote::IntoCreatesLocalBranch { local, remote_ref } => format!(
            "ローカルに '{}' がまだ無いため、'{}' の先頭に作成してからそこに merge します。push はしません。remote 側の '{}' は変わりません。",
            local, remote_ref, remote_ref
        ),
        MergeNote::IntoLocalDiffersFromRemote { local, remote_ref } => format!(
            "ローカルの '{}' は '{}' と同じ位置にありません。merge はローカル branch に対して行われ、remote の ref は読み書きしません。",
            local, remote_ref
        ),
        MergeNote::IntoWorkingTreeUntouched { current } => format!(
            "作業ツリーには触れません: '{}' はチェックアウトされたままで、ディスク上のファイルは1つも変わりません。",
            current
        ),
        MergeNote::NoChanges { target } => {
            format!("'{}' を merge しても変更は発生しません。", target)
        }
    }
}

/// Japanese rendering of one merge title.
pub fn title_ja(title: &MergeTitle) -> String {
    match title {
        MergeTitle::Into {
            target,
            current: Some(current),
        } => format!("{} を {} に merge", target, current),
        MergeTitle::Into {
            target,
            current: None,
        } => format!("{} を現在の branch に merge", target),
    }
}

/// Japanese rendering of one merge recovery block.
pub fn recovery_ja(recovery: &MergeRecovery) -> String {
    match recovery {
        MergeRecovery::AfterMergeIntoBranch {
            target,
            previous_sha,
        } => format!(
            "HEAD は動いていないため git reflog には出ません。'{target}' を戻すには:\n  git branch -f {target} {previous_sha}\nbranch 自身の reflog (git reflog {target}) にも移動が記録されています。"
        ),
        MergeRecovery::AfterMerge => {
            "この merge を実行後に取り消したい場合は、git reflog で以前の HEAD を確認してください。\n\
             fast-forward merge は branch を元に戻すことで取り消せます。merge commit は \
             git revert -m 1 <merge-commit> で revert できます。"
                .to_string()
        }
    }
}
