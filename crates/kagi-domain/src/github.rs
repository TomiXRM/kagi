//! GitHub pull-request model (pure). Filled by `kagi_git::github` from
//! `gh pr list --json`; rendered by the sidebar's PULL REQUESTS section.

/// Aggregate CI state of a PR's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    /// No checks reported (or `gh` returned none).
    None,
    Pending,
    Success,
    Failure,
}

/// GitHub's `reviewDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// No decision yet (also: no reviewers requested).
    None,
    ReviewRequired,
    ChangesRequested,
    Approved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    /// Head (source) branch name, without the remote prefix.
    pub head: String,
    /// Base (target) branch name.
    pub base: String,
    pub is_draft: bool,
    pub ci: CiState,
    pub review: ReviewState,
    pub url: String,
    /// Login of the PR author.
    pub author: String,
}

impl PullRequest {
    /// Whether this PR's base is itself another open PR's head — i.e. it is
    /// stacked on `others`. Pure; used to mark stacked PRs in the list.
    pub fn is_stacked_on(&self, others: &[PullRequest]) -> bool {
        others
            .iter()
            .any(|o| o.number != self.number && o.head == self.base)
    }
}

/// Fold per-check conclusions into one [`CiState`]: any failure wins, then
/// any pending, then success; no checks → `None`.
pub fn fold_ci(conclusions: &[Option<&str>]) -> CiState {
    if conclusions.is_empty() {
        return CiState::None;
    }
    let mut pending = false;
    for c in conclusions {
        match c.map(|s| s.to_ascii_uppercase()) {
            Some(ref s)
                if s == "FAILURE" || s == "TIMED_OUT" || s == "CANCELLED" || s == "ERROR" =>
            {
                return CiState::Failure
            }
            Some(ref s) if s == "SUCCESS" || s == "NEUTRAL" || s == "SKIPPED" => {}
            _ => pending = true,
        }
    }
    if pending {
        CiState::Pending
    } else {
        CiState::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_folds_failure_over_pending_over_success() {
        assert_eq!(fold_ci(&[]), CiState::None);
        assert_eq!(
            fold_ci(&[Some("SUCCESS"), Some("SKIPPED")]),
            CiState::Success
        );
        assert_eq!(fold_ci(&[Some("SUCCESS"), None]), CiState::Pending);
        assert_eq!(
            fold_ci(&[Some("SUCCESS"), None, Some("FAILURE")]),
            CiState::Failure
        );
    }

    #[test]
    fn stacked_detection_uses_head_of_another_open_pr() {
        let mk = |n, head: &str, base: &str| PullRequest {
            number: n,
            title: String::new(),
            head: head.into(),
            base: base.into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
        };
        let prs = vec![mk(1, "feat/a", "main"), mk(2, "feat/b", "feat/a")];
        assert!(!prs[0].is_stacked_on(&prs));
        assert!(prs[1].is_stacked_on(&prs));
    }
}
