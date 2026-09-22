//! GithubNote — PR operations that go through `gh` (2026-08-19).
//!
//! Merging a PR is a remote write, so it takes the same
//! plan → confirm → preflight → execute → oplog path as every local write
//! op. The notes below are what the confirm modal states before the user
//! commits to it.

/// Plan notes for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubNote {
    /// blocker — the L1 list did not provide the immutable head commit needed
    /// by `gh pr merge --match-head-commit`.
    HeadUnavailable { number: u64 },
    /// blocker — GitHub reports the PR as not mergeable (conflicts, or a
    /// branch-protection gate that is not satisfied).
    NotMergeable { number: u64 },
    /// blocker — the PR is a draft; GitHub refuses to merge drafts.
    IsDraft { number: u64 },
    /// warning — CI is red on the head commit.
    ChecksFailing { number: u64, failed: usize },
    /// warning — CI has not finished.
    ChecksPending { number: u64 },
    /// warning — a reviewer asked for changes.
    ChangesRequested { number: u64 },
    /// warning — merging happens on GitHub, so the local clone is untouched
    /// until the next fetch. A plan that also deletes the local head branch
    /// (#705) does write here, and leaves this note out rather than claiming
    /// otherwise.
    RemoteSideEffect,
    /// warning — `--delete-branch` deletes the head branch on the *remote*.
    /// The local branch is a separate, separately-frozen promise kagi keeps
    /// itself ([`Self::DeletesLocalBranch`]).
    DeletesBranch { branch: String },
    /// warning (#705) — the PR comes from a fork, so `gh pr merge` against the
    /// base repository leaves the fork's own head branch alone. Stated rather
    /// than refused: the local half below is still kept as a checkable
    /// promise, and the fork is not kagi's to write to.
    ForkKeepsRemoteBranch,
    /// warning (#705) — after GitHub confirms the merge, kagi deletes the
    /// local head branch through its own guarded delete path. `branch` and
    /// `tip` are frozen at plan time; `tip` is `None` when the branch is
    /// already absent locally, which is a promise about *that* branch — a
    /// same-named branch created later is never deleted.
    DeletesLocalBranch { branch: String, tip: Option<String> },
    /// warning (#705) — plan time already knows the local branch will be
    /// kept, so the plan says so instead of promising a deletion it would
    /// then refuse (#705 review P2: plan and receipt must agree).
    KeepsLocalBranch {
        branch: String,
        reason: PrMergeLocalReason,
    },
    /// receipt (#705) — the frozen local branch was deleted; `tip` is the OID
    /// its recovery ref retains.
    LocalBranchDeleted { name: String, tip: String },
    /// receipt (#705) — the local branch was already gone, so the promise was
    /// kept by absence and nothing was written.
    LocalBranchAbsent { name: String },
    /// receipt (#705) — the approval itself kept the branch: the plan said so
    /// up front, or the merge was only queued. Nothing was attempted and
    /// nothing failed, so this belongs to a **successful** receipt — which is
    /// what separates it from [`Self::LocalBranchNotDeleted`].
    LocalBranchKept {
        name: String,
        reason: PrMergeLocalReason,
    },
    /// receipt (#705) — the merge landed but the promised local deletion did
    /// not happen: the branch or HEAD moved under the approval, or the delete
    /// itself failed. A partial receipt, never a merge to retry.
    LocalBranchNotDeleted { reason: PrMergeLocalReason },
    /// blocker (#351) — the working-tree file the suggestion anchors to is
    /// gone, or the anchored range is out of bounds.
    SuggestionRangeGone { path: String },
    /// blocker (#351, TOCTOU) — the anchored lines in the working tree no
    /// longer match what the suggestion was reviewed against. Applying would
    /// edit the wrong lines, so it is refused.
    SuggestionStale { path: String },
    /// warning (#351) — the suggestion is written to the working tree only;
    /// review it with hunk staging before committing.
    SuggestionWorkingTreeOnly,
    /// blocker — a pull-request comment with no text. `gh pr comment` would
    /// happily post an empty comment; there is nothing to say and nothing to
    /// undo it with but a manual delete, so the plan refuses it.
    CommentBodyEmpty,
    /// blocker — a new Issue has no explicit title and its body has no
    /// meaningful title candidate after Markdown structure is removed.
    IssueTitleEmpty,
    /// blocker — a review that GitHub requires words for (`--request-changes`
    /// or `--comment`) was submitted with none. An approval may be wordless;
    /// these two are refused by the API, so the plan refuses them first
    /// rather than sending a request that cannot succeed. `verdict` is the
    /// [`crate::github::ReviewVerdict`] slug.
    ReviewBodyEmpty { verdict: String },
    /// blocker — a `gh pr edit` with no reviewer, assignee or label change in
    /// it. `gh` would accept the call and change nothing; a round trip and an
    /// oplog receipt for a no-op is worse than refusing it at plan time.
    FieldEditEmpty { number: u64 },
}

