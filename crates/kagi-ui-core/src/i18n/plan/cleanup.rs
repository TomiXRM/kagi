//! JA strings for `CleanupNote` (ADR-0129 appendix §B-10 — merged-branch
//! cleanup, ADR-0128).

use kagi_domain::plan_note::{CleanupNote, CleanupRecovery, CleanupTitle};

/// Japanese rendering of one cleanup note.
pub fn note_ja(note: &CleanupNote) -> String {
    match note {
        CleanupNote::NoSelection => "削除するブランチが選択されていません。".to_string(),
        CleanupNote::NoLongerCandidate { name } => format!(
            "ブランチ '{}' はクリーンアップ対象ではなくなりました。一覧を更新してください。",
            name
        ),
        CleanupNote::NotSafelyDeletable { name } => format!(
            "ブランチ '{}' は安全に削除できません(マージ後に新しいコミットが追加された可能性があります)。一覧を更新してください。",
            name
        ),
        CleanupNote::TipMoved { name } => format!(
            "ブランチ '{}' は一覧の作成後に移動しました。一覧を更新してください。",
            name
        ),
        CleanupNote::SquashHeuristicOnly => {
            "一部のブランチは squash マージされた*可能性がある*(アップストリームが消失)だけで、マージされたというローカルな証拠はありません。".to_string()
        }
        CleanupNote::RemoteDeleteNetwork => {
            "'origin' 上のリモートブランチが削除されます(ネットワーク書き込みが発生します)。".to_string()
        }
    }
}

/// Japanese rendering of one cleanup title.
pub fn title_ja(title: &CleanupTitle) -> String {
    match title {
        CleanupTitle::CleanupDelete { count } => {
            format!("マージ済みブランチを {} 件削除", count)
        }
    }
}

/// Japanese rendering of one cleanup recovery block.
pub fn recovery_ja(recovery: &CleanupRecovery) -> String {
    match recovery {
        CleanupRecovery::CleanupDelete => {
            "削除された各ブランチの先端 OID は oplog に記録されています。復元するには:\n  \
             git branch <name> <oid>          (ローカル)\n  \
             git push origin <oid>:refs/heads/<name>   (リモート)"
                .to_string()
        }
    }
}
