//! JA strings for `ConflictsNote` (ADR-0129 Phase 2 — `crates/kagi-git/src/conflicts.rs`,
//! the Conflict Editor's continue/abort/skip plans).

use kagi_domain::plan_note::{ConflictsNote, ConflictsRecovery, ConflictsTitle};

/// Japanese rendering of one conflicts note.
pub fn note_ja(note: &ConflictsNote) -> String {
    match note {
        ConflictsNote::UnresolvedFiles { files } => format!(
            "{} 件のファイルがまだ未解決です: {}。続行する前にすべてのファイルを解決してください。",
            files.len(),
            files.join(", ")
        ),
        ConflictsNote::MarkerResidue { files } => format!(
            "コンフリクトマーカーが残っています: {}。続行する前に <<<<<<< ======= >>>>>>> マーカーをすべて削除してください。",
            files.join(", ")
        ),
        ConflictsNote::IndexUnmerged { files } => format!(
            "インデックスにこのセッションが把握していない未マージのエントリがあります: {}。リポジトリを再スキャンしてください。",
            files.join(", ")
        ),
        ConflictsNote::BinaryUnresolved { files } => format!(
            "バイナリのコンフリクトにまだどちらを採用するか選択されていません: {}。",
            files.join(", ")
        ),
        ConflictsNote::DeletionUndecided { files } => format!(
            "保持するか削除するかの判断がまだ決まっていません: {}。",
            files.join(", ")
        ),
        ConflictsNote::EmptyMergeMessage => {
            "マージコミットのメッセージが空です。続行する前にコミットメッセージを入力してください。".to_string()
        }
        // Checklist prose stays untranslated (error/checklist keying is out of
        // scope for this migration — mirrors CommonNote::GitErrorPassthrough).
        ConflictsNote::ChecklistBlocker { message } => message.clone(),
        ConflictsNote::NoConflictingFilesDetected => {
            "コンフリクトファイルは検出されませんでした。続行すると操作はそのまま完了します。"
                .to_string()
        }
        ConflictsNote::PartialResolutionsPreserved => {
            "部分的な解決内容はオートセーブディレクトリに保存され、操作ログにも記録されます。\
             破棄はされません。"
                .to_string()
        }
        ConflictsNote::SkipDiscardsStep => {
            "Skip は現在のステップの変更を破棄します(コンフリクトを起こしたコミットは適用されません)。\
             部分的な解決内容はオートセーブディレクトリに保存されます。"
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
            "続行がうまくいかない場合は、操作前の状態に戻すことができます:\n  git {} --abort\n\
             操作前の HEAD は ORIG_HEAD と reflog に記録されています。",
            op
        ),
        ConflictsRecovery::Abort { op } => format!(
            "Abort は ORIG_HEAD から {} 実行前の状態を復元します。気が変わった場合も、\
             reflog にはすべての HEAD 移動が記録されています。",
            op
        ),
        ConflictsRecovery::Skip { op } => format!(
            "Skip は現在の {} ステップを破棄します。reflog にはすべての HEAD 移動が記録されており、\
             完全に中止したい場合は操作前の HEAD が ORIG_HEAD に残っています。",
            op
        ),
    }
}
