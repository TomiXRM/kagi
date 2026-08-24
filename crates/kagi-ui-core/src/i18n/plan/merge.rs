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
        MergeRecovery::AfterMerge => {
            "この merge を実行後に取り消したい場合は、git reflog で以前の HEAD を確認してください。\n\
             fast-forward merge は branch を元に戻すことで取り消せます。merge commit は \
             git revert -m 1 <merge-commit> で revert できます。"
                .to_string()
        }
    }
}