impl GithubNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubNote::HeadUnavailable { number } => format!(
                "The head commit for #{} is unavailable. Refresh the pull-request list before merging.",
                number
            ),
            GithubNote::NotMergeable { number } => format!(
                "GitHub reports #{} as not mergeable. Resolve conflicts (or satisfy branch protection) first.",
                number
            ),
            GithubNote::IsDraft { number } => {
                format!("#{} is a draft. Mark it ready for review before merging.", number)
            }
            GithubNote::ChecksFailing { number, failed } => format!(
                "#{} has {} failing check(s). Merging now lands code its CI rejected.",
                number, failed
            ),
            GithubNote::ChecksPending { number } => {
                format!("#{}'s checks have not finished yet.", number)
            }
            GithubNote::ChangesRequested { number } => {
                format!("A reviewer requested changes on #{}.", number)
            }
            GithubNote::RemoteSideEffect => {
                "This merges on GitHub. Your local clone is unchanged until the next fetch.".to_string()
            }
            GithubNote::DeletesBranch { branch } => format!(
                "The head branch '{}' will be deleted on the remote.",
                branch
            ),
            GithubNote::ForkKeepsRemoteBranch => {
                "The remote branch in the fork is not deleted by gh.".to_string()
            }
            GithubNote::DeletesLocalBranch { branch, tip } => match tip {
                Some(tip) => format!(
                    "Once GitHub confirms the merge, the local branch '{}' at {} is deleted here — only if it still points there and is checked out nowhere.",
                    branch, tip
                ),
                None => format!(
                    "The local branch '{}' does not exist here, so nothing local is deleted after the merge.",
                    branch
                ),
            },
            GithubNote::KeepsLocalBranch { branch, reason } => format!(
                "The local branch '{}' is kept, not deleted: {}",
                branch,
                reason.message_en()
            ),
            GithubNote::LocalBranchDeleted { name, tip } => {
                format!("local branch deleted: {}@{}", name, tip)
            }
            GithubNote::LocalBranchAbsent { name } => {
                format!("local branch already absent: {}", name)
            }
            GithubNote::LocalBranchKept { name, reason } => {
                format!("local branch kept: {} ({})", name, reason.message_en())
            }
            GithubNote::LocalBranchNotDeleted { reason } => {
                format!("local branch not deleted: {}", reason.message_en())
            }
            GithubNote::SuggestionRangeGone { path } => format!(
                "The lines '{}' was reviewed at no longer exist. Re-open the review against the current file.",
                path
            ),
            GithubNote::SuggestionStale { path } => format!(
                "'{}' has changed since this suggestion was reviewed. Applying it now could edit the wrong lines, so it is refused. Re-open the review against the current file.",
                path
            ),
            GithubNote::SuggestionWorkingTreeOnly => {
                "This edits the working tree only — nothing is committed. Review it with hunk staging before you commit.".to_string()
            }
            GithubNote::CommentBodyEmpty => {
                "The comment is empty. Write something before posting it.".to_string()
            }
            GithubNote::IssueTitleEmpty => {
                "The issue title is empty. Write a title or add meaningful text to the body."
                    .to_string()
            }
            GithubNote::ReviewBodyEmpty { verdict } => format!(
                "GitHub requires a comment on a '{}' review. Write what you want changed before submitting it.",
                verdict
            ),
            GithubNote::FieldEditEmpty { number } => format!(
                "Nothing on #{} would change. Pick a reviewer, assignee or label to add or remove first.",
                number
            ),
        }
    }
}

