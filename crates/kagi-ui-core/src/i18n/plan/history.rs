//! JA strings for `HistoryNote` (ADR-0129 appendix §B-1 — undo / amend /
//! undo·redo history-move).

use kagi_domain::plan::AmendMode;
use kagi_domain::plan_note::{
    HistoryMoveDir, HistoryNote, HistoryOp, HistoryRecovery, HistoryTitle,
};

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
                "commit {} は merge commit です(親 {} 個)。merge commit の undo は MVP では未対応です。",
                sha, parents
            ),
            HistoryOp::Amend => format!(
                "commit {} は merge commit です(親 {} 個)。merge commit の amend は未対応です。",
                sha, parents
            ),
        },
        HistoryNote::RootCommit { sha, op } => match op {
            HistoryOp::Undo => format!(
                "commit {} は root commit です(親なし)。これより前には戻れません。",
                sha
            ),
            HistoryOp::Amend => format!(
                "commit {} は root commit です(親なし)。root commit の amend は MVP では未対応です。",
                sha
            ),
        },
        HistoryNote::PushedHistoryRewrite { sha, op } => match op {
            HistoryOp::Undo => format!(
                "commit {} は upstream の追跡 branch に push 済みです。push 済み commit の undo は公開済み履歴を書き換えることになるため許可されていません。代わりに `git revert` で打ち消し commit を作成してください。",
                sha
            ),
            HistoryOp::Amend => format!(
                "commit {} は push 済みで、この branch は他の人が土台にしているものです。履歴を書き換えると、既に fetch 済みの clone がすべて取り残されます。確認の有無にかかわらず amend は拒否されます。修正は新しい commit として行ってください。",
                sha
            ),
        },
        HistoryNote::AmendDivergesFromRemote { sha, branch } => format!(
            "commit {} は既に remote にあります。amend は新しい commit で置き換えるため、'{}' は upstream から分岐し、通常の push は拒否されます。branch メニューの 'Force-with-lease push...' で反映してください(最後の fetch 以降に誰かが push していれば失敗します)。",
            sha, branch
        ),
        HistoryNote::EmptyMessage => "commit メッセージを空にすることはできません。".to_string(),
        HistoryNote::NothingStagedForAmend => {
            "commit に取り込む stage 済みの変更がありません。先に変更を stage するか、メッセージのみの amend を使用してください。".to_string()
        }
        HistoryNote::WrongBranch {
            branch,
            current,
            label,
        } => format!(
            "この操作は branch '{}' 上で行われましたが、現在の branch は '{}' です。{} するには '{}' に切り替えてください。",
            branch, current, label.label_en_lower(), branch
        ),
        HistoryNote::HeadNotOnBranch { label } => format!(
            "HEAD が branch を指していません。{} には対象 branch を checkout している必要があります。",
            label.label_en()
        ),
        HistoryNote::EntryStaleBranchMoved {
            branch,
            now,
            expected,
        } => format!(
            "branch '{}' はこの操作以降に移動しています(現在 {}、想定 {})。この履歴エントリは古いためスキップされます。",
            branch, now, expected
        ),
        HistoryNote::BranchNoTarget { branch } => {
            format!("branch '{}' に対象 commit がありません。", branch)
        }
        HistoryNote::BranchGone { branch } => format!("branch '{}' はもう存在しません。", branch),
        HistoryNote::EntryStaleUnreachable { sha } => format!(
            "対象 commit {} はオブジェクトストアから到達できません。この履歴エントリは古いためスキップされます。",
            sha
        ),
        HistoryNote::SoftMovePreservesChanges => {
            "commit されていない変更があります。これらはそのまま保持されます — 移動するのは branch の参照のみです(soft reset — インデックスと作業ツリーは変更されません)。".to_string()
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
                "commit の undo(実行不可 — blockers を確認してください)".to_string()
            } else {
                format!(
                    "commit {} '{}' を undo — 変更は stage されます",
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
                "最新 commit の amend(実行不可 — blockers を確認してください)".to_string()
            } else {
                let mode_label = match mode {
                    AmendMode::MessageOnly => "メッセージのみ",
                    AmendMode::Staged => "stage 済みを取り込み",
                    AmendMode::Both => "stage 済みを取り込み + メッセージ",
                };
                format!(
                    "commit {} '{}' を amend({}) — SHA が変わります",
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
            "'{}' の {} を{}({} → {})",
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
            "取り消した commit は削除されません — オブジェクトストアと reflog に残り続けます。\n\
             完全に復元する(同じ SHA で再 commit する)には:\n  git reset --soft {}\n\
             取り消した commit の変更は undo 直後に stage されます。\n\
             reflog にはすべての HEAD 移動が記録されます:\n  git reflog",
            sha
        ),
        HistoryRecovery::Amend { blocked: true, .. } => {
            "この amend は実行できません(上記の blockers を確認してください)。".to_string()
        }
        HistoryRecovery::Amend { sha, blocked: false } => format!(
            "amend は履歴を書き換えます。新しい commit には新しい SHA が付き、元の commit {} \
             は branch から到達できなくなります(ただし reflog には残ります)。\n\
             元の commit に戻すには:\n  git reset --hard {}\n\
             reflog にはすべての HEAD 移動が記録されます:\n  git reflog",
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
            "{} は branch '{}' を {} から {} へ、安全な参照移動で動かします(reset --hard も clean も使いません)。\
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
