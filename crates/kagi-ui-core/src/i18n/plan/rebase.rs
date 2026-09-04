//! JA strings for `RebaseNote`/`RebaseTitle`/`RebaseRecovery` (rebase-current-onto).

use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};

/// Japanese rendering of one rebase note.
pub fn note_ja(note: &RebaseNote) -> String {
    match note {
        RebaseNote::DetachedHead => {
            "HEAD が detached です。rebase には branch が必要です。".to_string()
        }
        RebaseNote::DirtyWorkingTree => {
            "作業ツリーに未 commit の変更があります。先に commit か stash か破棄してください。"
                .to_string()
        }
        RebaseNote::InvalidOnto { onto } => {
            format!("branch / commit として解決できません。\nonto `{}`", onto)
        }
        RebaseNote::AlreadyUpToDate { branch, onto } => format!(
            "すでに追従しています。rebase する内容はありません。\nbranch `{}` / onto `{}`",
            branch, onto
        ),
        RebaseNote::MayConflict => {
            "rebase は途中で conflict により停止することがあります。conflict エディタで commit ごとに解決してから Continue してください。".to_string()
        }
    }
}

/// Japanese rendering of one rebase title.
pub fn title_ja(title: &RebaseTitle) -> String {
    match title {
        RebaseTitle::RebaseCurrentOnto { branch, onto } => {
            format!("`{}` を `{}` の上に rebase", branch, onto)
        }
    }
}

/// Japanese rendering of one rebase recovery block.
pub fn recovery_ja(recovery: &RebaseRecovery) -> String {
    match recovery {
        RebaseRecovery::RebaseCurrentOnto { branch, from } => format!(
            "rebase 中はconflict バナーから abort すれば `{branch}` を {from} へ戻せます。完了済みなら rebase 前の先端を復元:\n  git update-ref refs/heads/{branch} {from}"
        ),
    }
}
