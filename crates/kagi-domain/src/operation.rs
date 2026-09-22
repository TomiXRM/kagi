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
    StashDrop {
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
            Operation::StashDrop { .. } => "stash-drop",
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

/// What became of the local head branch a PR merge promised to delete (#705).
///
/// None of these is a failure of the merge itself. [`Self::Kept`] is part of a
/// **successful** receipt — the approval already said the branch stays, or
/// GitHub queued the merge so there is nothing to clean up yet — while
/// [`Self::NotDeleted`] is the merge landing without the promised deletion,
/// which the receipt reports as partial rather than as a merge to retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrMergeLocalOutcome {
    /// The frozen branch was deleted; `tip` is the full OID it held, retained
    /// by the delete executor's recovery ref.
    Deleted { name: String, tip: String },
    /// The branch was already gone locally — the promise was fulfilled by
    /// absence, and nothing was written.
    Absent { name: String },
    /// The branch was kept because the approval itself kept it, or because
    /// the merge is only queued. Nothing was attempted; nothing failed.
    Kept {
        name: String,
        reason: crate::plan_note::PrMergeLocalReason,
    },
    /// The promised deletion did not happen: the branch or HEAD moved under
    /// the approval, or the delete failed.
    NotDeleted {
        name: String,
        reason: crate::plan_note::PrMergeLocalReason,
    },
}

impl PrMergeLocalOutcome {
    /// One wording source for the durable EN receipt and localized UI notice.
    pub fn note(&self) -> crate::plan_note::GithubNote {
        use crate::plan_note::GithubNote;
        match self {
            Self::Deleted { name, tip } => GithubNote::LocalBranchDeleted {
                name: name.clone(),
                tip: tip.clone(),
            },
            Self::Absent { name } => GithubNote::LocalBranchAbsent { name: name.clone() },
            Self::Kept { name, reason } => GithubNote::LocalBranchKept {
                name: name.clone(),
                reason: reason.clone(),
            },
            Self::NotDeleted { reason, .. } => GithubNote::LocalBranchNotDeleted {
                reason: reason.clone(),
            },
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
    /// The full commit OID of the stash created by a stash push.
    StashPush {
        oid: String,
    },
    StashPop(StashPopOutcome),
    /// The full commit OID of a deleted stash, recoverable with `git stash store`.
    StashDrop {
        oid: String,
    },
    /// A snapshot restore. Carries the id of the savepoint taken of the
    /// pre-restore working tree — the recovery handle (#418). It must reach the
    /// oplog and UI so the overwritten state is recoverable.
    RestoreSnapshot {
        savepoint: String,
    },
    /// A PR review suggestion applied to the working tree (#351).
    Suggestion(SuggestionOutcome),
    /// Branch Cleanup's per-branch results (ADR-0128). Not dispatched through
    /// [`Operation`] — the batch is its own execute — but it rides the same
    /// run-family completion so an unconfirmed delete lands in reconcile.
    BranchCleanup(crate::branch_cleanup::CleanupOutcome),
    /// The accounted result of `gh pr merge` or a queue submission (ADR-0149).
    /// Like Branch Cleanup, a non-`Operation` write that rides the run family.
    /// `confirmed` describes receipt completion, not necessarily PR merge:
    /// a known queued submission or an approved local keep is also true.
    /// It is false for a confirmed merge with a transport error or an unfulfilled
    /// cleanup promise; `number` names the PR that must not be offered again.
    ///
    /// `local_branch` is the local half of a `--delete-branch` merge (#705):
    /// `None` when none was promised, otherwise what actually became of the
    /// frozen branch. A local deletion that failed leaves the merge itself
    /// done, so it is reported here rather than as an `Err` — the merge must
    /// never be retried for it.
    PrMerge {
        number: u64,
        detail: String,
        confirmed: bool,
        local_branch: Option<PrMergeLocalOutcome>,
    },
    /// `gh pr comment`'s receipt for a comment posted to a PR. Like
    /// [`Self::PrMerge`], a non-`Operation` remote write recorded at its own
    /// transport boundary. `detail` is the new comment's URL when gh printed
    /// one — the only handle that identifies what was posted.
    PrComment {
        number: u64,
        detail: String,
    },
    /// GitHub Issue writes record their receipt at the transport boundary.
    IssueCreate {
        detail: String,
    },
    IssueComment {
        number: u64,
        detail: String,
    },
    /// `gh pr review`'s receipt for a review submitted on a PR — the verdict
    /// half of the same remote-write family as [`Self::PrComment`].
    /// `verdict` is [`crate::github::ReviewVerdict::as_str`], so the receipt
    /// names *which* review was submitted; `detail` is gh's own words (the
    /// review's URL when it printed one).
    PrReview {
        number: u64,
        verdict: String,
        detail: String,
    },
    /// `gh pr edit`'s receipt for a reviewer / assignee / label change on a
    /// PR — the metadata half of the same remote-write family as
    /// [`Self::PrComment`]. `detail` is gh's own words (the PR's URL when it
    /// printed one); *what* changed is in the plan the receipt carries, not
    /// re-read from the server.
    PrEdit {
        number: u64,
        detail: String,
    },
    /// Deleted branch tip retained by a mandatory commit recovery ref (#584).
    DeleteBranch {
        name: String,
        tip: String,
        reference: String,
    },
    Unit,
}

/// Observed metadata side effects of branch deletion, even if ref commit fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeleteBranchProgress {
    pub reflog_removed: bool,
}