/// Why a PR merge's local head branch was **not** deleted (#705 review P3).
///
/// Typed rather than a flattened English sentence: the delete-branch family
/// already refuses in a [`PlanNote`](crate::plan_note::PlanNote) that knows
/// how to render itself in every language (#606), and the reasons kagi itself
/// decides are a closed set. Flattening any of them here would put English
/// inside a Japanese notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrMergeLocalReason {
    /// The delete-branch family refuses this branch, in its own words:
    /// checked out here or in a linked worktree, detached at the tip, … .
    /// Boxed because `PlanNote` contains this type.
    Plan(Box<crate::plan_note::PlanNote>),
    /// The local branch does not point at the PR head that was merged, so
    /// deleting it would drop commits the merge never saw.
    NotAtPrHead { tip: String, head: String },
    /// `gh` accepted the merge and GitHub queued it: a known, successful
    /// submission with nothing merged yet, so there is nothing to clean up.
    Queued,
    /// The branch moved between approval and execution.
    Changed,
    /// HEAD moved between approval and execution.
    HeadChanged,
    /// The approval names a worktree identity that is not the one open here.
    IdentityChanged,
    /// The PR reads as merged, but `gh` reported an error, so the transport
    /// never authorized the local deletion. `mergedAt` says a merge happened;
    /// it does not say the approved head is what landed, and it cannot order
    /// the error against the merge — so the local half is left alone and the
    /// `--match-head-commit` guarantee holds for it too (#705 review).
    DeletionUnauthorized { detail: String },
    /// A failure with no typed shape: the delete family's own error text.
    Detail(String),
}

impl PrMergeLocalReason {
    /// Sole English renderer; a sentence fragment, quoted by the notes above.
    pub fn message_en(&self) -> String {
        match self {
            Self::Plan(note) => note.message_en(),
            Self::NotAtPrHead { tip, head } => format!(
                "the local branch is at {}, not the merged PR head {}",
                tip, head
            ),
            Self::Queued => {
                "GitHub queued the merge, so nothing has been merged yet".to_string()
            }
            Self::Changed => "the branch moved after approval".to_string(),
            Self::HeadChanged => "HEAD moved after approval".to_string(),
            Self::IdentityChanged => {
                "this is not the repository the approval named".to_string()
            }
            Self::DeletionUnauthorized { detail } => format!(
                "gh reported an error, so the transport did not authorize deleting the local branch: {}",
                detail
            ),
            Self::Detail(detail) => detail.clone(),
        }
    }
}

/// Plan titles for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTitle {
    CreateIssue,
    CommentIssue {
        number: u64,
    },
    /// `Merge pull request #<n> (<method>)`.
    MergePr {
        number: u64,
        method: String,
    },
    /// `Apply suggestion to '<path>'` (#351).
    ApplySuggestion {
        path: String,
    },
    /// `Comment on pull request #<n>`.
    CommentPr {
        number: u64,
    },
    /// `Review pull request #<n> (<verdict>)`.
    ReviewPr {
        number: u64,
        verdict: String,
    },
    /// `Edit pull request #<n>`.
    EditPr {
        number: u64,
    },
}

impl GithubTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubTitle::CreateIssue => "Create issue".into(),
            GithubTitle::CommentIssue { number } => format!("Comment on issue #{number}"),
            GithubTitle::MergePr { number, method } => {
                format!("Merge pull request #{} ({})", number, method)
            }
            GithubTitle::ApplySuggestion { path } => {
                format!("Apply suggestion to '{}'", path)
            }
            GithubTitle::CommentPr { number } => {
                format!("Comment on pull request #{}", number)
            }
            GithubTitle::ReviewPr { number, verdict } => {
                format!("Review pull request #{} ({})", number, verdict)
            }
            GithubTitle::EditPr { number } => {
                format!("Edit pull request #{}", number)
            }
        }
    }
}

