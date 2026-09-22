use super::*;

fn issue(number: u64) -> Issue {
    Issue {
        number,
        title: format!("Issue {number}"),
        state: IssueState::Open,
        url: String::new(),
        author: "alice".into(),
        assignees: Vec::new(),
        labels: Vec::new(),
        body: String::new(),
        comments: Vec::new(),
        comment_count: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn label(name: &str) -> IssueLabel {
    IssueLabel {
        name: name.into(),
        color: String::new(),
        description: String::new(),
    }
}

fn numbers(rows: &[Issue], indices: Vec<usize>) -> Vec<u64> {
    indices
        .into_iter()
        .map(|index| rows[index].number)
        .collect()
}

#[test]
fn empty_predicates_preserve_every_row_and_equal_key_input_order() {
    let mut rows = vec![issue(9), issue(1), issue(7)];
    rows[1].state = IssueState::Closed;
    rows[2].state = IssueState::Unknown;
    assert_eq!(
        numbers(&rows, apply_issues(&rows, &IssueFilter::default())),
        vec![9, 1, 7]
    );
    let prs: Vec<_> = rows
        .iter()
        .map(|row| PullRequest {
            number: row.number,
            state: row.state,
            ..Default::default()
        })
        .collect();
    assert_eq!(
        apply_prs(&prs, &PrFilter::default(), |_| panic!(
            "All must not inspect checks"
        ))
        .iter()
        .map(|&index| prs[index].number)
        .collect::<Vec<_>>(),
        vec![9, 1, 7]
    );
}

#[test]
fn issue_predicates_and_each_selected_label_compose_with_collection_membership() {
    let mut matching = issue(1);
    matching.title = "Repair 日本語 INPUT".into();
    matching.labels = vec![label("bug"), label("UI")];
    let mut rows = vec![matching.clone(); 6];
    for (index, row) in rows.iter_mut().enumerate() {
        row.number = index as u64 + 1;
    }
    rows[1].state = IssueState::Closed;
    rows[2].labels.pop();
    rows[3].author = "bob".into();
    rows[4].title = "Other task".into();
    let filter = IssueFilter {
        common: ListFilter {
            state: StateFilter::Open,
            labels: vec!["BUG".into(), "ui".into()],
            author: Some("ALICE".into()),
            text: "日本語 input".into(),
        },
        ..Default::default()
    };
    // Row 6 satisfies the strip but is outside the selected collection.
    let indices = apply_issues(&rows, &filter)
        .into_iter()
        .filter(|&index| rows[index].number != 6)
        .collect();
    assert_eq!(numbers(&rows, indices), vec![1]);
}

#[test]
fn closed_does_not_accept_unknown_or_open_issues() {
    let mut rows = vec![issue(1), issue(2), issue(3)];
    rows[1].state = IssueState::Closed;
    rows[2].state = IssueState::Unknown;
    let filter = IssueFilter {
        common: ListFilter {
            state: StateFilter::Closed,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(numbers(&rows, apply_issues(&rows, &filter)), vec![2]);
}

#[test]
fn pr_common_draft_and_checks_predicates_are_all_required() {
    let matching = PullRequest {
        number: 1,
        state: IssueState::Open,
        title: "Fix input".into(),
        author: "alice".into(),
        labels: vec![label("bug")],
        ci: CiState::Failure,
        ..Default::default()
    };
    let mut prs = vec![matching; 5];
    for (index, pr) in prs.iter_mut().enumerate() {
        pr.number = index as u64 + 1;
    }
    prs[1].is_draft = true;
    prs[2].ci = CiState::Success;
    prs[3].labels.clear();
    prs[4].state = IssueState::Closed;
    let filter = PrFilter {
        common: ListFilter {
            state: StateFilter::Open,
            labels: vec!["bug".into()],
            author: Some("Alice".into()),
            text: "INPUT".into(),
        },
        draft: DraftFilter::Ready,
        checks: ChecksFilter::Failing,
        ..Default::default()
    };
    assert_eq!(
        apply_prs(&prs, &filter, |_| PrDetailAvailability::Fresh)
            .iter()
            .map(|&index| prs[index].number)
            .collect::<Vec<_>>(),
        vec![1]
    );
}

#[test]
fn checks_filter_requires_fresh_evidence_and_distinguishes_no_checks() {
    let prs = vec![
        PullRequest {
            number: 1,
            ci: CiState::Success,
            ..Default::default()
        },
        PullRequest {
            number: 2,
            ci: CiState::Failure,
            ..Default::default()
        },
        PullRequest {
            number: 3,
            ci: CiState::Pending,
            ..Default::default()
        },
        PullRequest {
            number: 4,
            ci: CiState::None,
            ..Default::default()
        },
    ];
    for (checks, expected) in [
        (ChecksFilter::Passing, 1),
        (ChecksFilter::Failing, 2),
        (ChecksFilter::Pending, 3),
    ] {
        let filter = PrFilter {
            checks,
            ..Default::default()
        };
        assert_eq!(
            apply_prs(&prs, &filter, |_| PrDetailAvailability::Fresh)
                .iter()
                .map(|&index| prs[index].number)
                .collect::<Vec<_>>(),
            vec![expected]
        );
        for availability in [
            PrDetailAvailability::Missing,
            PrDetailAvailability::Loading,
            PrDetailAvailability::Stale,
        ] {
            assert!(apply_prs(&prs, &filter, |_| availability).is_empty());
        }
    }
}

#[test]
fn all_four_sort_keys_work_in_both_directions_without_mutating_input() {
    let mut rows = vec![issue(3), issue(1), issue(2)];
    rows[0].updated_at = "2026-01-02T00:00:00Z".into();
    rows[0].created_at = "2025-01-03T00:00:00Z".into();
    rows[0].comment_count = 1;
    rows[1].updated_at = "2026-01-03T00:00:00Z".into();
    rows[1].created_at = "2025-01-01T00:00:00Z".into();
    rows[1].comment_count = 2;
    rows[2].updated_at = "2026-01-01T00:00:00Z".into();
    rows[2].created_at = "2025-01-02T00:00:00Z".into();
    rows[2].comment_count = 3;
    let prs: Vec<_> = rows
        .iter()
        .map(|row| PullRequest {
            number: row.number,
            updated_at: row.updated_at.clone(),
            created_at: row.created_at.clone(),
            comment_count: row.comment_count,
            ..Default::default()
        })
        .collect();
    for (field, ascending) in [
        (SortField::Updated, vec![2, 3, 1]),
        (SortField::Created, vec![1, 2, 3]),
        (SortField::Number, vec![1, 2, 3]),
        (SortField::Comments, vec![3, 1, 2]),
    ] {
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            let sort = ListSort { field, direction };
            let mut expected = ascending.clone();
            if direction == SortDirection::Descending {
                expected.reverse();
            }
            assert_eq!(
                numbers(
                    &rows,
                    apply_issues(
                        &rows,
                        &IssueFilter {
                            sort,
                            ..Default::default()
                        }
                    )
                ),
                expected
            );
            assert_eq!(
                apply_prs(
                    &prs,
                    &PrFilter {
                        sort,
                        ..Default::default()
                    },
                    |_| PrDetailAvailability::Missing
                )
                .iter()
                .map(|&index| prs[index].number)
                .collect::<Vec<_>>(),
                expected
            );
        }
    }
    assert_eq!(
        rows.iter().map(|row| row.number).collect::<Vec<_>>(),
        vec![3, 1, 2]
    );
}

#[test]
fn equal_keys_are_stable_and_missing_timestamps_stay_last_in_both_directions() {
    let mut rows = vec![issue(4), issue(9), issue(2), issue(1)];
    for row in &mut rows[1..] {
        row.updated_at = "2026-01-01T00:00:00Z".into();
    }
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        let filter = IssueFilter {
            sort: ListSort {
                field: SortField::Updated,
                direction,
            },
            ..Default::default()
        };
        assert_eq!(
            numbers(&rows, apply_issues(&rows, &filter)),
            vec![9, 2, 1, 4]
        );
    }
}
