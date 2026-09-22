//! Regression tests for the Issue-list transitions in `github_issue_state.rs`
//! (`#[path]`-attached test module of that file, split out for the LOC budget).

use super::*;
use kagi_domain::github::{Issue, IssueState};

fn issue(number: u64, title: &str) -> Issue {
    Issue {
        number,
        title: title.into(),
        state: IssueState::Open,
        url: format!("https://github.com/acme/widgets/issues/{number}"),
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

fn snapshot(
    issues: Vec<Issue>,
    mentioned_numbers: Vec<u64>,
    base_repo: &str,
    next_cursor: Option<&str>,
) -> IssueListSnapshot {
    IssueListSnapshot {
        issues,
        mentioned_numbers,
        base_repo: base_repo.into(),
        next_cursor: next_cursor.map(str::to_owned),
    }
}

/// A state that has accepted page 1 of `base_repo`: issues 1 and 2, mention
/// membership `[1]`, and `next_cursor`.
fn with_first_page(base_repo: &str, next_cursor: Option<&str>) -> TabUiState {
    let mut state = TabUiState::default();
    let generation = state.begin_github_issues_request();
    assert!(state.finish_github_issues_request(
        generation,
        Ok(snapshot(
            vec![issue(1, "one"), issue(2, "two")],
            vec![1],
            base_repo,
            next_cursor,
        )),
    ));
    state
}

fn numbers(state: &TabUiState) -> Vec<u64> {
    state
        .github_issues
        .iter()
        .map(|issue| issue.number)
        .collect()
}

#[test]
fn empty_success_is_loaded_but_failure_keeps_last_success() {
    let mut state = TabUiState::default();
    let first = state.begin_github_issues_request();
    assert!(state.finish_github_issues_request(
        first,
        Ok(snapshot(
            vec![issue(1, "kept")],
            vec![1],
            "github.com/a/one",
            None
        ))
    ));

    let failed = state.begin_github_issues_request();
    assert!(state
        .finish_github_issues_request(failed, Err(PrFetchError::Auth("login required".into()))));
    assert_eq!(state.github_issues[0].number, 1);
    assert!(state.github_issues_loaded);
    assert!(state
        .github_issues_error
        .as_deref()
        .is_some_and(|error| error.contains("login required")));
    assert_eq!(state.github_issue_mentions, vec![1]);
    assert_eq!(
        state.issue_composer.base_repo.as_deref(),
        Some("github.com/a/one"),
        "a failed refresh keeps the frozen write destination"
    );

    let empty = state.begin_github_issues_request();
    assert!(state.finish_github_issues_request(
        empty,
        Ok(snapshot(Vec::new(), vec![], "github.com/a/one", None))
    ));
    assert!(state.github_issues.is_empty());
    assert!(state.github_issues_loaded);
    assert!(state.github_issues_error.is_none());
}

#[test]
fn later_list_request_rejects_delayed_completion() {
    let mut state = TabUiState::default();
    let old = state.begin_github_issues_request();
    let new = state.begin_github_issues_request();
    assert!(state.finish_github_issues_request(
        new,
        Ok(snapshot(
            vec![issue(2, "new")],
            vec![2],
            "github.com/a/new",
            None
        ))
    ));
    assert!(!state.finish_github_issues_request(
        old,
        Ok(snapshot(
            vec![issue(1, "stale")],
            vec![1],
            "github.com/a/stale",
            Some("stale-cursor")
        ))
    ));
    assert_eq!(state.github_issues[0].number, 2);
    assert_eq!(state.github_issue_mentions, vec![2]);
    assert_eq!(
        state.issue_composer.base_repo.as_deref(),
        Some("github.com/a/new")
    );
    assert!(
        state.github_issues_cursor.is_none(),
        "a superseded response cannot install its cursor either"
    );
}

#[test]
fn list_identity_wait_and_refresh_failure_preserve_restored_draft() {
    let mut state = TabUiState::default();
    state.issue_composer.editors.insert(
        None,
        crate::ui::issues_composer::IssueEditor {
            draft: kagi_domain::issue_composer::IssueDraft {
                title: "restored".into(),
                body: "keep me".into(),
                revision: 3,
            },
            loaded: true,
            ..Default::default()
        },
    );
    let failed = state.begin_github_issues_request();
    assert!(state.issue_composer.repo_loading);
    assert!(state.issue_composer.base_repo.is_none());
    assert!(
        state.finish_github_issues_request(failed, Err(PrFetchError::Network("offline".into())))
    );
    assert_eq!(
        state.issue_composer.editors[&None].draft.body.as_str(),
        "keep me"
    );
    assert!(state.issue_composer.base_repo.is_none());

    let success = state.begin_github_issues_request();
    assert!(state.finish_github_issues_request(
        success,
        Ok(snapshot(Vec::new(), Vec::new(), "github.com/a/repo", None))
    ));
    assert_eq!(
        state.issue_composer.editors[&None].draft.body.as_str(),
        "keep me",
        "accepting the frozen destination never rewrites a restored draft"
    );
    assert_eq!(
        state.issue_composer.base_repo.as_deref(),
        Some("github.com/a/repo")
    );
}

#[test]
fn pages_append_until_the_final_page_ends_pagination() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    let (generation, cursor, base_repo) = state
        .begin_github_issues_page_request()
        .expect("a loaded list with a cursor can page");
    assert_eq!(cursor, "c1");
    assert_eq!(
        base_repo, "github.com/a/one",
        "the page is fetched against the destination frozen with the list"
    );
    assert!(state.github_issues_loading_more);

    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Ok(snapshot(
            vec![issue(3, "three")],
            vec![3],
            "github.com/a/one",
            Some("c2")
        ))
    ));
    assert_eq!(numbers(&state), vec![1, 2, 3]);
    assert_eq!(state.github_issue_mentions, vec![1, 3]);
    assert_eq!(state.github_issues_cursor.as_deref(), Some("c2"));
    assert!(!state.github_issues_loading_more);

    assert!(
        !state.finish_github_issues_page_request(
            generation,
            "c1",
            Ok(snapshot(
                vec![issue(3, "three")],
                vec![3],
                "github.com/a/one",
                Some("c2")
            ))
        ),
        "a replayed delivery of an already-merged position is refused"
    );
    assert_eq!(numbers(&state), vec![1, 2, 3]);

    let (generation, cursor, _) = state
        .begin_github_issues_page_request()
        .expect("the advanced cursor pages again");
    assert_eq!(cursor, "c2");
    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Ok(snapshot(
            vec![issue(4, "four")],
            Vec::new(),
            "github.com/a/one",
            None
        ))
    ));
    assert_eq!(numbers(&state), vec![1, 2, 3, 4]);
    assert_eq!(
        state.github_issue_mentions,
        vec![1, 3],
        "a page that mentions nothing does not un-mention earlier pages"
    );
    assert!(state.github_issues_cursor.is_none());
    assert!(state.github_issues_error.is_none());
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "the final page ends pagination"
    );
}

