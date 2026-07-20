//! BranchNote — ADR-0129 appendix §B-9 (create / rename / delete branch).
//!
//! `create-branch`'s branch-name-validity blockers (`BranchNameError`, §E) and
//! `rename-branch`'s name-validity blockers (also `BranchNameError`) are
//! `PlanNote::Common(CommonNote::BranchNameErrorKeyed(..))` — not redefined
//! here, since `CommonNote` already covers every keyed `BranchNameError`
//! variant and localizes via `kagi_ui_core::i18n::branch_name_error`.
//! `BranchNote` covers every OTHER branch-op template: the commit-missing
//! blocker (create), and the full delete/rename set.
//!
//! `Branch '{}' does not exist(.| in this repository.)` (create's commit-exists
//! check has no such template; delete/rename's missing-branch blockers) is a
//! cross-op template (§A14/A15) and stays `PlanNote::Common(CommonNote::BranchMissing)`
//! — not redefined here.

/// Plan notes for the branch op family (create / rename / delete).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchNote {
    /// blocker (`plan_create_branch`) — the target commit does not exist.
    CommitMissing { sha: String },
    /// warning (`plan_rename_branch`) — rename is ref-only; dirty WT is
    /// untouched by it.
    RenameRefOnlyDirty,
    /// warning (`plan_rename_branch`, unconditional) — the remote branch name
    /// is not renamed automatically.
    RenameRemoteNotRenamed,
    /// blocker (`plan_delete_branch`) — the branch is the current HEAD branch.
    DeleteCurrentBranch { name: String },
    /// blocker (`plan_delete_branch`) — a LOCKED linked worktree has the
    /// branch checked out.
    DeleteBranchInLockedWorktree { name: String, path: String },
    /// blocker (`plan_delete_branch`) — a dirty linked worktree has the branch
    /// checked out.
    DeleteBranchInDirtyWorktree { name: String, path: String },
    /// warning (`plan_delete_branch`) — a CLEAN linked worktree has the branch
    /// checked out; it will be removed, then the branch deleted (ADR-0129 F-3:
    /// the UI matches on this variant rather than substring-searching the
    /// rendered warning text).
    DeleteRemovesPinningWorktree { name: String, path: String },
    /// blocker (`plan_delete_branch`) — HEAD is detached at the branch's tip.
    DeleteDetachedAtTip { name: String },
    /// blocker (`plan_delete_branch`) — the branch has unmerged commits.
    DeleteUnmerged { name: String, tip: String },
    /// warning (`plan_delete_branch`) — the branch has an upstream that is not
    /// deleted by this operation.
    DeleteKeepsRemote { name: String },
}

impl BranchNote {
    /// Sole English renderer (byte-identical to the legacy producer strings).
    pub fn message_en(&self) -> String {
        match self {
            BranchNote::CommitMissing { sha } => {
                format!("Commit '{}' does not exist in this repository.", sha)
            }
            BranchNote::RenameRefOnlyDirty => {
                "Working tree is dirty; branch rename is ref-only and will not touch files."
                    .to_string()
            }
            BranchNote::RenameRemoteNotRenamed => {
                "Remote branch names are not renamed automatically; only local branch config is carried over.".to_string()
            }
            BranchNote::DeleteCurrentBranch { name } => format!(
                "Branch '{}' is the currently checked-out branch. Checkout a different branch before deleting this one.",
                name
            ),
            BranchNote::DeleteBranchInLockedWorktree { name, path } => format!(
                "Branch '{}' is checked out in LOCKED worktree '{}'. Unlock it first (right-click the worktree in the sidebar \u{2192} Unlock worktree) before deleting the branch.",
                name, path
            ),
            BranchNote::DeleteBranchInDirtyWorktree { name, path } => format!(
                "Branch '{}' is checked out in worktree '{}' which has uncommitted changes. Commit or discard them there first — the worktree is not removed while it holds work.",
                name, path
            ),
            BranchNote::DeleteRemovesPinningWorktree { name, path } => format!(
                "Branch '{}' is checked out in clean worktree '{}'. The worktree will be removed, then the branch deleted.",
                name, path
            ),
            BranchNote::DeleteDetachedAtTip { name } => format!(
                "HEAD is detached and points to the same commit as '{}'. This branch cannot be deleted while HEAD is at its tip.",
                name
            ),
            BranchNote::DeleteUnmerged { name, tip } => format!(
                "Branch '{}' has unmerged commits (tip {} is not reachable from HEAD). Merge or discard the branch manually before deleting. Force delete is not provided.",
                name, tip
            ),
            BranchNote::DeleteKeepsRemote { name } => format!(
                "Branch '{}' has an upstream tracking branch. Only the local branch will be deleted; the remote branch is NOT removed.",
                name
            ),
        }
    }
}

