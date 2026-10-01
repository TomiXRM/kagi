//! JA strings for `RebaseNote`/`RebaseTitle`/`RebaseRecovery` (rebase-current-onto).

use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_REBASE_DIRTY_WORKING_TREE: &str =
    "作業ツリーに未 commit の変更があります。先に commit か stash か破棄してください。";
pub(crate) const ADVICE_REBASE_MAY_CONFLICT: &str =
    "rebase は途中で conflict により停止することがあります。conflict エディタで commit ごとに解決してから Continue してください。";

/// Japanese rendering of one rebase note.
pub fn note_ja(note: &RebaseNote) -> String {
    match note {
        RebaseNote::DetachedHead => {
            "HEAD が detached です。rebase には branch が必要です。".to_string()
        }
        RebaseNote::DirtyWorkingTree => super::advice_text(Msg::AdviceRebaseDirtyWorkingTree, &[]),
        RebaseNote::InvalidOnto { onto } => {
            format!("branch / commit として解決できません。\nonto `{}`", onto)
        }
        RebaseNote::AlreadyUpToDate { branch, onto } => format!(
            "すでに追従しています。rebase する内容はありません。\nbranch `{}` / onto `{}`",
            branch, onto
        ),
        RebaseNote::MayConflict => super::advice_text(Msg::AdviceRebaseMayConflict, &[]),
        RebaseNote::ReplayRangeHasMerges { branch, count } => format!(
            "`{}` に merge commit が {} 個含まれています。git replay は merge を replay できません。その worktree から Rebase onto を使ってください。",
            branch, count
        ),
        RebaseNote::ReplayWorktreeDirty { branch, path } => format!(
            "`{}` は {} で checkout 中で、未 commit の変更があります。先にそこで commit か stash してください。branch を動かすとその作業ツリーが取り残されます。",
            branch, path
        ),
        RebaseNote::ReplayConflicts { branch, onto } => format!(
            "`{}` を `{}` の上に replay すると conflict します。git replay は conflict を解決できません。branch の worktree から rebase してください。",
            branch, onto
        ),
        RebaseNote::ReplayNothingToDo { branch, onto } => format!(
            "replay するものがありません。`{}` は既に `{}` の上にあります。",
            branch, onto
        ),
        RebaseNote::ReplayUpdates { count, sample, more } => {
            let mut list = sample.join("\n  ");
            if *more > 0 {
                list = format!("{}(+{} 件)", list, more);
            }
            format!(
                "git replay が印字したとおりに {} 個の ref を更新します:\n  {}",
                count, list
            )
        }
        RebaseNote::ReplayDropsSignatures { count } => format!(
            "署名付き commit {} 個が、署名なしで作り直されます。",
            count
        ),
        RebaseNote::ReplayWorktreeStale { branch, path } => format!(
            "`{}` は {} で checkout 中です。HEAD は branch に追従しますが、その worktree の index と作業ファイルはこの操作では更新されません。後でそこで `git reset --keep` を実行するか、その worktree から rebase してください。",
            branch, path
        ),
    }
}

/// Japanese rendering of one rebase title.
pub fn title_ja(title: &RebaseTitle) -> String {
    match title {
        RebaseTitle::RebaseCurrentOnto { branch, onto } => {
            format!("`{}` を `{}` の上に rebase", branch, onto)
        }
        RebaseTitle::ReplayOnto { branch, onto } => {
            format!("`{}` を `{}` の上に replay", branch, onto)
        }
    }
}

/// Japanese rendering of one rebase recovery block.
pub fn recovery_ja(recovery: &RebaseRecovery) -> String {
    match recovery {
        RebaseRecovery::RebaseCurrentOnto { branch, from } => format!(
            "rebase 中はconflict バナーから abort すれば `{branch}` を {from} へ戻せます。完了済みなら rebase 前の先端を復元:\n  git update-ref refs/heads/{branch} {from}"
        ),
        RebaseRecovery::ReplayOnto { branch, from } => format!(
            "replay は ref を動かすだけで checkout はしません。replay 前の先端を復元:\n  git update-ref refs/heads/{branch} {from}\n(同じ先端は この entry を forget するまで refs/kagi/backups/ に保持されます)"
        ),
    }
}
