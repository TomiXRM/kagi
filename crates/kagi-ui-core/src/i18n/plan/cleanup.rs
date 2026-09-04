//! JA strings for `CleanupNote` (ADR-0129 appendix §B-10 — merged-branch
//! cleanup, ADR-0128).

use kagi_domain::plan_note::{CleanupNote, CleanupRecovery, CleanupTitle};

/// Japanese rendering of one cleanup note.
pub fn note_ja(note: &CleanupNote) -> String {
    match note {
        CleanupNote::NoSelection => "削除する branch が選択されていません。".to_string(),
        CleanupNote::NoLongerCandidate { name } => format!(
            "クリーンアップ対象ではなくなりました。一覧を更新してください。\nbranch `{}`",
            name
        ),
        CleanupNote::NotSafelyDeletable { name } => format!(
            "安全に削除できません。merge 後に commit が追加された可能性があります。一覧を更新してください。\nbranch `{}`",
            name
        ),
        CleanupNote::TipMoved { name } => format!(
            "一覧の作成後に移動しました。一覧を更新してください。\nbranch `{}`",
            name
        ),
        CleanupNote::SquashHeuristicOnly => {
            "一部の branch は squash merge された可能性があるだけで、merge された証拠はありません。".to_string()
        }
        CleanupNote::RemoteDeleteNetwork => {
            "remote branch も `origin` から削除されます。ネットワーク書き込みが発生します。".to_string()
        }
    }
}

/// Japanese rendering of one cleanup title.
pub fn title_ja(title: &CleanupTitle) -> String {
    match title {
        CleanupTitle::CleanupDelete { count } => {
            format!("merge 済み branch を {} 件削除", count)
        }
    }
}

/// Japanese rendering of one cleanup recovery block.
pub fn recovery_ja(recovery: &CleanupRecovery) -> String {
    match recovery {
        CleanupRecovery::CleanupDelete => {
            "削除した各 branch の先端 OID は oplog に記録されます。復元するには:\n  \
             git branch <name> <oid>          (ローカル)\n  \
             git push origin <oid>:refs/heads/<name>   (リモート)"
                .to_string()
        }
    }
}