/// Plan titles for the branch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchTitle {
    /// `plan_create_branch` / `plan_create_branch_with_checkout` —
    /// `Create branch '<name>' @ <at>` (+ ` and checkout` when `checkout`).
    CreateBranch {
        name: String,
        at: String,
        checkout: bool,
    },
    /// `plan_rename_branch` — `Rename branch '<old>' to '<new>'`.
    RenameBranch { old: String, new: String },
    /// `plan_delete_branch` — `Delete branch '<name>' (tip <tip>)` when the
    /// branch was found, `Delete branch '<name>'` when it was not.
    DeleteBranch { name: String, tip: Option<String> },
}

impl BranchTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            BranchTitle::CreateBranch { name, at, checkout } => {
                if *checkout {
                    format!("Create branch '{}' @ {} and checkout", name, at)
                } else {
                    format!("Create branch '{}' @ {}", name, at)
                }
            }
            BranchTitle::RenameBranch { old, new } => {
                format!("Rename branch '{}' to '{}'", old, new)
            }
            BranchTitle::DeleteBranch {
                name,
                tip: Some(tip),
            } => {
                format!("Delete branch '{}' (tip {})", name, tip)
            }
            BranchTitle::DeleteBranch { name, tip: None } => format!("Delete branch '{}'", name),
        }
    }
}

/// Recovery kinds for the branch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchRecovery {
    /// `plan_create_branch` — the new branch can simply be `git branch -d`'d.
    CreateBranch { name: String },
    /// `plan_rename_branch` — undo with `git branch -m <new> <old>`.
    RenameBranch { old: String, new: String },
    /// `plan_delete_branch` — restore with `git branch <name> <tip>`, or (when
    /// the branch was never found) an explanatory no-op message.
    DeleteBranch { name: String, tip: Option<String> },
}

