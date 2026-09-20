//! How the pull-request list is sliced, sectioned and ordered.
//!
//! Split out of `github.rs` when that file passed the 800-LOC ceiling: the PR
//! *model* and the *views over a list of them* are different concerns, and
//! only this half is about presentation order. Pure, like the rest of the
//! crate — the UI owns which slice is showing, never how one is decided.

use crate::github::{PrAttention, PrDetailAvailability, PrGroup, PullRequest};

/// Which slice of the *open* pull requests the list is showing.
///
/// The two are disjoint, so their counts add up to what was fetched: GitHub
/// calls a draft "open" too, and a header that said `OPEN 12 · DRAFT 3` out of
/// twelve would be counting three of them twice. A merged slice is not here
/// because merged PRs are a different fetch (`list_merged_prs`) with a
/// different field set; a chip for them arrives with that data, not before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrListFilter {
    /// Open and ready for review.
    #[default]
    Open,
    /// Open, but marked draft.
    Draft,
}

impl PrListFilter {
    pub fn accepts(self, pr: &PullRequest) -> bool {
        match self {
            Self::Open => !pr.is_draft,
            Self::Draft => pr.is_draft,
        }
    }
}

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

/// How the list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrSort {
    /// Most recently updated first — what `gh pr list` already returns, and
    /// the order the header calls `SORT: UPDATED`.
    #[default]
    Updated,
    /// Newest PR first.
    Created,
    /// Highest number first.
    Number,
}

/// Order `prs` in place.
///
/// `created_at` / `updated_at` are `gh`'s RFC-3339 UTC strings — fixed width,
/// zero-padded, same offset — so comparing them as text is comparing the
/// instants they name, and this crate needs no date parsing to sort. A PR
/// whose timestamp is missing (a reduced field set) sorts last rather than
/// first: an empty string is the smallest, and it is not evidence of age.
pub fn sort_prs(prs: &mut [PullRequest], sort: PrSort) {
    match sort {
        PrSort::Updated => {
            prs.sort_by(|a, b| stamp_key(&b.updated_at).cmp(&stamp_key(&a.updated_at)))
        }
        PrSort::Created => {
            prs.sort_by(|a, b| stamp_key(&b.created_at).cmp(&stamp_key(&a.created_at)))
        }
        PrSort::Number => prs.sort_by(|a, b| b.number.cmp(&a.number)),
    }
}

/// A missing timestamp must not read as "oldest possible"; ordering is
/// descending, so the absent ones are keyed below every real stamp.
fn stamp_key(stamp: &str) -> Option<&str> {
    (!stamp.is_empty()).then_some(stamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::CiState;

    #[test]
    fn the_two_list_slices_partition_what_was_fetched() {
        let open = PullRequest {
            number: 1,
            ..Default::default()
        };
        let draft = PullRequest {
            number: 2,
            is_draft: true,
            ..Default::default()
        };
        for (slice, wanted) in [
            (PrListFilter::Open, vec![1]),
            (PrListFilter::Draft, vec![2]),
        ] {
            let kept: Vec<u64> = [&open, &draft]
                .into_iter()
                .filter(|pr| slice.accepts(pr))
                .map(|pr| pr.number)
                .collect();
            assert_eq!(kept, wanted, "{slice:?}");
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
    fn section_indices_are_distinct_and_dense() {
        let mut seen: Vec<usize> = PrSection::ALL.into_iter().map(PrSection::index).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3]);
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
        let mut prs = vec![
            stamped(1, "2026-02-09T00:00:00Z", "2026-10-01T00:00:00Z"),
            stamped(2, "2026-10-01T00:00:00Z", "2026-02-09T00:00:00Z"),
            stamped(3, "", ""),
        ];

        sort_prs(&mut prs, PrSort::Updated);
        assert_eq!(numbers(&prs), vec![1, 2, 3], "newest update first");

        sort_prs(&mut prs, PrSort::Created);
        assert_eq!(numbers(&prs), vec![2, 1, 3], "newest PR first");

        sort_prs(&mut prs, PrSort::Number);
        assert_eq!(numbers(&prs), vec![3, 2, 1]);
    }

    fn numbers(prs: &[PullRequest]) -> Vec<u64> {
        prs.iter().map(|pr| pr.number).collect()
    }
}