/// Recovery kinds for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubRecovery {
    /// A merged PR can be reverted on GitHub, or locally with `git revert -m 1`.
    ///
    /// `delete_branch` is the head branch this merge also promises to delete
    /// (`--delete-branch`), frozen here at plan time and `None` when the merge
    /// keeps it. A reconcile confirms the *whole* promise: merging without
    /// deleting is not the operation the user approved (#701). `base_repo` is
    /// the `<host>/<owner>/<repo>` that deletion happens in — a remote *name*
    /// would be a guess, `origin` is not always the PR's base, and without the
    /// host a same-named repository on another host answers instead. Neither
    /// is rendered — the recovery text is about the merge.
    ///
    /// `cross_repository` says the PR's head lives in a fork, where the
    /// remote deletion `delete_branch` names never happens — the reconcile
    /// read must not demand a ref that `gh` was never going to remove (#705).
    /// `local_branch` is the local half of the same promise, frozen so the
    /// deletion kagi performs itself is checkable against the branch that was
    /// actually approved.
    MergePr {
        number: u64,
        base_repo: String,
        delete_branch: Option<String>,
        cross_repository: bool,
        local_branch: Option<Box<crate::plan::PrMergeLocalBranch>>,
    },
    /// A suggestion edits only the working tree; the pre-apply file content is
    /// backed up to the ODB and recoverable by blob SHA (#351).
    ApplySuggestion,
}

