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
    "未追跡ファイル {} 件をディスクから削除します（空フォルダも削除）。各ファイルを削除前に Git 参照で保持するバックアップに保存します。操作ログの記録にある参照を使い、`git cat-file blob <backup-ref>:file` で内容を取り出せます。";

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

    #[test]
    fn display_explains_manual_retained_backup_recovery() {
        assert!(ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED.contains("バックアップ"));
        assert!(ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED.contains("<backup-ref>:file"));
        assert!(!ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED.contains("PERMANENTLY"));
        assert!(!ADVICE_DISCARD_UNTRACKED_WILL_BE_DELETED.contains('⚠'));
        let recovery = recovery_ja();
        assert!(recovery.contains("Git の種類・モード"));
        assert!(recovery.contains("git ls-tree <backup-ref>"));
        assert!(recovery.contains("復旧は手動"));
        assert!(recovery.contains("自動復元するわけではありません"));
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
    "未 stage の変更を破棄します。追跡ファイルは index から復元し、未追跡ファイルはディスクから削除します。\
     各ファイルの内容と Git の種類・モードを、実行前にバックアップ参照で保持します。操作ログの記録にある正確な参照を使い、\
     `git cat-file blob <backup-ref>:file` で内容を、`git ls-tree <backup-ref>` で種類・モードを確認できます。\
     復旧は手動です。Kagi がファイルを自動復元するわけではありません。"
        .to_string()
}
