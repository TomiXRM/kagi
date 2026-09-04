//! JA strings for `ConflictsNote` (ADR-0129 Phase 2 — the git backend's
//! conflict-session module, the Conflict Editor's continue/abort/skip plans).

use kagi_domain::plan_note::{ConflictsNote, ConflictsRecovery, ConflictsTitle};

/// Japanese rendering of one conflicts note.
pub fn note_ja(note: &ConflictsNote) -> String {
    match note {
        ConflictsNote::UnresolvedFiles { files } => format!(
            "{} 件が未解決です。続行前にすべて解決してください。\nfiles {}",
            files.len(),
            files.join(", ")
        ),
        ConflictsNote::MarkerResidue { files } => format!(
            "conflict marker が残っています。続行前にすべて削除してください。\nfiles {}",
            files.join(", ")
        ),
        ConflictsNote::IndexUnmerged { files } => format!(
            "index にこのセッションが把握していない未 merge エントリがあります。リポジトリを再スキャンしてください。\nfiles {}",
            files.join(", ")
        ),
        ConflictsNote::BinaryUnresolved { files } => format!(
            "バイナリ conflict の採用側が未選択です。\nfiles {}",
            files.join(", ")
        ),
        ConflictsNote::DeletionUndecided { files } => format!(
            "保持か削除かが未決定です。\nfiles {}",
            files.join(", ")
        ),
        ConflictsNote::EmptyMergeMessage => {
            "merge commit のメッセージが空です。入力してから続行してください。".to_string()
        }
        // Checklist prose stays untranslated (error/checklist keying is out of
        // scope for this migration — mirrors CommonNote::GitErrorPassthrough).
        ConflictsNote::ChecklistBlocker { message } => message.clone(),
        ConflictsNote::NoConflictingFilesDetected => {
            "conflict ファイルはありません。続行すると操作はそのまま完了します。".to_string()
        }
        ConflictsNote::PartialResolutionsPreserved => {
            "部分的な解決内容は autosave と oplog に保存されます。破棄されません。".to_string()
        }
        ConflictsNote::SkipDiscardsStep => {
            "Skip は現在ステップの変更を破棄します。conflict を起こした commit は適用されません。\
             部分的な解決内容は autosave に保存されます。"
                .to_string()
        }
    }
}

/// Japanese rendering of one conflicts title.
pub fn title_ja(title: &ConflictsTitle) -> String {
    match title {
        ConflictsTitle::Continue { op } => format!("{} を続行", op),
        ConflictsTitle::Abort { op } => format!("{} を中止", op),
        ConflictsTitle::Skip { op } => format!("{} のステップをスキップ", op),
    }
}

/// Japanese rendering of one conflicts recovery block.
pub fn recovery_ja(recovery: &ConflictsRecovery) -> String {
    match recovery {
        ConflictsRecovery::Continue { op } => format!(
            "うまくいかない場合は操作前の状態に戻せます:\n  git {} --abort\n\
             操作前の HEAD は ORIG_HEAD と reflog に残ります。",
            op
        ),
        ConflictsRecovery::Abort { op } => format!(
            "Abort は ORIG_HEAD から {} 実行前の状態を復元します。HEAD 移動はすべて reflog に残ります。",
            op
        ),
        ConflictsRecovery::Skip { op } => format!(
            "Skip は現在の {} ステップを破棄します。HEAD 移動は reflog に残り、完全に中止する場合は操作前の HEAD が ORIG_HEAD に残ります。",
            op
        ),
    }
}
