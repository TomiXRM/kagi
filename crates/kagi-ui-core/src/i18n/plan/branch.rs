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

/// Japanese rendering of one branch note.
pub fn note_ja(note: &BranchNote) -> String {
    match note {
        BranchNote::CommitMissing { sha } => {
            format!("commit がありません。\ncommit `{}`", sha)
        }
        BranchNote::RenameRefOnlyDirty => {
            "リネームは ref だけを変更します。作業ツリーは変わりません。".to_string()
        }
        BranchNote::RenameRemoteNotRenamed => {
            "remote branch はリネームされません。local の設定だけ引き継ぎます。".to_string()
        }
        BranchNote::DeleteCurrentBranch { name } => format!(
            "checkout 中の branch は削除できません。別の branch に切り替えてください。\nbranch `{}`",
            name
        ),
        BranchNote::DeleteBranchInLockedWorktree { name, path } => format!(
            "ロックされた worktree で checkout 中です。先にロックを解除してください。\nbranch `{}` / worktree `{}`",
            name, path
        ),
        BranchNote::DeleteBranchInDirtyWorktree { name, path } => format!(
            "worktree に未 commit の変更があります。先に commit か破棄してください。\nbranch `{}` / worktree `{}`",
            name, path
        ),
        BranchNote::DeleteRemovesPinningWorktree { name, path } => format!(
            "clean な worktree で checkout 中です。worktree を削除してから branch を削除します。\nbranch `{}` / worktree `{}`",
            name, path
        ),
        BranchNote::DeleteDetachedAtTip { name } => format!(
            "HEAD がこの branch の先端を指しています（detached）。削除できません。\nbranch `{}`",
            name
        ),
        BranchNote::DeleteUnmerged { name, tip } => format!(
            "未 merge の commit があります。先に merge か破棄してください（強制削除は非対応）。\nbranch `{}` / 先端 `{}`",
            name, tip
        ),
        BranchNote::DeleteSquashMerged { name, squash } => format!(
            "squash merge 済みです。変更は取り込み済みで、削除しても失われません。\nbranch `{}` / merge 先 `{}`",
            name, squash
        ),
        BranchNote::DeleteKeepsRemote { name } => format!(
            "削除するのは local branch だけです。remote は残ります。\nbranch `{}`",
            name
        ),
    }
}

/// Japanese rendering of one branch title.
pub fn title_ja(title: &BranchTitle) -> String {
    match title {
        BranchTitle::CreateBranch { name, at, checkout } => {
            if *checkout {
                format!("branch を作成して checkout: `{}`（起点 {}）", name, at)
            } else {
                format!("branch を作成: `{}`（起点 {}）", name, at)
            }
        }
        BranchTitle::RenameBranch { old, new } => {
            format!("branch をリネーム: `{}` → `{}`", old, new)
        }
        BranchTitle::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!("branch を削除: `{}`（先端 {}）", name, tip),
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
            "復元する:\n  git branch {} {}\n先端 commit `{}` は GC まで残ります。",
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
