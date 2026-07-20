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
            format!("コミット '{}' はこのリポジトリに存在しません。", sha)
        }
        BranchNote::RenameRefOnlyDirty => {
            "作業ツリーが dirty ですが、ブランチのリネームは ref のみを変更するためファイルには影響しません。".to_string()
        }
        BranchNote::RenameRemoteNotRenamed => {
            "リモートブランチ名は自動的にはリネームされません。ローカルのブランチ設定のみが引き継がれます。".to_string()
        }
        BranchNote::DeleteCurrentBranch { name } => format!(
            "ブランチ '{}' は現在チェックアウト中のブランチです。削除する前に別のブランチにチェックアウトしてください。",
            name
        ),
        BranchNote::DeleteBranchInLockedWorktree { name, path } => format!(
            "ブランチ '{}' はロックされた worktree '{}' でチェックアウトされています。ブランチを削除する前に、まずロックを解除してください(サイドバーの worktree を右クリック → Unlock worktree)。",
            name, path
        ),
        BranchNote::DeleteBranchInDirtyWorktree { name, path } => format!(
            "ブランチ '{}' は worktree '{}' でチェックアウトされており、そこには未コミットの変更があります。まずそこでコミットするか変更を破棄してください — 作業が残っている間、worktree は削除されません。",
            name, path
        ),
        BranchNote::DeleteRemovesPinningWorktree { name, path } => format!(
            "ブランチ '{}' はクリーンな worktree '{}' でチェックアウトされています。この worktree を削除してから、ブランチを削除します。",
            name, path
        ),
        BranchNote::DeleteDetachedAtTip { name } => format!(
            "HEAD は detached 状態で、'{}' と同じコミットを指しています。HEAD がその先端にある間、このブランチは削除できません。",
            name
        ),
        BranchNote::DeleteUnmerged { name, tip } => format!(
            "ブランチ '{}' には未マージのコミットがあります(先端 {} は HEAD から到達できません)。削除する前に手動でマージするか破棄してください。強制削除はサポートされていません。",
            name, tip
        ),
        BranchNote::DeleteKeepsRemote { name } => format!(
            "ブランチ '{}' にはアップストリームの追跡ブランチが設定されています。削除されるのはローカルブランチのみで、リモートブランチは削除されません。",
            name
        ),
    }
}

/// Japanese rendering of one branch title.
pub fn title_ja(title: &BranchTitle) -> String {
    match title {
        BranchTitle::CreateBranch { name, at, checkout } => {
            if *checkout {
                format!("ブランチ '{}' を {} に作成してチェックアウト", name, at)
            } else {
                format!("ブランチ '{}' を {} に作成", name, at)
            }
        }
        BranchTitle::RenameBranch { old, new } => {
            format!("ブランチ '{}' を '{}' にリネーム", old, new)
        }
        BranchTitle::DeleteBranch {
            name,
            tip: Some(tip),
        } => format!("ブランチ '{}' を削除(先端 {})", name, tip),
        BranchTitle::DeleteBranch { name, tip: None } => format!("ブランチ '{}' を削除", name),
    }
}

/// Japanese rendering of one branch recovery block.
pub fn recovery_ja(recovery: &BranchRecovery) -> String {
    match recovery {
        BranchRecovery::CreateBranch { name } => format!(
            "新しいブランチ '{}' は副作用なく削除できます:\n  git branch -d {}\n(ブランチの作成は HEAD を移動せず、作業ツリーも変更しません。)",
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
            "削除したブランチを復元するには:\n  git branch {} {}\nブランチの先端コミット '{}' は GC されるまでオブジェクトストアに残ります。",
            name, tip, tip
        ),
        BranchRecovery::DeleteBranch { name, tip: None } => format!(
            "ブランチ '{}' が見つかりませんでした。`git branch` でローカルブランチの一覧を確認してください。",
            name
        ),
    }
}