#[test]
fn overlapping_page_appends_only_unseen_numbers() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    let (generation, cursor, _) = state.begin_github_issues_page_request().expect("can page");
    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Ok(snapshot(
            vec![issue(2, "two moved"), issue(3, "three")],
            vec![2, 1],
            "github.com/a/one",
            None
        ))
    ));
    assert_eq!(
        numbers(&state),
        vec![1, 2, 3],
        "an overlapping page neither duplicates nor reorders the rows on screen"
    );
    assert_eq!(
        state.github_issues[1].title.as_str(),
        "two",
        "the row already on screen keeps its value"
    );
    assert_eq!(state.github_issue_mentions, vec![1, 2]);
}

#[test]
fn page_failure_keeps_rows_and_cursor_and_stays_retryable() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    let (generation, cursor, _) = state.begin_github_issues_page_request().expect("can page");
    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Err(PrFetchError::Network("offline".into()))
    ));
    assert_eq!(numbers(&state), vec![1, 2]);
    assert!(state.github_issues_loaded);
    assert!(!state.github_issues_loading_more);
    assert_eq!(
        state.github_issues_cursor.as_deref(),
        Some("c1"),
        "the failed position is still the next position"
    );
    assert!(state
        .github_issues_error
        .as_deref()
        .is_some_and(|error| error.contains("offline")));

    let (retry_gen, retry_cursor, _) = state
        .begin_github_issues_page_request()
        .expect("a failed append is retryable from the same cursor");
    assert_eq!((retry_gen, retry_cursor.as_str()), (generation, "c1"));
    assert!(
        state.github_issues_error.is_none(),
        "the retry clears the gate that stopped the automatic load"
    );
    assert!(state.finish_github_issues_page_request(
        retry_gen,
        &retry_cursor,
        Ok(snapshot(
            vec![issue(3, "three")],
            Vec::new(),
            "github.com/a/one",
            None
        ))
    ));
    assert_eq!(numbers(&state), vec![1, 2, 3]);
}

