//! GithubNote — PR operations that go through `gh` (2026-08-19).
//!
//! Merging a PR is a remote write, so it takes the same
//! plan → confirm → preflight → execute → oplog path as every local write
//! op. The notes below are what the confirm modal states before the user
//! commits to it.

/// Plan notes for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubNote {
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
    /// until the next fetch.
    RemoteSideEffect,
    /// warning — `--delete-branch` also deletes the local branch when it is
    /// checked out nowhere.
    DeletesBranch { branch: String },
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
}

impl GithubNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
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
                "The head branch '{}' will be deleted on the remote (and locally if it is not checked out).",
                branch
            ),
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
        }
    }
}

/// Plan titles for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTitle {
    /// `Merge pull request #<n> (<method>)`.
    MergePr { number: u64, method: String },
    /// `Apply suggestion to '<path>'` (#351).
    ApplySuggestion { path: String },
}

impl GithubTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubTitle::MergePr { number, method } => {
                format!("Merge pull request #{} ({})", number, method)
            }
            GithubTitle::ApplySuggestion { path } => {
                format!("Apply suggestion to '{}'", path)
            }
        }
    }
}

/// Recovery kinds for the GitHub PR ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubRecovery {
    /// A merged PR can be reverted on GitHub, or locally with `git revert -m 1`.
    MergePr { number: u64 },
    /// A suggestion edits only the working tree; the pre-apply file content is
    /// backed up to the ODB and recoverable by blob SHA (#351).
    ApplySuggestion,
}

impl GithubRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubRecovery::MergePr { number } => format!(
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
            "The head branch 'feat/x' will be deleted on the remote (and locally if it is not checked out)."
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
        assert_eq!(
            GithubRecovery::MergePr { number: 42 }.message_en(),
            "GitHub keeps a 'Revert' button on #42 after the merge. Locally, the merge commit can be undone with:\n  git revert -m 1 <merge-sha>\nThe branch itself is restorable from the PR page if it was deleted."
        );
    }
}
