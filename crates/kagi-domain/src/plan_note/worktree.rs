//! WorktreeNote — ADR-0129 appendix §B-8 (create-branch+checkout /
//! create-worktree / unlock-worktree).
//!
//! `plan_create_worktree_impl`'s `validate_worktree_path` blocker is kept as
//! `PlanNote::Verbatim(msg)` — it is ALREADY keyed/localizable via
//! `kagi-domain::plan::{WorktreePathError, WorktreeValidationError}::Display`
//! (§E) and the `localize_plan_blockers` shim in `src/ui/mod.rs` (deleted only
//! in Phase 3), so it is not redefined here. Likewise the branch-missing
//! blocker in `plan_create_worktree_impl`'s existing-branch path is the
//! cross-op `PlanNote::Common(CommonNote::BranchMissing { in_repo: true, .. })`
//! (§A14) — not redefined here.
//!
//! `plan_create_branch_with_checkout`'s conflicted/dirty-working-tree
//! blockers are their own dedicated sentences (verified byte-for-byte against
//! the current `ops/worktree.rs` source, appendix §B-8 row 2): the dirty one
//! reads "…checkout after branch creation could lose work. Stash changes
//! before continuing." which is NOT the cross-op `CommonNote::DirtyBlocksOp`
//! sentence ("…stash or commit changes before {op}.") — so it stays a
//! dedicated `DirtyBlocksCheckoutAfterCreate` variant here rather than being
//! folded into `CommonNote`. Its conflicted-files blocker IS the cross-op
//! template and reuses `CommonNote::ConflictedFiles { before:
//! OpPhrase::CheckingOutTheNewBranch }`; its untracked-files warning reuses
//! `CommonNote::UntrackedRemain { ctx: UntrackedCtx::AfterSwitchingBranches }`.

/// Plan notes for the worktree op family (create-branch+checkout,
/// create-worktree, unlock-worktree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeNote {
    /// blocker (`plan_create_branch_with_checkout`) — the working tree has
    /// staged/modified changes that the post-create checkout could clobber.
    DirtyBlocksCheckoutAfterCreate { parts: super::common::DirtyParts },
    /// blocker (`plan_create_worktree_impl`) — the branch is already checked
    /// out in a different worktree.
    BranchInOtherWorktree { branch: String, path: String },
    /// warning (`plan_create_worktree_impl`) — describes the linked worktree
    /// that will be created.
    CreatesLinkedWorktree {
        path: String,
        branch: String,
        start: String,
    },
    /// warning (`plan_unlock_worktree`) — the worktree is locked; surfaces the
    /// recorded reason (or notes that none was recorded).
    LockedWithReason { reason: Option<String> },
    /// blocker (`plan_unlock_worktree`) — the worktree is already unlocked
    /// (no-op family).
    AlreadyUnlocked { name: String },
    /// blocker (`plan_unlock_worktree`) — the lock state could not be read.
    LockStateUnreadable { name: String, err: String },
    /// blocker (`plan_unlock_worktree`) — the named worktree does not exist.
    WorktreeMissing { name: String },
}

impl WorktreeNote {
    /// Byte-identical to the legacy `ops/worktree.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeNote::DirtyBlocksCheckoutAfterCreate { parts } => format!(
                "Working tree has {} — checkout after branch creation could lose work. Stash changes before continuing.",
                parts.parts_en()
            ),
            WorktreeNote::BranchInOtherWorktree { branch, path } => format!(
                "Branch '{}' is already checked out in another worktree: {}",
                branch, path
            ),
            WorktreeNote::CreatesLinkedWorktree {
                path,
                branch,
                start,
            } => format!(
                "Creates a linked worktree at '{}' with branch '{}' (start point {}).",
                path, branch, start
            ),
            WorktreeNote::LockedWithReason { reason } => {
                let reason_display = match reason {
                    Some(r) => format!("\"{}\"", r),
                    None => "(no reason recorded)".to_string(),
                };
                format!(
                    "Locked with reason: {} — a lock is deliberate protection someone \
                     placed on this worktree. Make sure it is no longer needed.",
                    reason_display
                )
            }
            WorktreeNote::AlreadyUnlocked { name } => {
                format!("Worktree '{}' is already unlocked.", name)
            }
            WorktreeNote::LockStateUnreadable { name, err } => format!(
                "Could not read the lock state of worktree '{}': {}",
                name, err
            ),
            WorktreeNote::WorktreeMissing { name } => {
                format!("Worktree '{}' does not exist.", name)
            }
        }
    }
}

/// Plan titles for the worktree op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeTitle {
    /// `plan_create_branch_with_checkout` — `Create branch '<name>' @ <at> and
    /// checkout` (overrides the plain `BranchTitle::CreateBranch` title that
    /// `plan_create_branch` set).
    CreateBranchCheckout { name: String, at: String },
    /// `plan_create_worktree_impl` — `Create worktree '<branch>' @ <start>`.
    CreateWorktree { branch: String, start: String },
    /// `plan_unlock_worktree` — `Unlock worktree '<name>'`.
    UnlockWorktree { name: String },
}

impl WorktreeTitle {
    /// Byte-identical to the legacy strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeTitle::CreateBranchCheckout { name, at } => {
                format!("Create branch '{}' @ {} and checkout", name, at)
            }
            WorktreeTitle::CreateWorktree { branch, start } => {
                format!("Create worktree '{}' @ {}", branch, start)
            }
            WorktreeTitle::UnlockWorktree { name } => format!("Unlock worktree '{}'", name),
        }
    }
}