#[test]
fn refresh_supersedes_an_in_flight_page() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    let (page_gen, page_cursor, _) = state
        .begin_github_issues_page_request()
        .expect("an append is in flight");

    let refresh = state.begin_github_issues_request();
    assert!(!state.github_issues_loading_more);
    assert!(
        state.github_issues_cursor.is_none(),
        "a refresh restarts pagination from the first page"
    );
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "there is nothing to append to while the first page is loading"
    );

    assert!(
        !state.finish_github_issues_page_request(
            page_gen,
            &page_cursor,
            Ok(snapshot(
                vec![issue(9, "late page")],
                vec![9],
                "github.com/a/one",
                Some("c2")
            ))
        ),
        "the superseded append cannot extend a list that is being replaced"
    );
    assert_eq!(numbers(&state), vec![1, 2]);
    assert_eq!(state.github_issue_mentions, vec![1]);
    assert!(state.github_issues_cursor.is_none());

    assert!(state.finish_github_issues_request(
        refresh,
        Ok(snapshot(
            vec![issue(5, "fresh")],
            Vec::new(),
            "github.com/a/one",
            Some("c9")
        ))
    ));
    assert_eq!(numbers(&state), vec![5]);
    assert_eq!(state.github_issues_cursor.as_deref(), Some("c9"));
}

#[test]
fn page_from_another_repository_is_refused_but_retryable() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    let (generation, cursor, _) = state.begin_github_issues_page_request().expect("can page");
    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Ok(snapshot(
            vec![issue(3, "someone else's issue")],
            vec![3],
            "github.com/a/other",
            Some("c2")
        ))
    ));
    assert_eq!(
        numbers(&state),
        vec![1, 2],
        "a page of another repository is never merged"
    );
    assert_eq!(state.github_issue_mentions, vec![1]);
    assert_eq!(
        state.issue_composer.base_repo.as_deref(),
        Some("github.com/a/one"),
        "the frozen write destination is not moved by a foreign answer"
    );
    assert!(state
        .github_issues_error
        .as_deref()
        .is_some_and(|error| error.contains("github.com/a/other")));
    assert_eq!(state.github_issues_cursor.as_deref(), Some("c1"));
    let (_, retry_cursor, base_repo) = state.begin_github_issues_page_request().expect("retry");
    assert_eq!(retry_cursor, "c1");
    assert_eq!(base_repo, "github.com/a/one");
    assert!(state.github_issues_error.is_none());
}

#[test]
fn page_requests_need_a_settled_list_and_never_overlap() {
    let mut state = TabUiState::default();
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "a tab that never loaded Issues has nothing to append to"
    );

    let refresh = state.begin_github_issues_request();
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "no append while the first page is loading"
    );
    assert!(state.finish_github_issues_request(
        refresh,
        Ok(snapshot(
            vec![issue(1, "one")],
            Vec::new(),
            "github.com/a/one",
            Some("c1")
        ))
    ));

    assert!(state.begin_github_issues_page_request().is_some());
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "the append slot admits one request"
    );

    state.github_issues_loading_more = false;
    state.issue_composer.base_repo = None;
    assert!(
        state.begin_github_issues_page_request().is_none(),
        "a page cannot be fetched without the destination frozen with the list"
    );
}