impl BranchRecovery {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            BranchRecovery::CreateBranch { name } => format!(
                "The new branch '{}' can be removed without side effects:\n  git branch -d {}\n(Branch creation does not move HEAD or alter the working tree.)",
                name, name
            ),
            BranchRecovery::RenameBranch { old, new } => format!(
                "This renames only the local ref. To undo: git branch -m {} {}",
                new, old
            ),
            BranchRecovery::DeleteBranch { name, tip: Some(tip) } => format!(
                "To restore the deleted branch:\n  git branch {} {}\nThe branch tip commit '{}' remains in the object store until GC.",
                name, tip, tip
            ),
            BranchRecovery::DeleteBranch { name, tip: None } => format!(
                "Branch '{}' could not be found. Use `git branch` to list local branches.",
                name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, quotes, and
    //    paths must render byte-identically to the legacy producer strings. ──

    #[test]
    fn commit_missing() {
        assert_eq!(
            BranchNote::CommitMissing {
                sha: "a1b2c3d4".into()
            }
            .message_en(),
            "Commit 'a1b2c3d4' does not exist in this repository."
        );
    }

    #[test]
    fn rename_ref_only_dirty() {
        assert_eq!(
            BranchNote::RenameRefOnlyDirty.message_en(),
            "Working tree is dirty; branch rename is ref-only and will not touch files."
        );
    }

    #[test]
    fn rename_remote_not_renamed() {
        assert_eq!(
            BranchNote::RenameRemoteNotRenamed.message_en(),
            "Remote branch names are not renamed automatically; only local branch config is carried over."
        );
    }

    #[test]
    fn delete_current_branch() {
        assert_eq!(
            BranchNote::DeleteCurrentBranch {
                name: "main".into()
            }
            .message_en(),
            "Branch 'main' is the currently checked-out branch. Checkout a different branch before deleting this one."
        );
    }

    #[test]
    fn delete_branch_in_locked_worktree() {
        assert_eq!(
            BranchNote::DeleteBranchInLockedWorktree {
                name: "feat/x".into(),
                path: "/repo/../wt one".into()
            }
            .message_en(),
            "Branch 'feat/x' is checked out in LOCKED worktree '/repo/../wt one'. Unlock it first (right-click the worktree in the sidebar \u{2192} Unlock worktree) before deleting the branch."
        );
    }

    #[test]
    fn delete_branch_in_dirty_worktree() {
        assert_eq!(
            BranchNote::DeleteBranchInDirtyWorktree {
                name: "feat/x".into(),
                path: "/repo/wt".into()
            }
            .message_en(),
            "Branch 'feat/x' is checked out in worktree '/repo/wt' which has uncommitted changes. Commit or discard them there first — the worktree is not removed while it holds work."
        );
    }

    #[test]
    fn delete_removes_pinning_worktree() {
        assert_eq!(
            BranchNote::DeleteRemovesPinningWorktree {
                name: "feat/x".into(),
                path: "/repo/wt".into()
            }
            .message_en(),
            "Branch 'feat/x' is checked out in clean worktree '/repo/wt'. The worktree will be removed, then the branch deleted."
        );
    }

    #[test]
    fn delete_detached_at_tip() {
        assert_eq!(
            BranchNote::DeleteDetachedAtTip {
                name: "feat/x".into()
            }
            .message_en(),
            "HEAD is detached and points to the same commit as 'feat/x'. This branch cannot be deleted while HEAD is at its tip."
        );
    }

    #[test]
    fn delete_unmerged() {
        assert_eq!(
            BranchNote::DeleteUnmerged {
                name: "feat/x".into(),
                tip: "a1b2c3d4".into()
            }
            .message_en(),
            "Branch 'feat/x' has unmerged commits (tip a1b2c3d4 is not reachable from HEAD). Merge or discard the branch manually before deleting. Force delete is not provided."
        );
    }

    #[test]
    fn delete_keeps_remote() {
        assert_eq!(
            BranchNote::DeleteKeepsRemote {
                name: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' has an upstream tracking branch. Only the local branch will be deleted; the remote branch is NOT removed."
        );
    }

    #[test]
    fn create_branch_title_plain_and_checkout() {
        assert_eq!(
            BranchTitle::CreateBranch {
                name: "feat/x".into(),
                at: "a1b2c3d4".into(),
                checkout: false
            }
            .message_en(),
            "Create branch 'feat/x' @ a1b2c3d4"
        );
        assert_eq!(
            BranchTitle::CreateBranch {
                name: "feat/x".into(),
                at: "a1b2c3d4".into(),
                checkout: true
            }
            .message_en(),
            "Create branch 'feat/x' @ a1b2c3d4 and checkout"
        );
    }

    #[test]
    fn rename_branch_title() {
        assert_eq!(
            BranchTitle::RenameBranch {
                old: "old-name".into(),
                new: "new-name".into()
            }
            .message_en(),
            "Rename branch 'old-name' to 'new-name'"
        );
    }

    #[test]
    fn delete_branch_title_found_and_missing() {
        assert_eq!(
            BranchTitle::DeleteBranch {
                name: "feat/x".into(),
                tip: Some("a1b2c3d4".into())
            }
            .message_en(),
            "Delete branch 'feat/x' (tip a1b2c3d4)"
        );
        assert_eq!(
            BranchTitle::DeleteBranch {
                name: "feat/x".into(),
                tip: None
            }
            .message_en(),
            "Delete branch 'feat/x'"
        );
    }

    #[test]
    fn create_branch_recovery() {
        assert_eq!(
            BranchRecovery::CreateBranch {
                name: "feat/x".into()
            }
            .message_en(),
            "The new branch 'feat/x' can be removed without side effects:\n  git branch -d feat/x\n(Branch creation does not move HEAD or alter the working tree.)"
        );
    }

    #[test]
    fn rename_branch_recovery() {
        assert_eq!(
            BranchRecovery::RenameBranch {
                old: "old-name".into(),
                new: "new-name".into()
            }
            .message_en(),
            "This renames only the local ref. To undo: git branch -m new-name old-name"
        );
    }

    #[test]
    fn delete_branch_recovery_found_and_missing() {
        assert_eq!(
            BranchRecovery::DeleteBranch {
                name: "feat/x".into(),
                tip: Some("a1b2c3d4".into())
            }
            .message_en(),
            "To restore the deleted branch:\n  git branch feat/x a1b2c3d4\nThe branch tip commit 'a1b2c3d4' remains in the object store until GC."
        );
        assert_eq!(
            BranchRecovery::DeleteBranch {
                name: "feat/x".into(),
                tip: None
            }
            .message_en(),
            "Branch 'feat/x' could not be found. Use `git branch` to list local branches."
        );
    }
}