/// Recovery kinds for the worktree op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeRecovery {
    /// `plan_create_branch_with_checkout` — remove the branch / switch back.
    CreateBranchCheckout { name: String, prev: String },
    /// `plan_create_worktree_impl` — remove the worktree / branch.
    CreateWorktree { path: String, branch: String },
    /// `plan_unlock_worktree` — re-lock if needed.
    Unlock { name: String },
}

impl WorktreeRecovery {
    /// Byte-identical to the legacy strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            WorktreeRecovery::CreateBranchCheckout { name, prev } => format!(
                "This creates branch '{}' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d {}\nTo return after checkout:\n  git checkout {}",
                name, name, prev
            ),
            WorktreeRecovery::CreateWorktree { path, branch } => format!(
                "Remove the linked worktree if needed:\n  git worktree remove {}\nThe branch can then be removed with:\n  git branch -d {}",
                path, branch
            ),
            WorktreeRecovery::Unlock { name } => format!(
                "Re-lock the worktree if needed:\n  git worktree lock --reason \"<why>\" <path-of-{}>",
                name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_note::common::DirtyParts;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, quotes, and
    //    paths must render byte-identically to the legacy producer strings. ──

    #[test]
    fn dirty_blocks_checkout_after_create_staged_and_modified() {
        assert_eq!(
            WorktreeNote::DirtyBlocksCheckoutAfterCreate {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                }
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified — checkout after branch creation could lose work. Stash changes before continuing."
        );
        assert_eq!(
            WorktreeNote::DirtyBlocksCheckoutAfterCreate {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                }
            }
            .message_en(),
            "Working tree has 3 modified — checkout after branch creation could lose work. Stash changes before continuing."
        );
    }

    #[test]
    fn branch_in_other_worktree() {
        assert_eq!(
            WorktreeNote::BranchInOtherWorktree {
                branch: "feat/x".into(),
                path: "/repo/../wt one".into()
            }
            .message_en(),
            "Branch 'feat/x' is already checked out in another worktree: /repo/../wt one"
        );
    }

    #[test]
    fn creates_linked_worktree() {
        assert_eq!(
            WorktreeNote::CreatesLinkedWorktree {
                path: "/repo/../wt".into(),
                branch: "feat/x".into(),
                start: "a1b2c3d4".into()
            }
            .message_en(),
            "Creates a linked worktree at '/repo/../wt' with branch 'feat/x' (start point a1b2c3d4)."
        );
    }

    #[test]
    fn locked_with_reason_some_and_none() {
        assert_eq!(
            WorktreeNote::LockedWithReason {
                reason: Some("agent still running".into())
            }
            .message_en(),
            "Locked with reason: \"agent still running\" — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed."
        );
        assert_eq!(
            WorktreeNote::LockedWithReason { reason: None }.message_en(),
            "Locked with reason: (no reason recorded) — a lock is deliberate protection someone placed on this worktree. Make sure it is no longer needed."
        );
    }

    #[test]
    fn already_unlocked() {
        assert_eq!(
            WorktreeNote::AlreadyUnlocked {
                name: "wt-free".into()
            }
            .message_en(),
            "Worktree 'wt-free' is already unlocked."
        );
    }

    #[test]
    fn lock_state_unreadable() {
        assert_eq!(
            WorktreeNote::LockStateUnreadable {
                name: "wt-x".into(),
                err: "corrupt lock file".into()
            }
            .message_en(),
            "Could not read the lock state of worktree 'wt-x': corrupt lock file"
        );
    }

    #[test]
    fn worktree_missing() {
        assert_eq!(
            WorktreeNote::WorktreeMissing {
                name: "no-such".into()
            }
            .message_en(),
            "Worktree 'no-such' does not exist."
        );
    }

    #[test]
    fn create_branch_checkout_title() {
        assert_eq!(
            WorktreeTitle::CreateBranchCheckout {
                name: "feat/x".into(),
                at: "a1b2c3d4".into()
            }
            .message_en(),
            "Create branch 'feat/x' @ a1b2c3d4 and checkout"
        );
    }

    #[test]
    fn create_worktree_title() {
        assert_eq!(
            WorktreeTitle::CreateWorktree {
                branch: "feat/x".into(),
                start: "a1b2c3d4".into()
            }
            .message_en(),
            "Create worktree 'feat/x' @ a1b2c3d4"
        );
    }

    #[test]
    fn unlock_worktree_title() {
        assert_eq!(
            WorktreeTitle::UnlockWorktree {
                name: "wt-x".into()
            }
            .message_en(),
            "Unlock worktree 'wt-x'"
        );
    }

    #[test]
    fn create_branch_checkout_recovery() {
        assert_eq!(
            WorktreeRecovery::CreateBranchCheckout {
                name: "feat/x".into(),
                prev: "main".into()
            }
            .message_en(),
            "This creates branch 'feat/x' and then checks it out. If checkout fails, the branch may still exist and can be removed with:\n  git branch -d feat/x\nTo return after checkout:\n  git checkout main"
        );
    }

    #[test]
    fn create_worktree_recovery() {
        assert_eq!(
            WorktreeRecovery::CreateWorktree {
                path: "/repo/../wt".into(),
                branch: "feat/x".into()
            }
            .message_en(),
            "Remove the linked worktree if needed:\n  git worktree remove /repo/../wt\nThe branch can then be removed with:\n  git branch -d feat/x"
        );
    }

    #[test]
    fn unlock_recovery() {
        assert_eq!(
            WorktreeRecovery::Unlock {
                name: "wt-x".into()
            }
            .message_en(),
            "Re-lock the worktree if needed:\n  git worktree lock --reason \"<why>\" <path-of-wt-x>"
        );
    }
}
