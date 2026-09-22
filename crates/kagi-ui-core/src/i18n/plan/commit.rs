//! JA strings for `CommitNote` / `CommitTitle` / `CommitRecovery`
//! (ADR-0129 Phase 2 — `staging.rs::plan_commit`, discovered "ops 外").

use kagi_domain::plan_note::{CommitNote, CommitRecovery, CommitTitle};

use crate::i18n::Msg;

/// JA text for `Msg::AdviceCommitNothingStaged`.
pub(crate) const ADVICE_COMMIT_NOTHING_STAGED: &str =
    "stage されたファイルがありません。先に変更を stage してください。";

/// JA template for `Msg::AdviceCommitConflictedFiles`.
pub(crate) const ADVICE_COMMIT_CONFLICTED_FILES: &str =
    "conflict ファイルが {} 件あります。すべて解決してから commit してください。";

/// JA template for `Msg::AdviceCommitLeftoverNotIncluded`.
pub(crate) const ADVICE_COMMIT_LEFTOVER_NOT_INCLUDED: &str =
    "この commit に含まれないファイルが {} 件あります({})。";

/// Japanese rendering of one commit note.
pub fn note_ja(note: &CommitNote) -> String {
    match note {
        CommitNote::EmptyMessage => "commit メッセージを空にはできません。".to_string(),
        CommitNote::NothingStaged => super::advice_text(Msg::AdviceCommitNothingStaged, &[]),
        CommitNote::ConflictedFiles { count } => {
            super::advice_text(Msg::AdviceCommitConflictedFiles, &[count])
        }
        CommitNote::LeftoverNotIncluded { count, parts } => {
            let mut ja_parts: Vec<String> = Vec::new();
            if parts.modified > 0 {
                ja_parts.push(format!("変更 {} 件", parts.modified));
            }
            if parts.untracked > 0 {
                ja_parts.push(format!("未追跡 {} 件", parts.untracked));
            }
            let parts = ja_parts.join(", ");
            super::advice_text(Msg::AdviceCommitLeftoverNotIncluded, &[count, &parts])
        }
    }
}

/// Japanese rendering of one commit title.
pub fn title_ja(title: &CommitTitle) -> String {
    match title {
        CommitTitle::Commit { summary } => format!("commit: \"{}\"", summary),
        CommitTitle::FinalizeMergeCommit => "merge commit を確定".to_string(),
    }
}

/// Japanese rendering of one commit recovery block.
pub fn recovery_ja(recovery: &CommitRecovery) -> String {
    match recovery {
        CommitRecovery::AfterCommit { staged_files } => format!(
            "直後にメッセージを修正するには:\n  git commit --amend\n\
             変更を stage したまま commit を取り消すには:\n  git revert HEAD\n\
             (stage されたファイル: {})",
            staged_files.join(", ")
        ),
    }
}
