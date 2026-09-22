//! JA strings for `HistoryNote` (ADR-0129 appendix §B-1 — undo / amend /
//! undo·redo history-move).

use kagi_domain::plan::AmendMode;
use kagi_domain::plan_note::{
    HistoryMoveDir, HistoryNote, HistoryOp, HistoryRecovery, HistoryTitle,
};

use crate::i18n::Msg;

pub(crate) const ADVICE_HISTORY_PUSHED_HISTORY_REWRITE_UNDO: &str =
    "push 済みの commit です。公開履歴を書き換えるため undo はできません。`git revert` で打ち消し commit を作成してください。\ncommit `{}`";
pub(crate) const ADVICE_HISTORY_PUSHED_HISTORY_REWRITE_AMEND: &str =
    "push 済みで、他の人が土台にしている branch です。履歴を書き換えると fetch 済みの clone が取り残されます。amend は拒否されます。修正は新しい commit で行ってください。\ncommit `{}`";
pub(crate) const ADVICE_HISTORY_AMEND_DIVERGES_FROM_REMOTE: &str =
    "remote にある commit です。amend は新しい commit で置き換えるため `{}` は upstream から分岐し、通常の push は拒否されます。branch メニューの Force-with-lease push を使ってください。\ncommit `{}`";
pub(crate) const ADVICE_HISTORY_NOTHING_STAGED_FOR_AMEND: &str =
    "stage 済みの変更がありません。先に stage するか、メッセージのみの amend を使ってください。";
pub(crate) const ADVICE_HISTORY_WRONG_BRANCH: &str =
    "この操作は branch `{}` 上のものです。現在の branch は `{}`。{} するには `{}` に切り替えてください。";
pub(crate) const ADVICE_HISTORY_HEAD_NOT_ON_BRANCH: &str =
    "HEAD が branch を指していません。{} には対象 branch を checkout している必要があります。";
pub(crate) const ADVICE_HISTORY_ENTRY_STALE_BRANCH_MOVED: &str =
    "branch `{}` は操作後に移動しています(現在 {}、想定 {})。古い履歴エントリのためスキップします。";
pub(crate) const ADVICE_HISTORY_ENTRY_STALE_UNREACHABLE: &str =
    "対象 commit に到達できません。古い履歴エントリのためスキップします。\ncommit `{}`";
pub(crate) const ADVICE_HISTORY_SOFT_MOVE_PRESERVES_CHANGES: &str =
    "未 commit の変更はそのまま保持されます。動かすのは branch の参照のみです(soft reset、index と作業ツリーは変更なし)。";

/// JA rendering of the undo/redo verb used by the ref-move title and recovery
/// text. Exhaustive on [`HistoryMoveDir`] — no `_` fallback to drop meaning.
fn label_ja(dir: HistoryMoveDir) -> &'static str {
    match dir {
        HistoryMoveDir::Undo => "取り消し",
        HistoryMoveDir::Redo => "やり直し",
    }
}

/// Japanese rendering of one history note.
pub fn note_ja(note: &HistoryNote) -> String {
    match note {
        HistoryNote::MergeCommitUnsupported { sha, parents, op } => match op {
            HistoryOp::Undo => format!(
                "merge commit です(親 {} 個)。merge commit の undo は未対応です。\ncommit `{}`",
                parents, sha
            ),
            HistoryOp::Amend => format!(
                "merge commit です(親 {} 個)。merge commit の amend は未対応です。\ncommit `{}`",
                parents, sha
            ),
        },
        HistoryNote::RootCommit { sha, op } => match op {
            HistoryOp::Undo => format!(
                "root commit です(親なし)。これより前には戻れません。\ncommit `{}`",
                sha
            ),
            HistoryOp::Amend => format!(
                "root commit です(親なし)。root commit の amend は未対応です。\ncommit `{}`",
                sha
            ),
        },
        HistoryNote::PushedHistoryRewrite { sha, op } => match op {
            HistoryOp::Undo => {
                super::advice_text(Msg::AdviceHistoryPushedHistoryRewriteUndo, &[sha])
            }
            HistoryOp::Amend => {
                super::advice_text(Msg::AdviceHistoryPushedHistoryRewriteAmend, &[sha])
            }
        },
        HistoryNote::AmendDivergesFromRemote { sha, branch } => {
            super::advice_text(Msg::AdviceHistoryAmendDivergesFromRemote, &[branch, sha])
        }
        HistoryNote::EmptyMessage => "commit メッセージを空にはできません。".to_string(),
        HistoryNote::NothingStagedForAmend => {
            super::advice_text(Msg::AdviceHistoryNothingStagedForAmend, &[])
        }
        HistoryNote::WrongBranch {
            branch,
            current,
            label,
        } => super::advice_text(
            Msg::AdviceHistoryWrongBranch,
            &[branch, current, &label.label_en_lower(), branch],
        ),
        HistoryNote::HeadNotOnBranch { label } => {
            super::advice_text(Msg::AdviceHistoryHeadNotOnBranch, &[&label.label_en()])
        }
        HistoryNote::EntryStaleBranchMoved {
            branch,
            now,
            expected,
        } => super::advice_text(
            Msg::AdviceHistoryEntryStaleBranchMoved,
            &[branch, now, expected],
        ),
        HistoryNote::BranchNoTarget { branch } => {
            format!("branch `{}` に対象 commit がありません。", branch)
        }
        HistoryNote::BranchGone { branch } => format!("branch `{}` はもう存在しません。", branch),
        HistoryNote::EntryStaleUnreachable { sha } => {
            super::advice_text(Msg::AdviceHistoryEntryStaleUnreachable, &[sha])
        }
        HistoryNote::SoftMovePreservesChanges => {
            super::advice_text(Msg::AdviceHistorySoftMovePreservesChanges, &[])
        }
    }
}

