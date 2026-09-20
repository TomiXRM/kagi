//! Ownership-aware composition of the three pull-request fetch levels.

use crate::github::{Check, CiState, Mergeable, PullRequest};

/// L2 data fetched for one pull request. The head SHA is part of the payload so
/// a completion can never attach checks from an old head to the current PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatusDetail {
    pub number: u64,
    pub head_sha: String,
    pub ci: CiState,
    pub checks: Vec<Check>,
    pub mergeable: Mergeable,
}

/// L3 data fetched for one pull request. Empty body and zero counts are valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrBodyDetail {
    pub number: u64,
    pub head_sha: String,
    pub updated_at: String,
    pub body: String,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
}

/// Whether volatile L2 fields can be used for an attention verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrDetailAvailability {
    #[default]
    Missing,
    Loading,
    Fresh,
    Stale,
}

/// Replace L1 fields while retaining separately fetched details for the same
/// PR head. A changed head deliberately drops the old L2/L3 values.
pub fn apply_pr_list(cache: &mut Vec<PullRequest>, incoming: Vec<PullRequest>) -> bool {
    let previous = std::mem::take(cache);
    let mut merged = Vec::with_capacity(incoming.len());
    for mut listed in incoming {
        if let Some(old) = previous
            .iter()
            .find(|old| old.number == listed.number && old.head_sha == listed.head_sha)
        {
            listed.ci = old.ci;
            listed.checks.clone_from(&old.checks);
            listed.mergeable = old.mergeable;
            listed.body.clone_from(&old.body);
            listed.changed_files = old.changed_files;
            listed.additions = old.additions;
            listed.deletions = old.deletions;
        }
        merged.push(listed);
    }
    let changed = previous != merged;
    *cache = merged;
    changed
}

pub fn apply_pr_status(pr: &mut PullRequest, detail: &PrStatusDetail) -> bool {
    if pr.number != detail.number || pr.head_sha != detail.head_sha {
        return false;
    }
    pr.ci = detail.ci;
    pr.checks.clone_from(&detail.checks);
    pr.mergeable = detail.mergeable;
    true
}

pub fn apply_pr_body(pr: &mut PullRequest, detail: &PrBodyDetail) -> bool {
    if pr.number != detail.number || pr.head_sha != detail.head_sha {
        return false;
    }
    pr.body.clone_from(&detail.body);
    pr.changed_files = detail.changed_files;
    pr.additions = detail.additions;
    pr.deletions = detail.deletions;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::ReviewState;

    fn listed(head: &str) -> PullRequest {
        PullRequest {
            number: 42,
            title: "new title".into(),
            head_sha: head.into(),
            review: ReviewState::Approved,
            updated_at: "new-time".into(),
            ..Default::default()
        }
    }

    fn detailed(head: &str) -> PullRequest {
        PullRequest {
            title: "old title".into(),
            ci: CiState::Failure,
            checks: vec![Check {
                name: "ci".into(),
                workflow: "test".into(),
                state: CiState::Failure,
                url: String::new(),
            }],
            mergeable: Mergeable::Conflicting,
            body: "body".into(),
            changed_files: 3,
            additions: 8,
            deletions: 5,
            updated_at: "old-time".into(),
            ..listed(head)
        }
    }

    #[test]
    fn list_refresh_updates_only_l1_and_preserves_same_head_details() {
        let mut cache = vec![detailed("same")];
        assert!(apply_pr_list(&mut cache, vec![listed("same")]));
        let pr = &cache[0];
        assert_eq!(pr.title, "new title");
        assert_eq!(pr.updated_at, "new-time", "updatedAt is L1-owned");
        assert_eq!(pr.checks.len(), 1);
        assert_eq!(pr.mergeable, Mergeable::Conflicting);
        assert_eq!(pr.body, "body");
        assert_eq!((pr.changed_files, pr.additions, pr.deletions), (3, 8, 5));
    }

    #[test]
    fn changed_head_does_not_present_old_details_as_current() {
        let mut cache = vec![detailed("old")];
        apply_pr_list(&mut cache, vec![listed("new")]);
        let pr = &cache[0];
        assert!(pr.checks.is_empty());
        assert_eq!(pr.mergeable, Mergeable::Unknown);
        assert!(pr.body.is_empty());
        assert_eq!((pr.changed_files, pr.additions, pr.deletions), (0, 0, 0));
    }

    #[test]
    fn detail_appliers_accept_empty_values_and_reject_wrong_head() {
        let mut pr = detailed("same");
        let status = PrStatusDetail {
            number: 42,
            head_sha: "same".into(),
            ci: CiState::None,
            checks: Vec::new(),
            mergeable: Mergeable::Clean,
        };
        assert!(apply_pr_status(&mut pr, &status));
        assert!(pr.checks.is_empty());
        assert_eq!(pr.mergeable, Mergeable::Clean);

        let body = PrBodyDetail {
            number: 42,
            head_sha: "same".into(),
            updated_at: "detail-time".into(),
            body: String::new(),
            changed_files: 0,
            additions: 0,
            deletions: 0,
        };
        assert!(apply_pr_body(&mut pr, &body));
        assert!(pr.body.is_empty());
        assert_eq!((pr.changed_files, pr.additions, pr.deletions), (0, 0, 0));
        assert_eq!(pr.updated_at, "old-time", "L3 cannot overwrite L1 fields");

        let mut wrong = status;
        wrong.head_sha = "moved".into();
        assert!(!apply_pr_status(&mut pr, &wrong));
    }
}
