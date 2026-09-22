//! JA strings for `CleanupNote` (ADR-0129 appendix §B-10 — merged-branch
//! cleanup, ADR-0128).

use kagi_domain::plan_note::{CleanupNote, CleanupRecovery, CleanupTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_CLEANUP_NO_LONGER_CANDIDATE: &str =
    "クリーンアップ対象ではなくなりました。一覧を更新してください。\nbranch `{}`";
pub(crate) const ADVICE_CLEANUP_NOT_SAFELY_DELETABLE: &str =
    "安全に削除できません。merge 後に commit が追加された可能性があります。一覧を更新してください。\nbranch `{}`";
pub(crate) const ADVICE_CLEANUP_TIP_MOVED: &str =
    "一覧の作成後に移動しました。一覧を更新してください。\nbranch `{}`";
pub(crate) const ADVICE_CLEANUP_SQUASH_HEURISTIC_ONLY: &str =
    "一部の branch は squash merge された可能性があるだけで、merge された証拠はありません。";
pub(crate) const ADVICE_CLEANUP_REMOTE_DELETE_NETWORK: &str =
    "remote branch も `origin` から削除されます。ネットワーク書き込みが発生します。";

/// Japanese rendering of one cleanup note.
pub fn note_ja(note: &CleanupNote) -> String {
    match note {
        CleanupNote::NoSelection => "削除する branch が選択されていません。".to_string(),
        CleanupNote::NoLongerCandidate { name } => {
            super::advice_text(Msg::AdviceCleanupNoLongerCandidate, &[name])
        }
        CleanupNote::NotSafelyDeletable { name } => {
            super::advice_text(Msg::AdviceCleanupNotSafelyDeletable, &[name])
        }
        CleanupNote::TipMoved { name } => super::advice_text(Msg::AdviceCleanupTipMoved, &[name]),
        CleanupNote::SquashHeuristicOnly => {
            super::advice_text(Msg::AdviceCleanupSquashHeuristicOnly, &[])
        }
        CleanupNote::RemoteDeleteNetwork => {
            super::advice_text(Msg::AdviceCleanupRemoteDeleteNetwork, &[])
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
        CleanupRecovery::CleanupDelete { .. } => {
            "削除した各 branch の先端 OID は oplog に記録されます。復元するには:\n  \
             git branch <name> <oid>          (ローカル)\n  \
             git push origin <oid>:refs/heads/<name>   (remote)"
                .to_string()
        }
    }
}
