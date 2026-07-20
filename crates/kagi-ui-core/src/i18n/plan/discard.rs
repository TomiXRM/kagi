//! JA strings for discard plan text (ADR-0129 — first structured producer).

use kagi_domain::plan_note::{DiscardNote, PlanTitle};

/// Japanese rendering of one discard note.
pub fn note_ja(note: &DiscardNote) -> String {
    match note {
        DiscardNote::NothingSelected => {
            "破棄する対象がありません: ファイルが選択されていません。".to_string()
        }
        DiscardNote::TargetConflicted { path } => format!(
            "'{}' はコンフリクト中です。破棄せずコンフリクト解決フローで処理してください。",
            path
        ),
        DiscardNote::NoUnstagedChanges { path } => {
            format!("'{}' には破棄できる未ステージの変更がありません。", path)
        }
        DiscardNote::UntrackedWillBeDeleted { count } => format!(
            "⚠️ 未追跡ファイル {} 件がディスクから完全に削除されます(空になったフォルダも削除されます)。\
             削除前に各ファイルのバックアップ blob が oplog に保存されます — \
             `git cat-file -p <blob-sha>` で復元できます。",
            count
        ),
    }
}

/// Japanese rendering of the discard title.
pub fn title_ja(title: &PlanTitle) -> String {
    match title {
        PlanTitle::Discard {
            single: Some(path), ..
        } => format!("'{}' の変更を破棄", path),
        PlanTitle::Discard {
            single: None,
            count,
        } => {
            format!("{} ファイルの変更を破棄", count)
        }
        PlanTitle::Verbatim(s) => s.clone(),
        // Other categories never reach here — plan_title_text dispatches them
        // to their own module; this arm exists for match exhaustiveness only.
        other => other.message_en(),
    }
}

/// Japanese rendering of the discard recovery block.
pub fn recovery_ja() -> String {
    "選択したファイルの未ステージ変更を破棄します: 追跡ファイルはインデックスから復元、\
     未追跡ファイルはディスクから削除されます。いずれの場合も、実行前に各ファイルの現内容の\
     バックアップ blob が oplog(op=\"discard\")に記録されます。\
     `git cat-file -p <blob-sha>` で復元できます。"
        .to_string()
}
