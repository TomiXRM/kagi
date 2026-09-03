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
    }
}
