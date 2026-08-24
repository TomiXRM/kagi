//! JA strings for `BranchNote` (ADR-0129 appendix §B-9 — create / rename /
//! delete branch).
//!
//! `create-branch`/`rename-branch`'s branch-name-validity blockers
//! (`BranchNameError`) are localized separately via
//! `crate::ui::i18n::branch_name_error` (§E) — they never reach `note_ja`
//! below as `BranchNote`.

use kagi_domain::plan_note::{BranchNote, BranchRecovery, BranchTitle};

/// Japanese rendering of one branch note.
pub fn note_ja(note: &BranchNote) -> String {
    match note {
        BranchNote::CommitMissing { sha } => {
            format!("commit '{}' はこのリポジトリに存在しません。", sha)
        }
        BranchNote::RenameRefOnlyDirty => {
            "作業ツリーが dirty ですが、branch のリネームは ref のみを変更するためファイルには影響しません。".to_string()
        }
        BranchNote::RenameRemoteNotRenamed => {
            "remote branch 名は自動的にはリネームされません。ローカルの branch 設定のみが引き継がれます。".to_string()
        }
        BranchNote::DeleteCurrentBranch { name } => format!(
            "branch '{}' は現在 checkout 中の branch です。削除する前に別の branch に checkout してください。",
            name
        ),
        BranchNote::DeleteBranchInLockedWorktree { name, path } => format!(
            "branch '{}' はロックされた worktree '{}' で checkout されています。branch を削除する前に、まずロックを解除してください(サイドバーの worktree を右クリック → Unlock worktree)。",
            name, path
        ),
        BranchNote::DeleteBranchInDirtyWorktree { name, path } => format!(
            "branch '{}' は worktree '{}' で checkout されており、そこには未 commit の変更があります。まずそこで commit するか変更を破棄してください — 作業が残っている間、worktree は削除されません。",
            name, path
        ),
        BranchNote::DeleteRemovesPinningWorktree { name, path } => format!(
            "branch '{}' はクリーンな worktree '{}' で checkout されています。この worktree を削除してから、branch を削除します。",
            name, path
        ),
        BranchNote::DeleteDetachedAtTip { name } => format!(
            "HEAD は detached 状態で、'{}' と同じ commit を指しています。HEAD がその先端にある間、この branch は削除できません。",
            name
        ),
        BranchNote::DeleteUnmerged { name, tip } => format!(
            "branch '{}' には未 merge の commit があります(先端 {} は HEAD から到達できません)。削除する前に手動で merge するか破棄してください。強制削除はサポートされていません。",
            name, tip
        ),
        BranchNote::DeleteSquashMerged { name, squash } => format!(
            "branch '{}' は {} として squash merge 済みです。commit 自体は HEAD の祖先ではない(グラフ上は行き止まりに見えます)ものの、同一の変更はすでに取り込まれています。削除しても失われるものはありません。",
            name, squash
        ),
        BranchNote::DeleteKeepsRemote { name } => format!(
            "branch '{}' にはアップストリームの追跡 branch が設定されています。削除されるのは local branch のみで、remote branch は削除されません。",
            name
        ),
    }
}

/// Japanese rendering of one branch title.
pub fn title_ja(title: &BranchTitle) -> String {
    match title {
        BranchTitle::CreateBranch { name, at, checkout } => {
            if *checkout {
                format!("branch '{}' を {} に作成して checkout", name, at)
            } else {
                format!("branch '{}' を {} に作成", name, at)
            }
        }
        BranchTitle::RenameBranch { old, new } => {
            format!("branch '{}' を '{}' にリネーム", old, new)
        }
        BranchTitle::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!("branch '{}' を削除(先端 {})", name, tip),
        BranchTitle::DeleteBranch { name, tip: None } => format!("branch '{}' を削除", name),
    }
}

/// Japanese rendering of one branch recovery block.
pub fn recovery_ja(recovery: &BranchRecovery) -> String {
    match recovery {
        BranchRecovery::CreateBranch { name } => format!(
            "新しい branch '{}' は副作用なく削除できます:\n  git branch -d {}\n(branch の作成は HEAD を移動せず、作業ツリーも変更しません。)",
            name, name
        ),
        BranchRecovery::RenameBranch { old, new } => format!(
            "変更されるのはローカルの ref のみです。元に戻すには: git branch -m {} {}",
            new, old
        ),
        BranchRecovery::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!(
            "削除した branch を復元するには:\n  git branch {} {}\nbranch の先端 commit '{}' は GC されるまでオブジェクトストアに残ります。",
            name, tip, tip
        ),
        BranchRecovery::DeleteBranch { name, tip: None } => format!(
            "branch '{}' が見つかりませんでした。`git branch` で local branch の一覧を確認してください。",
            name
        ),
    }
}
