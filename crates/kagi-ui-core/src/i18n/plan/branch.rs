//! JA strings for `BranchNote` (ADR-0129 appendix §B-9 — create / rename /
//! delete branch).
//!
//! `create-branch`/`rename-branch`'s branch-name-validity blockers
//! (`BranchNameError`) are localized separately via
//! `crate::ui::i18n::branch_name_error` (§E) — they never reach `note_ja`
//! below as `BranchNote`.
//!
//! Wording rules (#376): keep prose short and plain; put IDs/paths on their own
//! labeled line, not buried inside a sentence.

use kagi_domain::plan_note::{BranchNote, BranchRecovery, BranchTitle};

use crate::i18n::Msg;

/// JA template for `Msg::AdviceDeleteUnmerged`. The `{}` take, in order:
/// commit count, branch name, tip.
pub(crate) const ADVICE_DELETE_UNMERGED: &str =
    "未 merge の branch です。削除すると {} commit が他の ref から到達不能になりますが、復元用 ref で保持します。2 回確認すると削除します。\nbranch `{}` / 先端 `{}`";

pub(crate) const ADVICE_BRANCH_RENAME_REF_ONLY_DIRTY: &str =
    "リネームは ref だけを変更します。作業ツリーは変わりません。";
pub(crate) const ADVICE_BRANCH_RENAME_REMOTE_NOT_RENAMED: &str =
    "remote branch はリネームされません。local の設定だけ引き継ぎます。";
pub(crate) const ADVICE_BRANCH_DELETE_CURRENT_BRANCH: &str =
    "checkout 中の branch は削除できません。別の branch に切り替えてください。\nbranch `{}`";
pub(crate) const ADVICE_BRANCH_DELETE_BRANCH_CHECKED_OUT: &str =
    "ブランチ '{}' は worktree '{}' で checkout 中です。削除する前にその worktree を別のブランチへ切り替えてください。";
pub(crate) const ADVICE_BRANCH_DELETE_BRANCH_IN_LOCKED_WORKTREE: &str =
    "ロックされた worktree で checkout 中です。先にロックを解除してください。\nbranch `{}` / worktree `{}`";
pub(crate) const ADVICE_BRANCH_DELETE_BRANCH_IN_DIRTY_WORKTREE: &str =
    "worktree に未 commit の変更があります。先に commit か破棄してください。\nbranch `{}` / worktree `{}`";
pub(crate) const ADVICE_BRANCH_DELETE_REMOVES_PINNING_WORKTREE: &str =
    "clean な worktree で checkout 中です。worktree を削除してから branch を削除します。\nbranch `{}` / worktree `{}`";
pub(crate) const ADVICE_BRANCH_DELETE_SQUASH_MERGED: &str =
    "squash merge 済みです。変更は取り込み済みで、削除しても失われません。\nbranch `{}` / merge 先 `{}`";
pub(crate) const ADVICE_BRANCH_DELETE_KEEPS_REMOTE: &str =
    "削除するのは local branch だけです。remote は残ります。\nbranch `{}`";

/// Japanese rendering of one branch note.
pub fn note_ja(note: &BranchNote) -> String {
    match note {
        BranchNote::CommitMissing { sha } => {
            format!("commit がありません。\ncommit `{}`", sha)
        }
        BranchNote::RenameRefOnlyDirty => {
            super::advice_text(Msg::AdviceBranchRenameRefOnlyDirty, &[])
        }
        BranchNote::RenameRemoteNotRenamed => {
            super::advice_text(Msg::AdviceBranchRenameRemoteNotRenamed, &[])
        }
        BranchNote::DeleteCurrentBranch { name } => {
            super::advice_text(Msg::AdviceBranchDeleteCurrentBranch, &[name])
        }
        BranchNote::DeleteBranchCheckedOut { name, path } => {
            super::advice_text(Msg::AdviceBranchDeleteBranchCheckedOut, &[name, path])
        }
        BranchNote::DeleteBranchInLockedWorktree { name, path } => {
            super::advice_text(Msg::AdviceBranchDeleteBranchInLockedWorktree, &[name, path])
        }
        BranchNote::DeleteBranchInDirtyWorktree { name, path } => {
            super::advice_text(Msg::AdviceBranchDeleteBranchInDirtyWorktree, &[name, path])
        }
        BranchNote::DeleteRemovesPinningWorktree { name, path } => {
            super::advice_text(Msg::AdviceBranchDeleteRemovesPinningWorktree, &[name, path])
        }
        BranchNote::DeleteDetachedAtTip { name } => format!(
            "HEAD がこの branch の先端を指しています(detached)。削除できません。\nbranch `{}`",
            name
        ),
        BranchNote::DeleteUnmerged { name, tip, commits } => {
            super::advice_text(Msg::AdviceDeleteUnmerged, &[commits, name, tip])
        }
        BranchNote::DeleteSquashMerged { name, squash } => {
            super::advice_text(Msg::AdviceBranchDeleteSquashMerged, &[name, squash])
        }
        BranchNote::DeleteKeepsRemote { name } => {
            super::advice_text(Msg::AdviceBranchDeleteKeepsRemote, &[name])
        }
    }
}

/// Japanese rendering of one branch title.
pub fn title_ja(title: &BranchTitle) -> String {
    match title {
        BranchTitle::CreateBranch { name, at, checkout } => {
            if *checkout {
                format!("branch を作成して checkout: `{}`(起点 `{}`)", name, at)
            } else {
                format!("branch を作成: `{}`(起点 `{}`)", name, at)
            }
        }
        BranchTitle::RenameBranch { old, new } => {
            format!("branch をリネーム: `{}` → `{}`", old, new)
        }
        BranchTitle::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!("branch を削除: `{}`(先端 `{}`)", name, tip),
        BranchTitle::DeleteBranch { name, tip: None } => format!("branch を削除: `{}`", name),
    }
}

/// Japanese rendering of one branch recovery block.
pub fn recovery_ja(recovery: &BranchRecovery) -> String {
    match recovery {
        BranchRecovery::CreateBranch { name } => {
            format!("副作用なく削除できます:\n  git branch -d {}", name)
        }
        BranchRecovery::RenameBranch { old, new } => {
            format!("元に戻す:\n  git branch -m {} {}", new, old)
        }
        BranchRecovery::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!(
            "復元する:\n  git branch {} {}\n先端 commit `{}` は復元用 ref で保持します。GC 後は操作記録の backup ref から復元してください。",
            name, tip, tip
        ),
        BranchRecovery::DeleteBranch { name, tip: None } => {
            format!(
                "branch が見つかりません。`git branch` で一覧を確認してください。\nbranch `{}`",
                name
            )
        }
    }
}
