//! Pure attention verdicts for pull-request lists.

use crate::github::{CiState, Mergeable, PrDetailAvailability, PullRequest, ReviewState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrAttention {
    NeedsYou,
    Pending,
    InProgress,
    Ready,
    Waiting,
    Dormant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrReason {
    CiFailed(usize),
    ChangesRequested,
    Conflicting,
    CiRunning,
    ReadyToMerge,
    ReviewRequested,
    AwaitingReview,
    Draft,
    Pending,
    None,
}

impl PullRequest {
    pub fn failed_checks(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.state == CiState::Failure)
            .count()
    }

    pub fn attention(&self, mine: bool, review_requested: bool) -> (PrAttention, PrReason) {
        self.attention_with_status(mine, review_requested, PrDetailAvailability::Fresh)
    }

    pub fn attention_with_status(
        &self,
        mine: bool,
        review_requested: bool,
        status: PrDetailAvailability,
    ) -> (PrAttention, PrReason) {
        if mine {
            if self.review == ReviewState::ChangesRequested {
                return (PrAttention::NeedsYou, PrReason::ChangesRequested);
            }
            if status != PrDetailAvailability::Fresh {
                return (PrAttention::Pending, PrReason::Pending);
            }
            if self.mergeable == Mergeable::Conflicting {
                return (PrAttention::NeedsYou, PrReason::Conflicting);
            }
            let failed = self.failed_checks();
            if failed > 0 || self.ci == CiState::Failure {
                return (PrAttention::NeedsYou, PrReason::CiFailed(failed.max(1)));
            }
            if self.ci == CiState::Pending {
                return (PrAttention::InProgress, PrReason::CiRunning);
            }
            if self.is_draft {
                return (PrAttention::InProgress, PrReason::Draft);
            }
            if self.review == ReviewState::Approved || self.mergeable == Mergeable::Clean {
                return (PrAttention::Ready, PrReason::ReadyToMerge);
            }
            return (PrAttention::Waiting, PrReason::AwaitingReview);
        }
        if review_requested {
            return (PrAttention::Waiting, PrReason::ReviewRequested);
        }
        (PrAttention::Dormant, PrReason::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Check;

    fn pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: "t".into(),
            head: "feat".into(),
            base: "main".into(),
            ci: CiState::Success,
            author: "me".into(),
            mergeable: Mergeable::Clean,
            cross_repository: false,
            base_repo: "o/r".into(),
            ..Default::default()
        }
    }

    fn check(state: CiState) -> Check {
        Check {
            name: "test".into(),
            workflow: "ci".into(),
            state,
            url: String::new(),
        }
    }

    #[test]
    fn mine_failing_ci_needs_you_with_a_count() {
        let mut pr = pr();
        pr.ci = CiState::Failure;
        pr.checks = vec![
            check(CiState::Failure),
            check(CiState::Success),
            check(CiState::Failure),
        ];
        assert_eq!(
            pr.attention(true, false),
            (PrAttention::NeedsYou, PrReason::CiFailed(2))
        );
    }

    #[test]
    fn conflicts_outrank_ci_and_reviews() {
        let mut pr = pr();
        pr.mergeable = Mergeable::Conflicting;
        pr.ci = CiState::Failure;
        assert_eq!(
            pr.attention(true, false),
            (PrAttention::NeedsYou, PrReason::Conflicting)
        );
    }

    #[test]
    fn running_ci_and_drafts_are_in_progress_not_actionable() {
        let mut running = pr();
        running.ci = CiState::Pending;
        assert_eq!(running.attention(true, false).0, PrAttention::InProgress);
        let mut draft = pr();
        draft.is_draft = true;
        assert_eq!(
            draft.attention(true, false),
            (PrAttention::InProgress, PrReason::Draft)
        );
    }

    #[test]
    fn green_and_approved_is_ready_to_merge() {
        let mut pr = pr();
        pr.review = ReviewState::Approved;
        assert_eq!(
            pr.attention(true, false),
            (PrAttention::Ready, PrReason::ReadyToMerge)
        );
    }

    #[test]
    fn missing_or_stale_l2_is_pending_but_l1_changes_requested_is_final() {
        let mut pr = pr();
        pr.review = ReviewState::Approved;
        for status in [
            PrDetailAvailability::Missing,
            PrDetailAvailability::Loading,
            PrDetailAvailability::Stale,
        ] {
            assert_eq!(
                pr.attention_with_status(true, false, status),
                (PrAttention::Pending, PrReason::Pending)
            );
        }
        pr.review = ReviewState::ChangesRequested;
        assert_eq!(
            pr.attention_with_status(true, false, PrDetailAvailability::Missing),
            (PrAttention::NeedsYou, PrReason::ChangesRequested)
        );
    }

    #[test]
    fn other_peoples_prs_only_surface_when_your_review_is_requested() {
        let mut pr = pr();
        pr.ci = CiState::Failure;
        assert_eq!(pr.attention(false, false).0, PrAttention::Dormant);
        assert_eq!(
            pr.attention(false, true),
            (PrAttention::Waiting, PrReason::ReviewRequested)
        );
    }
}
