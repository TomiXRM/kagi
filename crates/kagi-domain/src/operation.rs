//! Unified write-operation request and outcome domain models.
//!
//! These types are pure data. They intentionally contain no git2 or UI types so
//! the operation pipeline boundary can be exercised without opening a repo.

use crate::{
    commit::CommitId,
    plan::{
        AmendMode, AmendOutcome, DiscardOutcome, PullOutcome, PushOutcome, RebaseOutcome,
        UndoOutcome,
    },
};

/// A write operation request handled by the git backend pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Commit {
        message: String,
    },
    /// Finalize an in-progress merge after conflicts are resolved
    /// (`git commit` with `MERGE_HEAD` present). Distinct from `Commit`
    /// because it creates a 2-parent merge commit and has no separate plan
    /// (the conflict-resolution save IS the plan).
    MergeCommit {
        message: String,
    },
    Checkout {
        branch: String,
    },
    CheckoutCommit {
        id: CommitId,
    },
    CreateBranch {
        name: String,
        at: CommitId,
    },
    CreateBranchWithCheckout {
        name: String,
        at: CommitId,
        checkout_after: bool,
    },
    CreateTag {
        name: String,
        at: CommitId,
    },
    CreateWorktree {
        branch: String,
        path: String,
        start: CommitId,
    },
    OpenWorktreeForBranch {
        branch: String,
        path: String,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply {
        index: usize,
    },
    StashPop {
        index: usize,
    },
    CherryPick {
        id: CommitId,
    },
    MergeBranch {
        target: String,
    },
    MergeIntoConflict {
        target: String,
    },
    CheckoutTrackingBranch {
        remote_branch: String,
        local_branch: String,
    },
    /// ADR-0101: fetch the remote, switch to `branch_name`, and fast-forward it
    /// to `remote_branch` when safe. Creates the local branch if missing.
    SwitchToLatestBranch {
        branch_name: String,
        remote_branch: String,
    },
    Revert {
        id: CommitId,
    },
    Pull,
    Push,
    PullBranchFf {
        branch_name: String,
    },
    PushBranch {
        branch_name: String,
        set_upstream: bool,
    },
    SetUpstream {
        branch_name: String,
        upstream: String,
    },
    RenameBranch {
        old_name: String,
        new_name: String,
    },
    UndoCommit,
    Amend {
        mode: AmendMode,
        message: Option<String>,
    },
    DeleteBranch {
        name: String,
    },
    DeleteRemoteBranch {
        remote_branch: String,
    },
    ResetCurrentToHead {
        target: CommitId,
    },
    ForceWithLeasePush,
    RebaseCurrentOnto {
        onto: String,
    },
    Discard {
        paths: Vec<String>,
    },
}

/// The successful result of executing an [`Operation`].
#[derive(Debug, Clone)]
pub enum OperationOutcome {
    Commit(CommitId),
    Pull(PullOutcome),
    Push(PushOutcome),
    Undo(UndoOutcome),
    Amend(AmendOutcome),
    Discard(DiscardOutcome),
    MergeIntoConflict(Vec<String>),
    Rebase(RebaseOutcome),
    Unit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_fields_are_accessible_without_repo() {
        let op = Operation::CreateBranch {
            name: "feature/domain-op".to_string(),
            at: CommitId("0123456789abcdef0123456789abcdef01234567".to_string()),
        };

        match op {
            Operation::CreateBranch { name, at } => {
                assert_eq!(name, "feature/domain-op");
                assert_eq!(at.short(), "01234567");
            }
            _ => panic!("unexpected operation variant"),
        }
    }

    #[test]
    fn operation_can_carry_collection_inputs() {
        let op = Operation::Discard {
            paths: vec!["src/lib.rs".to_string(), "README.md".to_string()],
        };

        match op {
            Operation::Discard { paths } => {
                assert_eq!(paths.len(), 2);
                assert_eq!(paths[0], "src/lib.rs");
            }
            _ => panic!("unexpected operation variant"),
        }
    }
}
