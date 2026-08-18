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
    /// Logins with a pending review request.
    pub reviewers: Vec<String>,
}

/// Which sidebar group a PR belongs to, from the viewer's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrGroup {
    /// Authored by me, or its head branch exists locally.
    Mine,
    /// My review was requested.
    ReviewRequested,
    Others,
}

impl PullRequest {
    /// Classify for the viewer `me` (login, if known) given the local branch
    /// names. Pure; the sidebar groups on this.
    pub fn group_for(&self, me: Option<&str>, local_branches: &[String]) -> PrGroup {
        let mine = me.is_some_and(|m| m.eq_ignore_ascii_case(&self.author))
            || local_branches.iter().any(|b| b == &self.head);
        if mine {
            PrGroup::Mine
        } else if me.is_some_and(|m| self.reviewers.iter().any(|r| r.eq_ignore_ascii_case(m))) {
            PrGroup::ReviewRequested
        } else {
            PrGroup::Others
        }
    }
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

/// Order PRs as a stack forest for display: roots (base is not another open
/// PR's head) in input order, each followed by its stacked children
/// depth-first. Returns `(index into prs, depth)`. Cycles (impossible on
/// GitHub, but the data is external) are broken by the visited set.
pub fn stack_order(prs: &[PullRequest]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(prs.len());
    let mut visited = vec![false; prs.len()];
    fn walk(
        prs: &[PullRequest],
        i: usize,
        depth: usize,
        visited: &mut [bool],
        out: &mut Vec<(usize, usize)>,
    ) {
        if visited[i] {
            return;
        }
        visited[i] = true;
        out.push((i, depth));
        for (j, child) in prs.iter().enumerate() {
            if !visited[j] && child.base == prs[i].head {
                walk(prs, j, depth + 1, visited, out);
            }
        }
    }
    for i in 0..prs.len() {
        if !prs[i].is_stacked_on(prs) {
            walk(prs, i, 0, &mut visited, &mut out);
        }
    }
    // Anything left is part of a cycle: emit flat.
    for i in 0..prs.len() {
        if !visited[i] {
            walk(prs, i, 0, &mut visited, &mut out);
        }
    }
    out
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
            reviewers: Vec::new(),
        };
        let prs = vec![mk(1, "feat/a", "main"), mk(2, "feat/b", "feat/a")];
        assert!(!prs[0].is_stacked_on(&prs));
        assert!(prs[1].is_stacked_on(&prs));
    }

    #[test]
    fn stack_order_puts_children_under_their_base() {
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
            reviewers: Vec::new(),
        };
        // 3 stacked on 1; 2 independent; 4 stacked on 3.
        let prs = vec![
            mk(1, "a", "main"),
            mk(2, "b", "main"),
            mk(3, "c", "a"),
            mk(4, "d", "c"),
        ];
        let order: Vec<(u64, usize)> = stack_order(&prs)
            .into_iter()
            .map(|(i, d)| (prs[i].number, d))
            .collect();
        assert_eq!(order, vec![(1, 0), (3, 1), (4, 2), (2, 0)]);
    }

    #[test]
    fn grouping_is_by_author_local_branch_then_review_request() {
        let mut pr = PullRequest {
            number: 1,
            title: String::new(),
            head: "feat/x".into(),
            base: "main".into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: "alice".into(),
            reviewers: vec!["bob".into()],
        };
        let local = vec!["main".to_string()];
        assert_eq!(pr.group_for(Some("alice"), &local), PrGroup::Mine);
        assert_eq!(pr.group_for(Some("bob"), &local), PrGroup::ReviewRequested);
        assert_eq!(pr.group_for(Some("carol"), &local), PrGroup::Others);
        // A local branch of the same name makes it mine regardless of author.
        assert_eq!(
            pr.group_for(Some("carol"), &["feat/x".to_string()]),
            PrGroup::Mine
        );
        // Unknown viewer: only the local-branch rule can say "mine".
        pr.author = "someone".into();
        assert_eq!(pr.group_for(None, &local), PrGroup::Others);
    }
}
