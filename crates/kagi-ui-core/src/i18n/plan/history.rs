//! JA strings for `HistoryNote` (ADR-0129 appendix §B-1 — undo / amend /
//! undo·redo history-move).

use kagi_domain::plan::AmendMode;
use kagi_domain::plan_note::{HistoryNote, HistoryOp, HistoryRecovery, HistoryTitle};

/// JA rendering of the "Undo"/"Redo" verb used by the undo/redo ref-move
/// title and recovery text.
fn label_ja(label: &str) -> &'static str {
    match label {
        "Undo" => "取り消し",
        "Redo" => "やり直し",
        _ => "操作",
    }
}

/// Japanese rendering of one history note.
pub fn note_ja(note: &HistoryNote) -> String {
    match note {
        HistoryNote::MergeCommitUnsupported { sha, parents, op } => match op {
            HistoryOp::Undo => format!(
                "コミット {} はマージコミットです(親 {} 個)。マージコミットの undo は MVP では未対応です。",
                sha, parents
            ),
            HistoryOp::Amend => format!(
                "コミット {} はマージコミットです(親 {} 個)。マージコミットの amend は未対応です。",
                sha, parents
            ),
        },
        HistoryNote::RootCommit { sha, op } => match op {
            HistoryOp::Undo => format!(
                "コミット {} はルートコミットです(親なし)。これより前には戻れません。",
                sha
            ),
            HistoryOp::Amend => format!(
                "コミット {} はルートコミットです(親なし)。ルートコミットの amend は MVP では未対応です。",
                sha
            ),
        },
        HistoryNote::PushedHistoryRewrite { sha, op } => match op {
            HistoryOp::Undo => format!(
                "コミット {} は upstream の追跡ブランチに push 済みです。push 済みコミットの undo は公開済み履歴を書き換えることになるため許可されていません。代わりに `git revert` で打ち消しコミットを作成してください。",
                sha
            ),
            HistoryOp::Amend => format!(
                "コミット {} は upstream の追跡ブランチに push 済みです。公開済み履歴の amend は許可されていません(ADR-0040)。修正は新しいコミットとして行ってください。",
                sha
            ),
        },
        HistoryNote::EmptyMessage => "コミットメッセージを空にすることはできません。".to_string(),
        HistoryNote::NothingStagedForAmend => {
            "コミットに取り込むステージ済みの変更がありません。先に変更をステージするか、メッセージのみの amend を使用してください。".to_string()
        }
        HistoryNote::WrongBranch {
            branch,
            current,
            label,
        } => format!(
            "この操作はブランチ '{}' 上で行われましたが、現在のブランチは '{}' です。{} するには '{}' に切り替えてください。",
            branch, current, label, branch
        ),
        HistoryNote::HeadNotOnBranch { label } => format!(
            "HEAD がブランチを指していません。{} には対象ブランチをチェックアウトしている必要があります。",
            label
        ),
        HistoryNote::EntryStaleBranchMoved {
            branch,
            now,
            expected,
        } => format!(
            "ブランチ '{}' はこの操作以降に移動しています(現在 {}、想定 {})。この履歴エントリは古いためスキップされます。",
            branch, now, expected
        ),
        HistoryNote::BranchNoTarget { branch } => {
            format!("ブランチ '{}' に対象コミットがありません。", branch)
        }
        HistoryNote::BranchGone { branch } => format!("ブランチ '{}' はもう存在しません。", branch),
        HistoryNote::EntryStaleUnreachable { sha } => format!(
            "対象コミット {} はオブジェクトストアから到達できません。この履歴エントリは古いためスキップされます。",
            sha
        ),
        HistoryNote::SoftMovePreservesChanges => {
            "コミットされていない変更があります。これらはそのまま保持されます — 移動するのはブランチの参照のみです(soft reset — インデックスと作業ツリーは変更されません)。".to_string()
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
                "コミットの undo(実行不可 — blockers を確認してください)".to_string()
            } else {
                format!(
                    "コミット {} '{}' を undo — 変更はステージされます",
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
                "最新コミットの amend(実行不可 — blockers を確認してください)".to_string()
            } else {
                let mode_label = match mode {
                    AmendMode::MessageOnly => "メッセージのみ",
                    AmendMode::Staged => "ステージ済みを取り込み",
                    AmendMode::Both => "ステージ済みを取り込み + メッセージ",
                };
                format!(
                    "コミット {} '{}' を amend({}) — SHA が変わります",
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
            label_ja(label),
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
            "取り消したコミットは削除されません — オブジェクトストアと reflog に残り続けます。\n\
             完全に復元する(同じ SHA で再コミットする)には:\n  git reset --soft {}\n\
             取り消したコミットの変更は undo 直後にステージされます。\n\
             reflog にはすべての HEAD 移動が記録されます:\n  git reflog",
            sha
        ),
        HistoryRecovery::Amend { blocked: true, .. } => {
            "この amend は実行できません(上記の blockers を確認してください)。".to_string()
        }
        HistoryRecovery::Amend { sha, blocked: false } => format!(
            "amend は履歴を書き換えます。新しいコミットには新しい SHA が付き、元のコミット {} \
             はブランチから到達できなくなります(ただし reflog には残ります)。\n\
             元のコミットに戻すには:\n  git reset --hard {}\n\
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
            "{} はブランチ '{}' を {} から {} へ、安全な参照移動で動かします(reset --hard も clean も使いません)。\
             {} コミットは削除されず、オブジェクトストアと reflog に残ります:\n  git reflog\n\
             手動で復元するには:\n  git update-ref refs/heads/{} {}",
            label_ja(label),
            branch,
            from_short,
            to_short,
            kind_slug,
            branch,
            from_full
        ),
    }
}
