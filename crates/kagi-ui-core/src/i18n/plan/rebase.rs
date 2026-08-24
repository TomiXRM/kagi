//! JA strings for `RebaseNote`/`RebaseTitle`/`RebaseRecovery` (rebase-current-onto).

use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};

/// Japanese rendering of one rebase note.
pub fn note_ja(note: &RebaseNote) -> String {
    match note {
        RebaseNote::DetachedHead => {
            "HEAD が detached 状態です。rebase にはアタッチされた branch が必要です。".to_string()
        }
        RebaseNote::DirtyWorkingTree => {
            "作業ツリーに未 commit の変更があります。rebase する前に commit・stash・破棄のいずれかを行ってください。".to_string()
        }
        RebaseNote::InvalidOnto { onto } => {
            format!("'{}' は branch または commit として解決できません。", onto)
        }
        RebaseNote::AlreadyUpToDate { branch, onto } => format!(
            "'{}' は既に '{}' に追従しています。rebase する内容がありません。",
            branch, onto
        ),
        RebaseNote::MayConflict => {
            "rebase は途中でコンフリクトにより停止することがあります。コンフリクトエディタで commit ごとに解決してから Continue してください。完了するまでシーケンスが継続します。".to_string()
        }
    }
}

/// Japanese rendering of one rebase title.
pub fn title_ja(title: &RebaseTitle) -> String {
    match title {
        RebaseTitle::RebaseCurrentOnto { branch, onto } => {
            format!("'{}' を '{}' の上に rebase", branch, onto)
        }
    }
}

/// Japanese rendering of one rebase recovery block.
pub fn recovery_ja(recovery: &RebaseRecovery) -> String {
    match recovery {
        RebaseRecovery::RebaseCurrentOnto { branch, from } => format!(
            "rebase が進行中の間は、コンフリクトバナーから abort すれば('git rebase --abort' 相当)'{branch}' を正確に {from} へ復元できます。既に完了している場合は、rebase 前の先端を次のコマンドで復元してください:\n  git update-ref refs/heads/{branch} {from}"
        ),
    }
}
