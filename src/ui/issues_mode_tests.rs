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
