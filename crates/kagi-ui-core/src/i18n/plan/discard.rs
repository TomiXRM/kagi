//! JA strings for discard plan text (ADR-0129 — first structured producer).

use kagi_domain::plan_note::{DiscardNote, PlanTitle};

/// Japanese rendering of one discard note.
pub fn note_ja(note: &DiscardNote) -> String {
    match note {
        DiscardNote::NothingSelected => {
            "破棄する対象が選択されていません。".to_string()
        }
        DiscardNote::TargetConflicted { path } => format!(
            "conflict 中です。破棄せず conflict 解決フローで処理してください。\nfile `{}`",
            path
        ),
        DiscardNote::NoUnstagedChanges { path } => {
            format!("破棄できる未 stage の変更がありません。\nfile `{}`", path)
        }
        DiscardNote::TargetSubmodule { path } => format!(
            "submodule は破棄できません。submodule 内で変更を管理してください。\nfile `{}`",
            path
        ),
        DiscardNote::UntrackedWillBeDeleted { count } => format!(
            "⚠️ 未追跡ファイル {} 件をディスクから削除します(空フォルダも削除)。削除前に blob を oplog に保存します:\n  git cat-file -p <blob-sha>",
            count
        ),
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
     実行前に現内容の blob を oplog(op=\"discard\")に記録します:\n  git cat-file -p <blob-sha>"
        .to_string()
}
