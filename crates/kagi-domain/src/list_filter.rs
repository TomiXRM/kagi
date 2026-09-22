//! Composable predicates and stable ordering over already-loaded GitHub rows.
//! Collection membership remains owned by IssueListTab / PrSection; callers
//! pass that collection here. Filtering never fetches missing checks or pages.
use std::cmp::Ordering;

use crate::github::{CiState, Issue, IssueLabel, IssueState, PrDetailAvailability, PullRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StateFilter {
    Open,
    Closed,
    #[default]
    All,
}

impl StateFilter {
    fn accepts(self, state: IssueState) -> bool {
        match self {
            Self::Open => state == IssueState::Open,
            Self::Closed => state == IssueState::Closed,
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DraftFilter {
    Ready,
    Draft,
    #[default]
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChecksFilter {
    Passing,
    Failing,
    Pending,
    #[default]
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Updated,
    Created,
    Number,
    Comments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    Ascending,
    #[default]
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ListSort {
    pub field: SortField,
    pub direction: SortDirection,
}

/// Empty predicates preserve membership. Multiple selected labels are ANDed,
/// just like state, author and title. Labels/logins compare ASCII-insensitively;
/// title matching is a case-insensitive substring, not GitHub search syntax.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListFilter {
    pub state: StateFilter,
    pub labels: Vec<String>,
    pub author: Option<String>,
    pub text: String,
}

impl ListFilter {
    fn accepts(
        &self,
        state: IssueState,
        labels: &[IssueLabel],
        author: &str,
        title: &str,
        text: &str,
    ) -> bool {
        self.state.accepts(state)
            && self.labels.iter().all(|selected| {
                labels
                    .iter()
                    .any(|label| label.name.eq_ignore_ascii_case(selected))
            })
            && self
                .author
                .as_deref()
                .is_none_or(|selected| author.eq_ignore_ascii_case(selected))
            && (text.is_empty() || title.to_lowercase().contains(text))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IssueFilter {
    pub common: ListFilter,
    pub sort: ListSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrFilter {
    pub common: ListFilter,
    pub draft: DraftFilter,
    pub checks: ChecksFilter,
    pub sort: ListSort,
}

/// Returns indices into the input snapshot, preserving input order for equal
/// sort keys. Virtual viewports retain this small projection, not cloned rows.
pub fn apply_issues(issues: &[Issue], filter: &IssueFilter) -> Vec<usize> {
    let text = filter.common.text.to_lowercase();
    let mut rows: Vec<_> = issues
        .iter()
        .enumerate()
        .filter_map(|(index, issue)| {
            filter
                .common
                .accepts(
                    issue.state,
                    &issue.labels,
                    &issue.author,
                    &issue.title,
                    &text,
                )
                .then_some(index)
        })
        .collect();
    rows.sort_by(|&a, &b| {
        compare(
            filter.sort,
            RowKey::issue(&issues[a]),
            RowKey::issue(&issues[b]),
        )
    });
    rows
}

/// Availability is supplied by the existing session-owned L2 read model.
/// A stale, missing or in-flight verdict cannot satisfy a checks predicate.
pub fn apply_prs(
    prs: &[PullRequest],
    filter: &PrFilter,
    availability: impl Fn(&PullRequest) -> PrDetailAvailability,
) -> Vec<usize> {
    let text = filter.common.text.to_lowercase();
    let mut rows: Vec<_> = prs
        .iter()
        .enumerate()
        .filter_map(|(index, pr)| {
            let accepts = filter
                .common
                .accepts(pr.state, &pr.labels, &pr.author, &pr.title, &text)
                && match filter.draft {
                    DraftFilter::Ready => !pr.is_draft,
                    DraftFilter::Draft => pr.is_draft,
                    DraftFilter::All => true,
                }
                && (filter.checks == ChecksFilter::All
                    || (availability(pr) == PrDetailAvailability::Fresh
                        && matches!(
                            (filter.checks, pr.ci),
                            (ChecksFilter::Passing, CiState::Success)
                                | (ChecksFilter::Failing, CiState::Failure)
                                | (ChecksFilter::Pending, CiState::Pending)
                        )));
            accepts.then_some(index)
        })
        .collect();
    rows.sort_by(|&a, &b| compare(filter.sort, RowKey::pr(&prs[a]), RowKey::pr(&prs[b])));
    rows
}

struct RowKey<'a> {
    updated: &'a str,
    created: &'a str,
    number: u64,
    comments: usize,
}

impl<'a> RowKey<'a> {
    fn issue(issue: &'a Issue) -> Self {
        Self {
            updated: &issue.updated_at,
            created: &issue.created_at,
            number: issue.number,
            comments: issue.comment_count,
        }
    }

    fn pr(pr: &'a PullRequest) -> Self {
        Self {
            updated: &pr.updated_at,
            created: &pr.created_at,
            number: pr.number,
            comments: pr.comment_count,
        }
    }
}

fn compare(sort: ListSort, a: RowKey<'_>, b: RowKey<'_>) -> Ordering {
    let order = match sort.field {
        SortField::Updated | SortField::Created => {
            let (a, b) = if sort.field == SortField::Updated {
                (a.updated, b.updated)
            } else {
                (a.created, b.created)
            };
            // Missing timestamps are absence of evidence, not the oldest date.
            match (a.is_empty(), b.is_empty()) {
                (true, false) => return Ordering::Greater,
                (false, true) => return Ordering::Less,
                _ => a.cmp(b),
            }
        }
        SortField::Number => a.number.cmp(&b.number),
        SortField::Comments => a.comments.cmp(&b.comments),
    };
    match sort.direction {
        SortDirection::Ascending => order,
        SortDirection::Descending => order.reverse(),
    }
}

#[cfg(test)]
#[path = "list_filter_tests.rs"]
mod tests;
