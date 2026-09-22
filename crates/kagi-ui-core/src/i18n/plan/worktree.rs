//! JA strings for `WorktreeNote` (ADR-0129 appendix §B-8 — create-branch+
//! checkout / create-worktree / unlock-worktree).

use kagi_domain::plan_note::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_WORKTREE_DIRTY_BLOCKS_CHECKOUT_AFTER_CREATE: &str =
    "作業ツリーに {} があります。branch 作成後の checkout で変更が失われる可能性があります。先に stash してください。";
pub(crate) const ADVICE_WORKTREE_CREATES_LINKED_WORKTREE: &str =
    "リンク worktree を作成します(起点 {})。\nworktree `{}` / branch `{}`";
pub(crate) const ADVICE_WORKTREE_LOCKED_WITH_REASON: &str =
    "ロック理由: {}。ロックは誰かが意図的に設定した保護です。不要か確認してください。";
pub(crate) const ADVICE_WORKTREE_INCLUDE_COPY: &str =
    ".worktreeinclude に一致する {} 件({})を新しい worktree にコピーします: {}。";
pub(crate) const ADVICE_WORKTREE_INCLUDE_SKIPPED_SYMLINKS: &str =
    "一致した symlink {} 件はスキップします(symlink はコピーしません)。";
pub(crate) const ADVICE_WORKTREE_INCLUDE_OVER_CAP: &str =
    ".worktreeinclude の一致 {} がコピー上限 {} を超えています。コピーは続行しますが大きくなる可能性があります(例: node_modules)。";
pub(crate) const ADVICE_WORKTREE_REMOVE_DIRTY: &str =
    "worktree に未 commit の変更があります({})。先に commit か stash してください(削除は force しません)。\nworktree `{}`";
pub(crate) const ADVICE_WORKTREE_REMOVE_LOCKED: &str =
    "worktree はロックされています({})。削除前にロックを解除してください(kagi は force しません)。\nworktree `{}`";
pub(crate) const ADVICE_WORKTREE_REMOVES_WORKTREE_DELETE_BRANCH: &str =
    "リンク worktree を削除し、branch も削除します。\nworktree `{}` / branch `{}`";
pub(crate) const ADVICE_WORKTREE_REMOVES_WORKTREE_KEEP_BRANCH: &str =
    "リンク worktree を削除します。branch は残します。\nworktree `{}` / branch `{}`";
pub(crate) const ADVICE_WORKTREE_LOCKS_WORKTREE: &str =
    "worktree をロックします。理由: {}。\nworktree `{}`";
pub(crate) const ADVICE_WORKTREE_PRUNE_PREVIEW: &str =
    "作業ディレクトリが消えた古い worktree 管理エントリ {} 件を prune します: {}。";
pub(crate) const ADVICE_WORKTREE_REPAIRS_WORKTREES: &str =
    "worktree の管理リンクを修復します(main / linked の移動に対応)。ファイルには触れず、.git のリンクのみを修復します。";
pub(crate) const ADVICE_WORKTREE_POST_CREATE_STEPS: &str =
    ".kagi/worktree.toml の作成後ステップ {} 件を実行します:{}";
pub(crate) const ADVICE_WORKTREE_POST_CREATE_STEPS_TRUST_REQUIRED: &str =
    ".kagi/worktree.toml の作成後ステップ {} 件を実行します:{}\n  ⚠ 確認すると設定を信頼し、上記 command を実行します(commit 済み設定は既定で未信頼)。";
pub(crate) const ADVICE_WORKTREE_PRE_REMOVE_STEPS: &str =
    ".kagi/worktree.toml の削除前ステップ {} 件を実行します(command が失敗または未信頼なら削除を中止):{}";
pub(crate) const ADVICE_WORKTREE_PRE_REMOVE_STEPS_TRUST_REQUIRED: &str =
    ".kagi/worktree.toml の削除前ステップ {} 件を実行します(command が失敗または未信頼なら削除を中止):{}\n  ⚠ 確認すると設定を信頼し、上記 command を実行します(commit 済み設定は既定で未信頼)。";

