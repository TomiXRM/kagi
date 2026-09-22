//! How the pull-request list is sliced, sectioned and ordered.
//!
//! Split out of `github.rs` when that file passed the 800-LOC ceiling: the PR
//! *model* and the *views over a list of them* are different concerns, and
//! only this half is about presentation order. Pure, like the rest of the
//! crate — the UI owns which slice is showing, never how one is decided.

use crate::github::{PrAttention, PrDetailAvailability, PrGroup, PullRequest};

/// One section of the pull-request navigator.
///
/// These are **filters, not a partition**: a PR the viewer owns, was asked to
/// review and is assigned to belongs in three of them, exactly as GitHub's own
/// pull-request views overlap. Section counts therefore need not add up to the
/// number of open PRs, and a row is drawn once per section it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrSection {
    /// There is something for the viewer to do: it is theirs and broken or
    /// ready, or their review was asked for.
    Inbox,
    /// Authored by the viewer, or its head branch exists locally.
    Mine,
    /// The viewer's review was requested.
    Review,
    /// The viewer is an assignee.
    Assigned,
}

impl PrSection {
    pub const ALL: [PrSection; 4] = [
        PrSection::Inbox,
        PrSection::Mine,
        PrSection::Review,
        PrSection::Assigned,
    ];

    /// Index into a fixed-size per-section array (fold state).
    pub fn index(self) -> usize {
        match self {
            Self::Inbox => 0,
            Self::Mine => 1,
            Self::Review => 2,
            Self::Assigned => 3,
        }
    }

    /// Whether `pr` belongs in this section for the viewer `me`.
    ///
    /// Without a known login nothing is the viewer's: `Inbox` then keeps only
    /// what is broken or ready on a locally-checked-out branch, and the three
    /// viewer-relative sections are empty rather than guessing whose they are.
    pub fn accepts(self, pr: &PullRequest, me: Option<&str>, local_branches: &[String]) -> bool {
        self.accepts_with_status(pr, me, local_branches, PrDetailAvailability::Fresh)
    }

    /// Section membership with an explicit L2 availability. Pending PRs stay
    /// in Inbox so fetching them cannot depend on a verdict that needs L2.
    pub fn accepts_with_status(
        self,
        pr: &PullRequest,
        me: Option<&str>,
        local_branches: &[String],
        status: PrDetailAvailability,
    ) -> bool {
        let is_me = |login: &String| me.is_some_and(|m| m.eq_ignore_ascii_case(login));
        let group = pr.group_for(me, local_branches);
        match self {
            Self::Inbox => {
                let (attention, _) = pr.attention_with_status(
                    group == PrGroup::Mine,
                    group == PrGroup::ReviewRequested,
                    status,
                );
                matches!(
                    attention,
                    PrAttention::NeedsYou | PrAttention::Pending | PrAttention::Ready
                ) || group == PrGroup::ReviewRequested
            }
            Self::Mine => group == PrGroup::Mine,
            Self::Review => pr.reviewers.iter().any(is_me),
            Self::Assigned => pr.assignees.iter().any(is_me),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::CiState;
    use crate::list_filter::{apply_prs, DraftFilter, ListSort, PrFilter, SortField};

    #[test]
    fn ready_and_draft_filters_partition_loaded_rows() {
        let open = PullRequest {
            number: 1,
            ..Default::default()
        };
        let draft = PullRequest {
            number: 2,
            is_draft: true,
            ..Default::default()
        };
        let prs = [open, draft];
        for (draft, wanted) in [(DraftFilter::Ready, vec![1]), (DraftFilter::Draft, vec![2])] {
            let filter = PrFilter {
                draft,
                ..Default::default()
            };
            let kept: Vec<u64> = apply_prs(&prs, &filter, |_| PrDetailAvailability::Fresh)
                .into_iter()
                .map(|index| prs[index].number)
                .collect();
            assert_eq!(kept, wanted, "{draft:?}");
        }
    }

    #[test]
    fn navigator_sections_overlap_because_they_are_filters() {
        // Mine, broken, review asked of me, and assigned to me — all at once.
        let everything = PullRequest {
            number: 1,
            author: "me".into(),
            ci: CiState::Failure,
            reviewers: vec!["ME".into()],
            assignees: vec!["me".into()],
            ..Default::default()
        };
        let matched: Vec<PrSection> = PrSection::ALL
            .into_iter()
            .filter(|s| s.accepts(&everything, Some("me"), &[]))
            .collect();
        assert_eq!(matched, PrSection::ALL.to_vec(), "one PR, four sections");

        // Someone else's, quietly waiting: in none of them.
        let theirs = PullRequest {
            number: 2,
            author: "them".into(),
            ..Default::default()
        };
        assert!(!PrSection::ALL
            .into_iter()
            .any(|s| s.accepts(&theirs, Some("me"), &[])));
    }

    #[test]
    fn without_a_login_only_local_evidence_fills_the_inbox() {
        // A failing PR on a branch that is checked out here is actionable even
        // when `gh` could not say who the viewer is; the viewer-relative
        // sections stay empty rather than claiming it is theirs.
        let pr = PullRequest {
            number: 1,
            head: "feat/x".into(),
            author: "them".into(),
            ci: CiState::Failure,
            reviewers: vec!["me".into()],
            assignees: vec!["me".into()],
            ..Default::default()
        };
        let local = vec!["feat/x".to_string()];
        assert!(PrSection::Inbox.accepts(&pr, None, &local));
        assert!(PrSection::Mine.accepts(&pr, None, &local), "local branch");
        for section in [PrSection::Review, PrSection::Assigned] {
            assert!(!section.accepts(&pr, None, &local), "{section:?}");
        }
    }

    #[test]
    fn sorting_reads_githubs_timestamps_as_instants() {
        let stamped = |n: u64, created: &str, updated: &str| PullRequest {
            number: n,
            created_at: created.into(),
            updated_at: updated.into(),
            ..Default::default()
        };
        // #2 was created last but touched first; #3 has no timestamps at all.
        // Note the day/month digits: a naive numeric or length-based compare
        // would put 2026-02-09 after 2026-10-01.
        let prs = vec![
            stamped(1, "2026-02-09T00:00:00Z", "2026-10-01T00:00:00Z"),
            stamped(2, "2026-10-01T00:00:00Z", "2026-02-09T00:00:00Z"),
            stamped(3, "", ""),
        ];

        let numbers = |field| {
            apply_prs(
                &prs,
                &PrFilter {
                    sort: ListSort {
                        field,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                |_| PrDetailAvailability::Missing,
            )
            .into_iter()
            .map(|index| prs[index].number)
            .collect::<Vec<_>>()
        };
        assert_eq!(
            numbers(SortField::Updated),
            vec![1, 2, 3],
            "newest update first"
        );
        assert_eq!(
            numbers(SortField::Created),
            vec![2, 1, 3],
            "newest PR first"
        );
        assert_eq!(numbers(SortField::Number), vec![3, 2, 1]);
    }
}
