use super::*;
use crate::ui::TabUiState;
use kagi_domain::github::{IssueListSnapshot, IssueState};
use kagi_git::github::PrFetchError;

#[test]
fn list_status_distinguishes_loading_empty_success_and_error() {
    assert_eq!(
        list_presentation(true, false, false, 0),
        IssueListPresentation::Loading
    );
    assert_eq!(
        list_presentation(false, true, false, 0),
        IssueListPresentation::Empty
    );
    assert_eq!(
        list_presentation(false, false, true, 0),
        IssueListPresentation::Error
    );
    assert_eq!(
        list_presentation(false, true, false, 1),
        IssueListPresentation::Issues
    );
    assert_eq!(
        list_presentation(true, true, false, 1),
        IssueListPresentation::Issues
    );
    assert_eq!(
        list_presentation(true, false, false, 1),
        IssueListPresentation::Issues
    );
    assert_eq!(
        list_presentation(false, false, true, 1),
        IssueListPresentation::Issues
    );
}

fn page(number: u64, cursor: Option<&str>) -> IssueListSnapshot {
    IssueListSnapshot {
        issues: vec![Issue {
            number,
            title: format!("Issue {number}"),
            state: IssueState::Open,
            url: format!("https://github.com/example/repo/issues/{number}"),
            author: "alice".into(),
            assignees: Vec::new(),
            labels: Vec::new(),
            body: String::new(),
            comments: Vec::new(),
            comment_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }],
        mentioned_numbers: Vec::new(),
        base_repo: "github.com/example/repo".into(),
        next_cursor: cursor.map(str::to_owned),
    }
}

#[test]
fn list_presentation_keeps_appended_rows_through_page_failure_and_retry() {
    let mut ui = TabUiState::default();
    let first = ui.begin_github_issues_request();
    assert!(ui.finish_github_issues_request(first, Ok(page(1, Some("page-2")))));
    let (generation, cursor, _) = ui.begin_github_issues_page_request().unwrap();
    assert!(ui.finish_github_issues_page_request(generation, &cursor, Ok(page(2, Some("page-3")))));
    let (generation, cursor, _) = ui.begin_github_issues_page_request().unwrap();
    assert_eq!(
        list_presentation(
            ui.github_issues_loading_more,
            ui.github_issues_loaded,
            false,
            ui.github_issues.len()
        ),
        IssueListPresentation::Issues
    );
    assert!(ui.finish_github_issues_page_request(
        generation,
        &cursor,
        Err(PrFetchError::Network("offline".into()))
    ));
    assert_eq!(
        ui.github_issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-3"));
    assert_eq!(
        list_presentation(
            false,
            ui.github_issues_loaded,
            ui.github_issues_error.is_some(),
            ui.github_issues.len()
        ),
        IssueListPresentation::Issues
    );
    let (generation, retry_cursor, _) = ui.begin_github_issues_page_request().unwrap();
    assert_eq!(retry_cursor, cursor);
    assert!(ui.finish_github_issues_page_request(generation, &retry_cursor, Ok(page(3, None))));
    assert_eq!(
        ui.github_issues
            .iter()
            .map(|issue| issue.number)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(ui.github_issues_cursor.is_none());
    assert!(ui.github_issues_error.is_none());
}

fn projected_numbers(ui: &TabUiState, login: Option<&str>) -> Vec<u64> {
    ui.issue_view(login)
        .order
        .iter()
        .map(|&index| ui.github_issues[index].number)
        .collect()
}

#[test]
fn issue_projection_reuses_one_sort_for_main_and_sidebar_consumers() {
    let mut ui = TabUiState::default();
    ui.github_issues = (1..=300)
        .map(|number| page(number, None).issues.remove(0))
        .collect();
    let before = issue_view_recomputations();
    assert_eq!(projected_numbers(&ui, None), (1..=300).collect::<Vec<_>>());
    for _ in 0..4 {
        assert_eq!(ui.issue_view(None).tab_counts, [0, 0, 0, 300]);
    }
    assert_eq!(issue_view_recomputations() - before, 1);
    let before_scroll = issue_view_recomputations();
    for _ in 0..10 {
        assert_eq!(projected_numbers(&ui, None)[150], 151);
    }
    assert_eq!(issue_view_recomputations() - before_scroll, 0);

    ui.github_issue_filter.common.text = "issue 30".into();
    assert_eq!(projected_numbers(&ui, None), vec![30, 300]);
    assert_eq!(ui.issue_view(None).tab_counts, [0, 0, 0, 2]);
    assert_eq!(issue_view_recomputations() - before_scroll, 1);
}

#[test]
fn issue_projection_tracks_login_tab_mentions_and_sort_changes() {
    let mut ui = TabUiState::default();
    ui.github_issues = (1..=3)
        .map(|number| page(number, None).issues.remove(0))
        .collect();
    ui.github_issues[0].assignees = vec!["alice".into()];
    ui.github_issues[1].author = "bob".into();
    ui.github_issues[1].assignees = vec!["alice".into()];
    ui.github_issues[2].author = "bob".into();
    ui.github_issues[2].assignees = vec!["bob".into()];
    ui.github_issue_tab = IssueListTab::AssignedToMe;
    assert_eq!(projected_numbers(&ui, None), Vec::<u64>::new());
    assert_eq!(projected_numbers(&ui, Some("alice")), vec![1, 2]);
    assert_eq!(ui.issue_view(Some("alice")).tab_counts, [2, 1, 0, 3]);
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![3]);
    assert_eq!(ui.issue_view(Some("bob")).tab_counts, [1, 2, 0, 3]);

    ui.github_issue_tab = IssueListTab::CreatedByMe;
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![2, 3]);
    ui.github_issue_tab = IssueListTab::MentioningMe;
    ui.github_issue_mentions = vec![2];
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![2]);
    ui.github_issue_mentions[0] = 1;
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![1]);
    assert_eq!(ui.issue_view(Some("bob")).tab_counts, [1, 2, 1, 3]);

    ui.github_issue_tab = IssueListTab::RecentlyUpdated;
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![1, 2, 3]);
    ui.github_issue_filter.sort.field = kagi_domain::list_filter::SortField::Number;
    assert_eq!(projected_numbers(&ui, Some("bob")), vec![3, 2, 1]);
}
