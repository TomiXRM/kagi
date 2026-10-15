//! JA strings for discard plan text (ADR-0129 — first structured producer).

use kagi_domain::plan_note::{DiscardNote, PlanTitle};

use crate::i18n::Msg;

/// JA template for `Msg::AdviceDiscardTargetConflicted`.
pub(crate) const ADVICE_DISCARD_TARGET_CONFLICTED: &str =
    "conflict 中です。破棄せず conflict 解決フローで処理してください。\nfile `{}`";

/// JA template for `Msg::AdviceDiscardTargetSubmodule`.
pub(crate) const ADVICE_DISCARD_TARGET_SUBMODULE: &str =
    "submodule は破棄できません。submodule 内で変更を管理してください。\nfile `{}`";

/// JA template for `Msg::AdviceDiscardUntrackedWillBeDeleted`.
pub(crate) const ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED: &str =
    "⚠️ untracked ファイル {} 件をディスクから削除します(空フォルダも削除)。削除前にバックアップを保存します。操作ログの詳細で blob SHA を確認し、`git cat-file -p <blob-sha>` で内容を取り出せます。";

/// Japanese rendering of one discard note.
pub fn note_ja(note: &DiscardNote) -> String {
    match note {
        DiscardNote::NothingSelected => "破棄する対象が選択されていません。".to_string(),
        DiscardNote::UnsafePath { path } => Msg::DiscardUnsafePath.t().replace("{}", path),
        DiscardNote::TargetConflicted { path } => {
            super::advice_text(Msg::AdviceDiscardTargetConflicted, &[path])
        }
        DiscardNote::NoUnstagedChanges { path } => {
            format!("破棄できる未 stage の変更がありません。\nfile `{}`", path)
        }
        DiscardNote::TargetSubmodule { path } => {
            super::advice_text(Msg::AdviceDiscardTargetSubmodule, &[path])
        }
        DiscardNote::UntrackedWillBeDeleted { count } => {
            super::advice_text(Msg::AdviceDiscardUntrackedWillBeDeleted, &[count])
        }
    }
}

/// Stable Japanese bytes for persisted notes, independent of display wording.
pub(crate) fn note_ja_contract(note: &DiscardNote) -> String {
    match note {
        DiscardNote::UntrackedWillBeDeleted { count } => format!(
            "⚠️ 未追跡ファイル {} 件をディスクから削除します(空フォルダも削除)。削除前に blob を oplog に保存します:\n  git cat-file -p <blob-sha>",
            count
        ),
        _ => note_ja(note),
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn untracked_deletion_preserves_legacy_bytes() {
        assert_eq!(
            note_ja_contract(&DiscardNote::UntrackedWillBeDeleted { count: 2 }),
            "⚠️ 未追跡ファイル 2 件をディスクから削除します(空フォルダも削除)。削除前に blob を oplog に保存します:\n  git cat-file -p <blob-sha>"
        );
    }
}

/// Japanese rendering of the discard title.
pub fn title_ja(title: &PlanTitle) -> String {
    match title {
        PlanTitle::Discard {
            single: Some(path), ..
        } => format!("`{}` の変更を破棄", path),
        PlanTitle::Discard {
            single: None,
            count,
        } => {
            format!("{} ファイルの変更を破棄", count)
        }
        // Other categories never reach here — plan_title_text dispatches them
        // to their own module; this arm exists for match exhaustiveness only.
        other => other.message_en(),
    }
}

/// Japanese rendering of the discard recovery block.
pub fn recovery_ja() -> String {
    "未 stage の変更を破棄します。追跡ファイルは index から復元、未追跡ファイルはディスクから削除。\
     実行前に各ファイルのバックアップを保存します。操作ログの詳細で blob SHA を確認し、内容を取り出してください:\n  git cat-file -p <blob-sha>"
        .to_string()
}