#[test]
fn page_state_is_per_owner_and_pages_land_only_in_their_own_list() {
    let mut owner_a = with_first_page("github.com/a/one", Some("c1"));
    let mut owner_b = with_first_page("github.com/b/two", Some("c1"));
    let (a_gen, a_cursor, a_repo) = owner_a.begin_github_issues_page_request().expect("a pages");
    let (b_gen, b_cursor, b_repo) = owner_b.begin_github_issues_page_request().expect("b pages");
    assert_eq!(
        (a_gen, a_cursor.as_str()),
        (b_gen, b_cursor.as_str()),
        "two tabs reach the same generation and cursor: only the owner separates them"
    );
    assert_eq!(a_repo, "github.com/a/one");
    assert_eq!(b_repo, "github.com/b/two");

    assert!(owner_b.finish_github_issues_page_request(
        b_gen,
        &b_cursor,
        Ok(snapshot(
            vec![issue(30, "b three")],
            vec![30],
            "github.com/b/two",
            None
        ))
    ));
    assert_eq!(numbers(&owner_b), vec![1, 2, 30]);
    assert_eq!(numbers(&owner_a), vec![1, 2]);
    assert!(
        owner_a.github_issues_loading_more,
        "another owner's completion does not settle this append"
    );
    assert_eq!(owner_a.github_issues_cursor.as_deref(), Some("c1"));

    assert!(owner_a.finish_github_issues_page_request(
        a_gen,
        &a_cursor,
        Ok(snapshot(
            vec![issue(30, "b three")],
            vec![30],
            "github.com/b/two",
            None
        ))
    ));
    assert_eq!(
        numbers(&owner_a),
        vec![1, 2],
        "a misrouted page is caught by the frozen destination, not by the generation"
    );
}

#[test]
fn pagination_state_is_not_pristine() {
    let mut state = TabUiState::default();
    assert_eq!(state.is_pristine(), Ok(()));
    state.github_issues_cursor = Some("c1".into());
    assert_eq!(
        state.is_pristine(),
        Err("issues_mode"),
        "a tab holding a page cursor is not one nobody touched"
    );

    let mut state = TabUiState::default();
    state.github_issues_loading_more = true;
    assert_eq!(state.is_pristine(), Err("issues_mode"));
}

#[test]
fn owner_states_and_detail_generations_are_independent() {
    let mut owner_a = TabUiState::default();
    let mut owner_b = TabUiState::default();
    let a = owner_a.begin_github_issues_request();
    let b = owner_b.begin_github_issues_request();
    assert!(owner_b.finish_github_issues_request(
        b,
        Ok(snapshot(vec![issue(20, "B")], Vec::new(), "b/r", None))
    ));
    assert!(owner_a.finish_github_issues_request(
        a,
        Ok(snapshot(vec![issue(10, "A")], Vec::new(), "a/r", None))
    ));
    assert_eq!(owner_a.github_issues[0].number, 10);
    assert_eq!(owner_b.github_issues[0].number, 20);

    let old = owner_a.begin_github_issue_detail_request(10);
    let newest = owner_a.begin_github_issue_detail_request(11);
    assert!(!owner_a.finish_github_issue_detail_request(old, 10, Ok(issue(10, "old"))));
    assert!(owner_a.finish_github_issue_detail_request(
        newest,
        11,
        Err(PrFetchError::NotFound("gone".into()))
    ));
    assert_eq!(owner_a.selected_github_issue, Some(11));
    assert!(owner_a.github_issue_details.is_empty());
    assert!(owner_a
        .github_issue_detail_error
        .as_deref()
        .is_some_and(|error| error.contains("gone")));

    let successful = owner_a.begin_github_issue_detail_request(11);
    assert!(owner_a.finish_github_issue_detail_request(successful, 11, Ok(issue(11, "cached"))));
    let failed = owner_a.begin_github_issue_detail_request(11);
    assert!(owner_a.finish_github_issue_detail_request(
        failed,
        11,
        Err(PrFetchError::Network("offline".into()))
    ));
    assert_eq!(
        owner_a
            .github_issue_details
            .get(&11)
            .map(|issue| issue.title.as_str()),
        Some("cached"),
        "detail failure keeps the last successful value"
    );
}

#[test]
fn returning_home_invalidates_detail_without_dropping_cache_or_reply() {
    let mut state = TabUiState::default();
    let generation = state.begin_github_issue_detail_request(7);
    state.github_issue_details.insert(7, issue(7, "cached"));
    state
        .issue_composer
        .editors
        .insert(Some(7), Default::default());
    state.clear_github_issue_selection();
    assert_eq!(state.selected_github_issue, None);
    assert_eq!(state.github_issue_detail_loading, None);
    assert!(state.github_issue_detail_error.is_none());
    assert!(state.github_issue_details.contains_key(&7));
    assert!(state.issue_composer.editors.contains_key(&Some(7)));
    assert!(!state.finish_github_issue_detail_request(generation, 7, Ok(issue(7, "late"))));
    assert_eq!(state.selected_github_issue, None);
}