impl GithubRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubRecovery::MergePr { number, .. } => format!(
                "GitHub keeps a 'Revert' button on #{} after the merge. Locally, the merge commit can be undone with:\n  git revert -m 1 <merge-sha>\nThe branch itself is restorable from the PR page if it was deleted.",
                number
            ),
            GithubRecovery::ApplySuggestion =>
                "This rewrites only the working-tree file (nothing is staged or committed). The file's pre-apply content is recorded as a blob in the oplog (op=\"apply-suggestion\") first; recover it with `git cat-file -p <blob-sha>`, or discard the change.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocker_notes() {
        assert_eq!(
            GithubNote::NotMergeable { number: 42 }.message_en(),
            "GitHub reports #42 as not mergeable. Resolve conflicts (or satisfy branch protection) first."
        );
        assert_eq!(
            GithubNote::IsDraft { number: 42 }.message_en(),
            "#42 is a draft. Mark it ready for review before merging."
        );
    }

    #[test]
    fn ci_and_review_warnings() {
        assert_eq!(
            GithubNote::ChecksFailing {
                number: 7,
                failed: 3
            }
            .message_en(),
            "#7 has 3 failing check(s). Merging now lands code its CI rejected."
        );
        assert_eq!(
            GithubNote::ChecksPending { number: 7 }.message_en(),
            "#7's checks have not finished yet."
        );
        assert_eq!(
            GithubNote::ChangesRequested { number: 7 }.message_en(),
            "A reviewer requested changes on #7."
        );
    }

    #[test]
    fn side_effect_warnings() {
        assert_eq!(
            GithubNote::RemoteSideEffect.message_en(),
            "This merges on GitHub. Your local clone is unchanged until the next fetch."
        );
        assert_eq!(
            GithubNote::DeletesBranch {
                branch: "feat/x".into()
            }
            .message_en(),
            "The head branch 'feat/x' will be deleted on the remote."
        );
        assert_eq!(
            GithubNote::ForkKeepsRemoteBranch.message_en(),
            "The remote branch in the fork is not deleted by gh."
        );
        // The frozen tip is stated in full: the promise is about *that*
        // commit, and `None` promises the branch is already gone.
        assert_eq!(
            GithubNote::DeletesLocalBranch {
                branch: "feat/x".into(),
                tip: Some("a".repeat(40)),
            }
            .message_en(),
            format!(
                "Once GitHub confirms the merge, the local branch 'feat/x' at {} is deleted here — only if it still points there and is checked out nowhere.",
                "a".repeat(40)
            )
        );
        assert_eq!(
            GithubNote::DeletesLocalBranch {
                branch: "feat/x".into(),
                tip: None,
            }
            .message_en(),
            "The local branch 'feat/x' does not exist here, so nothing local is deleted after the merge."
        );
    }

    /// The receipt quotes these verbatim, so the prefixes are a contract.
    #[test]
    fn local_branch_receipt_notes() {
        assert_eq!(
            GithubNote::LocalBranchDeleted {
                name: "feat/x".into(),
                tip: "b".repeat(40),
            }
            .message_en(),
            format!("local branch deleted: feat/x@{}", "b".repeat(40))
        );
        assert_eq!(
            GithubNote::LocalBranchAbsent {
                name: "feat/x".into()
            }
            .message_en(),
            "local branch already absent: feat/x"
        );
        assert_eq!(
            GithubNote::LocalBranchKept {
                name: "feat/x".into(),
                reason: PrMergeLocalReason::Queued,
            }
            .message_en(),
            "local branch kept: feat/x (GitHub queued the merge, so nothing has been merged yet)"
        );
        assert_eq!(
            GithubNote::LocalBranchNotDeleted {
                reason: PrMergeLocalReason::Changed,
            }
            .message_en(),
            "local branch not deleted: the branch moved after approval"
        );
    }

    /// A blocker the delete-branch family raised keeps *its* words, at every
    /// nesting depth — the flattening this replaces put English into the JA
    /// notice (#705 review P3).
    #[test]
    fn a_kept_branch_quotes_the_delete_familys_own_note() {
        let blocker = crate::plan_note::PlanNote::Branch(
            crate::plan_note::BranchNote::DeleteBranchCheckedOut {
                name: "feat/x".into(),
                path: "/w/other".into(),
            },
        );
        let reason = PrMergeLocalReason::Plan(Box::new(blocker.clone()));
        assert_eq!(reason.message_en(), blocker.message_en());
        assert_eq!(
            GithubNote::KeepsLocalBranch {
                branch: "feat/x".into(),
                reason,
            }
            .message_en(),
            format!(
                "The local branch 'feat/x' is kept, not deleted: {}",
                blocker.message_en()
            )
        );
        assert_eq!(
            PrMergeLocalReason::NotAtPrHead {
                tip: "a".repeat(40),
                head: "b".repeat(40),
            }
            .message_en(),
            format!(
                "the local branch is at {}, not the merged PR head {}",
                "a".repeat(40),
                "b".repeat(40)
            )
        );
    }

    #[test]
    fn merge_title_and_recovery() {
        assert_eq!(
            GithubTitle::MergePr {
                number: 42,
                method: "squash".into()
            }
            .message_en(),
            "Merge pull request #42 (squash)"
        );
        // The frozen branch promise is reconcile material, not display: the
        // recovery text is the same with and without it.
        for delete_branch in [None, Some("feat/x".to_string())] {
            assert_eq!(
                GithubRecovery::MergePr {
                    number: 42,
                    base_repo: "o/r".into(),
                    delete_branch,
                    cross_repository: false,
                    local_branch: None,
                }
                .message_en(),
                "GitHub keeps a 'Revert' button on #42 after the merge. Locally, the merge commit can be undone with:\n  git revert -m 1 <merge-sha>\nThe branch itself is restorable from the PR page if it was deleted."
            );
        }
    }

    #[test]
    fn comment_title_and_empty_body_blocker() {
        assert_eq!(
            GithubTitle::CommentPr { number: 42 }.message_en(),
            "Comment on pull request #42"
        );
        assert_eq!(
            GithubNote::CommentBodyEmpty.message_en(),
            "The comment is empty. Write something before posting it."
        );
        assert_eq!(
            GithubNote::IssueTitleEmpty.message_en(),
            "The issue title is empty. Write a title or add meaningful text to the body."
        );
    }

    #[test]
    fn edit_title_and_empty_edit_blocker() {
        assert_eq!(
            GithubTitle::EditPr { number: 42 }.message_en(),
            "Edit pull request #42"
        );
        assert_eq!(
            GithubNote::FieldEditEmpty { number: 42 }.message_en(),
            "Nothing on #42 would change. Pick a reviewer, assignee or label to add or remove first."
        );
    }
}
