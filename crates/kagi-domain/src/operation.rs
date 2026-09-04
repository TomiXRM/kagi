//! Unified write-operation request and outcome domain models.
//!
//! These types are pure data. They intentionally contain no git2 or UI types so
//! the operation pipeline boundary can be exercised without opening a repo.

use crate::{
    commit::CommitId,
    plan::{
        AmendMode, AmendOutcome, DiscardOutcome, PullOutcome, PushOutcome, RebaseOutcome,
        StashPopOutcome, SuggestionOutcome, UndoOutcome,
    },
    suggestion::Suggestion,
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
    /// Publish an existing local tag to a remote. Never forced — see
    /// `ops::tag::plan_push_tag`.
    PushTag {
        name: String,
        remote: String,
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
    /// Merge `source` into `target` without checking `target` out (ADR-0144).
    MergeIntoBranch {
        source: String,
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
    /// Restore the working tree to a saved snapshot (`refs/kagi/snapshots/<id>`,
    /// ADR-0154). Rewrites the working tree through the full safe path; a
    /// pre-restore savepoint snapshot is taken first so the restore is itself
    /// reversible. Never deletes files (no `git clean`).
    RestoreSnapshot {
        id: String,
    },
    /// Apply a GitHub PR review "suggested change" to the working-tree file
    /// (#351, ADR-0172). `expected_original` is the anchored range's content
    /// captured at plan time; execute refuses if the working tree at that range
    /// no longer matches it (TOCTOU stale-line guard). Writes only the working
    /// tree — nothing is staged or committed.
    ApplySuggestion {
        suggestion: Suggestion,
        expected_original: Vec<String>,
    },
}

impl Operation {
    /// Short, stable slug used as the `op` field in the oplog (ADR-0149).
    /// `Backend::run` records this so every write path names the op the same
    /// way, regardless of caller (GUI / MCP / CLI).
    pub fn oplog_name(&self) -> &'static str {
        match self {
            Operation::Commit { .. } => "commit",
            Operation::MergeCommit { .. } => "merge-commit",
            Operation::Checkout { .. } => "checkout",
            Operation::CheckoutCommit { .. } => "checkout-commit",
            Operation::CreateBranch { .. } => "create-branch",
            Operation::CreateBranchWithCheckout { .. } => "create-branch",
            Operation::CreateTag { .. } => "create-tag",
            Operation::PushTag { .. } => "push-tag",
            Operation::CreateWorktree { .. } => "create-worktree",
            Operation::OpenWorktreeForBranch { .. } => "open-worktree",
            Operation::StashPush { .. } => "stash-push",
            Operation::StashApply { .. } => "stash-apply",
            Operation::StashPop { .. } => "stash-pop",
            Operation::CherryPick { .. } => "cherry-pick",
            Operation::MergeBranch { .. } => "merge",
            Operation::MergeIntoConflict { .. } => "merge",
            Operation::MergeIntoBranch { .. } => "merge-into",
            Operation::CheckoutTrackingBranch { .. } => "checkout-tracking",
            Operation::SwitchToLatestBranch { .. } => "switch-to-latest",
            Operation::Revert { .. } => "revert",
            Operation::Pull => "pull",
            Operation::Push => "push",
            Operation::PullBranchFf { .. } => "pull",
            Operation::PushBranch { .. } => "push",
            Operation::SetUpstream { .. } => "set-upstream",
            Operation::RenameBranch { .. } => "rename-branch",
            Operation::UndoCommit => "undo",
            Operation::Amend { .. } => "amend",
            Operation::DeleteBranch { .. } => "delete-branch",
            Operation::DeleteRemoteBranch { .. } => "delete-remote-branch",
            Operation::ResetCurrentToHead { .. } => "reset",
            Operation::ForceWithLeasePush => "force-with-lease-push",
            Operation::RebaseCurrentOnto { .. } => "rebase",
            Operation::Discard { .. } => "discard",
            Operation::RestoreSnapshot { .. } => "restore-snapshot",
            Operation::ApplySuggestion { .. } => "apply-suggestion",
        }
    }
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
    StashPop(StashPopOutcome),
    /// A snapshot restore. Carries the id of the savepoint taken of the
    /// pre-restore working tree — the recovery handle (#418). It must reach the
    /// oplog and UI so the overwritten state is recoverable.
    RestoreSnapshot {
        savepoint: String,
    },
    /// A PR review suggestion applied to the working tree (#351).
    Suggestion(SuggestionOutcome),
    Unit,
}