/// Japanese rendering of one worktree note.
pub fn note_ja(note: &WorktreeNote) -> String {
    match note {
        WorktreeNote::DirtyBlocksCheckoutAfterCreate { parts } => super::advice_text(
            Msg::AdviceWorktreeDirtyBlocksCheckoutAfterCreate,
            &[&parts.parts_en()],
        ),
        WorktreeNote::BranchInOtherWorktree { branch, path } => format!(
            "この branch は別の worktree で既に checkout 済みです。\nbranch `{}` / worktree `{}`",
            branch, path
        ),
        WorktreeNote::CreatesLinkedWorktree {
            path,
            branch,
            start,
        } => super::advice_text(
            Msg::AdviceWorktreeCreatesLinkedWorktree,
            &[start, path, branch],
        ),
        WorktreeNote::LockedWithReason { reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            super::advice_text(Msg::AdviceWorktreeLockedWithReason, &[&reason_display])
        }
        WorktreeNote::AlreadyUnlocked { name } => {
            format!(
                "worktree は既にロック解除されています。\nworktree `{}`",
                name
            )
        }
        WorktreeNote::LockStateUnreadable { name, err } => format!(
            "ロック状態を読み取れませんでした: {}\nworktree `{}`",
            err, name
        ),
        WorktreeNote::WorktreeMissing { name } => {
            format!("worktree が存在しません。\nworktree `{}`", name)
        }
        WorktreeNote::IncludeCopy {
            count,
            total_bytes,
            sample,
            more,
        } => {
            let mut names = sample.join(", ");
            if *more > 0 {
                names = format!("{} (他 {} 件)", names, more);
            }
            super::advice_text(
                Msg::AdviceWorktreeIncludeCopy,
                &[
                    count,
                    &kagi_domain::worktree_include::human_bytes(*total_bytes),
                    &names,
                ],
            )
        }
        WorktreeNote::IncludeSkippedSymlinks { count } => {
            super::advice_text(Msg::AdviceWorktreeIncludeSkippedSymlinks, &[count])
        }
        WorktreeNote::IncludeOverCap {
            total_bytes,
            cap_bytes,
        } => super::advice_text(
            Msg::AdviceWorktreeIncludeOverCap,
            &[
                &kagi_domain::worktree_include::human_bytes(*total_bytes),
                &kagi_domain::worktree_include::human_bytes(*cap_bytes),
            ],
        ),
        WorktreeNote::RemoveMainRefused => "main worktree は削除できません。".to_string(),
        WorktreeNote::RemoveDirty { path, summary } => {
            super::advice_text(Msg::AdviceWorktreeRemoveDirty, &[summary, path])
        }
        WorktreeNote::RemoveLocked { path, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            super::advice_text(Msg::AdviceWorktreeRemoveLocked, &[&reason_display, path])
        }
        WorktreeNote::RemovesWorktree {
            path,
            branch,
            delete_branch,
        } => {
            let branch_display = branch.as_deref().unwrap_or("(detached HEAD)");
            if *delete_branch {
                super::advice_text(
                    Msg::AdviceWorktreeRemovesWorktreeDeleteBranch,
                    &[path, &branch_display],
                )
            } else {
                super::advice_text(
                    Msg::AdviceWorktreeRemovesWorktreeKeepBranch,
                    &[path, &branch_display],
                )
            }
        }
        WorktreeNote::LocksWorktree { path, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由なし)".to_string(),
            };
            super::advice_text(Msg::AdviceWorktreeLocksWorktree, &[&reason_display, path])
        }
        WorktreeNote::AlreadyLocked { name, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            format!(
                "worktree は既にロックされています({})。\nworktree `{}`",
                reason_display, name
            )
        }
        WorktreeNote::PrunePreview {
            count,
            sample,
            more,
        } => {
            let mut names = sample.join(", ");
            if *more > 0 {
                names = format!("{} (他 {} 件)", names, more);
            }
            super::advice_text(Msg::AdviceWorktreePrunePreview, &[count, &names])
        }
        WorktreeNote::PruneNothing => "prune 対象の worktree はありません。".to_string(),
        WorktreeNote::RepairsWorktrees => {
            super::advice_text(Msg::AdviceWorktreeRepairsWorktrees, &[])
        }
        WorktreeNote::PostCreateSteps {
            steps,
            trust_required,
            ..
        } => {
            let step_lines =
                kagi_domain::plan_note::worktree::worktree_steps_lines(steps, false, "");
            let msg = if *trust_required {
                Msg::AdviceWorktreePostCreateStepsTrustRequired
            } else {
                Msg::AdviceWorktreePostCreateSteps
            };
            super::advice_text(msg, &[&steps.len(), &step_lines])
        }
        WorktreeNote::PreRemoveSteps {
            steps,
            trust_required,
            ..
        } => {
            let step_lines =
                kagi_domain::plan_note::worktree::worktree_steps_lines(steps, false, "");
            let msg = if *trust_required {
                Msg::AdviceWorktreePreRemoveStepsTrustRequired
            } else {
                Msg::AdviceWorktreePreRemoveSteps
            };
            super::advice_text(msg, &[&steps.len(), &step_lines])
        }
    }
}

