//! GithubNote — PR operations that go through `gh` (2026-08-19).
//!
//! Merging a PR is a remote write, so it takes the same
//! plan → confirm → preflight → execute → oplog path as every local write
//! op. The notes below are what the confirm modal states before the user
//! commits to it.

/// Plan notes for the GitHub PR ops.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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
        }
    }
}

/// Plan titles for the GitHub PR ops.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum GithubTitle {
    /// `Merge pull request #<n> (<method>)`.
    MergePr { number: u64, method: String },
}

impl GithubTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubTitle::MergePr { number, method } => {
                format!("Merge pull request #{} ({})", number, method)
            }
        }
    }
}

/// Recovery kinds for the GitHub PR ops.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum GithubRecovery {
    /// A merged PR can be reverted on GitHub, or locally with `git revert -m 1`.
    MergePr { number: u64 },
}

impl GithubRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            GithubRecovery::MergePr { number } => format!(
                "GitHub keeps a 'Revert' button on #{} after the merge. Locally, the merge commit can be undone with:\n  git revert -m 1 <merge-sha>\nThe branch itself is restorable from the PR page if it was deleted.",
                number
            ),
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