/// Japanese rendering of one history title.
pub fn title_ja(title: &HistoryTitle) -> String {
    match title {
        HistoryTitle::UndoCommit {
            sha,
            summary,
            blocked,
        } => {
            if *blocked {
                "commit の undo(実行不可、blockers を確認)".to_string()
            } else {
                format!(
                    "commit `{}` \"{}\" を undo(変更は stage されます)",
                    sha, summary
                )
            }
        }
        HistoryTitle::Amend {
            sha,
            summary,
            mode,
            blocked,
        } => {
            if *blocked {
                "最新 commit の amend(実行不可、blockers を確認)".to_string()
            } else {
                let mode_label = match mode {
                    AmendMode::MessageOnly => "メッセージのみ",
                    AmendMode::Staged => "stage 済みを取り込み",
                    AmendMode::Both => "stage 済みを取り込み + メッセージ",
                };
                format!(
                    "commit `{}` \"{}\" を amend({}、SHA が変わります)",
                    sha, summary, mode_label
                )
            }
        }
        HistoryTitle::HistoryMove {
            label,
            kind_slug,
            branch,
            from,
            to,
        } => format!(
            "`{}` の {} を{}({} → {})",
            branch,
            kind_slug,
            label_ja(*label),
            from,
            to
        ),
    }
}

/// Japanese rendering of one history recovery block.
pub fn recovery_ja(recovery: &HistoryRecovery) -> String {
    match recovery {
        HistoryRecovery::Undo { blocked: true, .. } => {
            "この undo は実行できません(上記の blockers を確認してください)。".to_string()
        }
        HistoryRecovery::Undo { sha, blocked: false } => format!(
            "取り消した commit は削除されず、オブジェクトストアと reflog に残ります。\n\
             同じ SHA に復元するには:\n  git reset --soft {}\n\
             変更は undo 直後に stage されます。\n\
             HEAD 移動は reflog に残ります:\n  git reflog",
            sha
        ),
        HistoryRecovery::Amend { blocked: true, .. } => {
            "この amend は実行できません(上記の blockers を確認してください)。".to_string()
        }
        HistoryRecovery::Amend { sha, blocked: false } => format!(
            "amend は履歴を書き換えます。新しい SHA が付き、元の commit `{}` は branch から到達不能になります(reflog には残ります)。\n\
             作業ツリーと index を変えずに元に戻すには:\n  git reset --soft {}\n\
             HEAD 移動は reflog に残ります:\n  git reflog",
            sha, sha
        ),
        HistoryRecovery::HistoryMove {
            label,
            branch,
            from_short,
            to_short,
            kind_slug,
            from_full,
        } => format!(
            "{} は branch `{}` を {} から {} へ安全な参照移動で動かします。\
             {} commit は削除されず、オブジェクトストアと reflog に残ります:\n  git reflog\n\
             手動で復元するには:\n  git update-ref refs/heads/{} {}",
            label_ja(*label),
            branch,
            from_short,
            to_short,
            kind_slug,
            branch,
            from_full
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amend_recovery_keeps_the_working_tree() {
        let text = recovery_ja(&HistoryRecovery::Amend {
            sha: "a1b2c3d4".into(),
            blocked: false,
        });
        assert!(text.contains("git reset --soft a1b2c3d4"));
        assert!(!text.contains("reset --hard"));
    }
}
