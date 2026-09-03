//! JA strings for `WorktreeNote` (ADR-0129 appendix §B-8 — create-branch+
//! checkout / create-worktree / unlock-worktree).

use kagi_domain::plan_note::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

/// Japanese rendering of one worktree note.
pub fn note_ja(note: &WorktreeNote) -> String {
    match note {
        WorktreeNote::DirtyBlocksCheckoutAfterCreate { parts } => format!(
            "作業ツリーに {} があります — branch 作成後の checkout で変更が失われる可能性があります。続行する前に変更を stash してください。",
            parts.parts_en()
        ),
        WorktreeNote::BranchInOtherWorktree { branch, path } => format!(
            "branch '{}' は別の worktree '{}' で既に checkout されています。",
            branch, path
        ),
        WorktreeNote::CreatesLinkedWorktree {
            path,
            branch,
            start,
        } => format!(
            "'{}' に branch '{}' で(開始点 {})リンク worktree を作成します。",
            path, branch, start
        ),
        WorktreeNote::LockedWithReason { reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            format!(
                "ロック理由: {} — ロックはこの worktree に誰かが意図的に設定した保護です。もう不要であることを確認してください。",
                reason_display
            )
        }
        WorktreeNote::AlreadyUnlocked { name } => {
            format!("worktree '{}' は既にロック解除されています。", name)
        }
        WorktreeNote::LockStateUnreadable { name, err } => format!(
            "worktree '{}' のロック状態を読み取れませんでした: {}",
            name, err
        ),
        WorktreeNote::WorktreeMissing { name } => {
            format!("worktree '{}' は存在しません。", name)
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
            format!(
                ".worktreeinclude に一致する {} 個のファイル ({}) を新しい worktree にコピーします: {}。",
                count,
                kagi_domain::worktree_include::human_bytes(*total_bytes),
                names
            )
        }
        WorktreeNote::IncludeSkippedSymlinks { count } => {
            format!("一致した symlink {} 個はスキップします(symlink はコピーしません)。", count)
        }
        WorktreeNote::IncludeOverCap {
            total_bytes,
            cap_bytes,
        } => format!(
            ".worktreeinclude の一致は {} で、コピー上限 {} を超えています — コピーは続行しますが大きくなる可能性があります(例: node_modules の一致)。",
            kagi_domain::worktree_include::human_bytes(*total_bytes),
            kagi_domain::worktree_include::human_bytes(*cap_bytes)
        ),
        WorktreeNote::RemoveMainRefused => {
            "これは main worktree です — 削除できません。".to_string()
        }
        WorktreeNote::RemoveDirty { path, summary } => format!(
            "worktree '{}' に未コミットの変更があります ({}) — 先に commit か stash してください(削除は force しません)。",
            path, summary
        ),
        WorktreeNote::RemoveLocked { path, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            format!(
                "worktree '{}' はロックされています ({}) — 削除する前にロックを解除してください(kagi は force しません)。",
                path, reason_display
            )
        }
        WorktreeNote::RemovesWorktree {
            path,
            branch,
            delete_branch,
        } => {
            let branch_display = branch.as_deref().unwrap_or("(detached HEAD)");
            if *delete_branch {
                format!(
                    "'{}' のリンク worktree を削除し、branch '{}' も削除します。",
                    path, branch_display
                )
            } else {
                format!(
                    "'{}' のリンク worktree を削除します — branch '{}' は残します。",
                    path, branch_display
                )
            }
        }
        WorktreeNote::LocksWorktree { path, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由なし)".to_string(),
            };
            format!("'{}' の worktree をロックします。理由: {}。", path, reason_display)
        }
        WorktreeNote::AlreadyLocked { name, reason } => {
            let reason_display = match reason {
                Some(r) => format!("「{}」", r),
                None => "(理由の記録なし)".to_string(),
            };
            format!("worktree '{}' は既にロックされています ({})。", name, reason_display)
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
            format!(
                "作業ディレクトリが消えた古い worktree 管理エントリ {} 件を prune します: {}。",
                count, names
            )
        }
        WorktreeNote::PruneNothing => "prune 対象の worktree はありません。".to_string(),
        WorktreeNote::RepairsWorktrees => {
            "worktree の管理リンクを修復します: main の移動、linked の移動、またはその両方に対処します。\
             ファイルには一切触れず、.git のリンクのみを修復します。"
                .to_string()
        }
    }
}

/// Japanese rendering of one worktree title.
pub fn title_ja(title: &WorktreeTitle) -> String {
    match title {
        WorktreeTitle::CreateBranchCheckout { name, at } => {
            format!("branch '{}' を {} に作成して checkout", name, at)
        }
        WorktreeTitle::CreateWorktree { branch, start } => {
            format!("worktree '{}' を {} に作成", branch, start)
        }
        WorktreeTitle::UnlockWorktree { name } => format!("worktree '{}' のロック解除", name),
        WorktreeTitle::RemoveWorktree { name } => format!("worktree '{}' の削除", name),
        WorktreeTitle::LockWorktree { name } => format!("worktree '{}' のロック", name),
        WorktreeTitle::PruneWorktrees => "古い worktree の prune".to_string(),
        WorktreeTitle::RepairWorktrees => "worktree リンクの修復".to_string(),
    }
}

/// Japanese rendering of one worktree recovery block.
pub fn recovery_ja(recovery: &WorktreeRecovery) -> String {
    match recovery {
        WorktreeRecovery::CreateBranchCheckout { name, prev } => format!(
            "branch '{}' を作成してから checkout します。checkout が失敗しても branch は残っている可能性があり、次のコマンドで削除できます:\n  git branch -d {}\ncheckout 後に元に戻すには:\n  git checkout {}",
            name, name, prev
        ),
        WorktreeRecovery::CreateWorktree { path, branch } => format!(
            "必要であればリンク worktree を削除してください:\n  git worktree remove {}\nその後 branch を削除できます:\n  git branch -d {}",
            path, branch
        ),
        WorktreeRecovery::Unlock { name } => format!(
            "必要であれば worktree を再度ロックしてください:\n  git worktree lock --reason \"<理由>\" <{} のパス>",
            name
        ),
        WorktreeRecovery::RemoveWorktree { path, branch } => match branch {
            Some(b) => format!(
                "必要であれば worktree を再作成してください:\n  git worktree add {} {}",
                path, b
            ),
            None => format!(
                "必要であれば worktree を再作成してください:\n  git worktree add {} <branch-or-commit>",
                path
            ),
        },
        WorktreeRecovery::LockWorktree { name } => format!(
            "必要であれば worktree のロックを解除してください:\n  git worktree unlock <{} のパス>",
            name
        ),
        WorktreeRecovery::Prune => {
            "prune は作業ディレクトリが既に消えた古い管理エントリのみを削除します。\
             必要な worktree は次で再作成してください:\n  git worktree add <path> <branch>"
                .to_string()
        }
        WorktreeRecovery::Repair => {
            "repair は冪等で .git のリンクのみを修復します。まだリンクが誤っている場合は main worktree から再実行してください:\n  git worktree repair [<path>...]"
                .to_string()
        }
    }
}