/// Japanese rendering of one worktree title.
pub fn title_ja(title: &WorktreeTitle) -> String {
    match title {
        WorktreeTitle::CreateBranchCheckout { name, at } => {
            format!("branch `{}` を {} に作成して checkout", name, at)
        }
        WorktreeTitle::CreateWorktree { branch, start } => {
            format!("worktree `{}` を {} に作成", branch, start)
        }
        WorktreeTitle::UnlockWorktree { name } => format!("worktree `{}` のロック解除", name),
        WorktreeTitle::RemoveWorktree { name } => format!("worktree `{}` の削除", name),
        WorktreeTitle::LockWorktree { name } => format!("worktree `{}` のロック", name),
        WorktreeTitle::PruneWorktrees => "古い worktree の prune".to_string(),
        WorktreeTitle::RepairWorktrees => "worktree リンクの修復".to_string(),
    }
}

/// Japanese rendering of one worktree recovery block.
pub fn recovery_ja(recovery: &WorktreeRecovery) -> String {
    match recovery {
        WorktreeRecovery::CreateBranchCheckout { name, prev } => format!(
            "branch `{}` を作成してから checkout します。失敗しても branch は残る場合があり、削除できます:\n  git branch -d {}\ncheckout 後に元へ戻す:\n  git checkout {}",
            name, name, prev
        ),
        WorktreeRecovery::CreateWorktree { path, branch } => format!(
            "不要ならリンク worktree を削除:\n  git worktree remove {}\nその後 branch を削除:\n  git branch -d {}",
            path, branch
        ),
        WorktreeRecovery::Unlock { name } => format!(
            "必要なら再度ロック:\n  git worktree lock --reason \"<理由>\" <{} のパス>",
            name
        ),
        WorktreeRecovery::RemoveWorktree { path, branch } => match branch {
            Some(b) => format!(
                "必要なら worktree を再作成:\n  git worktree add {} {}",
                path, b
            ),
            None => format!(
                "必要なら worktree を再作成:\n  git worktree add {} <branch-or-commit>",
                path
            ),
        },
        WorktreeRecovery::LockWorktree { name } => format!(
            "必要ならロック解除:\n  git worktree unlock <{} のパス>",
            name
        ),
        WorktreeRecovery::Prune => {
            "prune は消えた作業ディレクトリの管理エントリだけを削除します。\
             必要な worktree は再作成:\n  git worktree add <path> <branch>"
                .to_string()
        }
        WorktreeRecovery::Repair => {
            "repair は冪等で .git のリンクだけを修復します。まだ誤っていれば main worktree から再実行:\n  git worktree repair [<path>...]"
                .to_string()
        }
    }
}
