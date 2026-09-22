//! Regression tests for the Issue-list transitions in `github_issue_state.rs`
//! (`#[path]`-attached test module of that file, split out for the LOC budget).

use super::*;
use kagi_domain::github::{Issue, IssueListTab, IssueState};
use kagi_domain::list_filter::StateFilter;

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

fn visible_evidence(state: &TabUiState, login: Option<&str>) -> (Vec<u64>, usize) {
    let view = state.issue_view(login);
    let numbers = view
        .order
        .iter()
        .map(|&index| state.github_issues[index].number)
        .collect();
    let active_count = view.tab_counts[state.github_issue_tab.index()];
    (numbers, active_count)
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
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1], 1),
        "the retained successful rows remain visible during a retry"
    );
    assert!(state
        .finish_github_issues_request(failed, Err(PrFetchError::Auth("login required".into()))));
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1], 1),
        "an error completion preserves the visible evidence it did not replace"
    );
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
fn accepted_same_length_refresh_updates_cached_filter_membership() {
    let mut state = with_first_page("github.com/a/one", None);
    state.github_issue_filter.common.text = "one".into();

    let refresh = state.begin_github_issues_request();
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1], 1),
        "the retained rows remain visible while their refresh is in flight"
    );

    assert!(state.finish_github_issues_request(
        refresh,
        Ok(snapshot(
            vec![issue(1, "renamed"), issue(2, "two")],
            vec![1],
            "github.com/a/one",
            None,
        )),
    ));
    assert_eq!(
        visible_evidence(&state, None),
        (Vec::new(), 0),
        "an accepted replacement with the same length must refresh title-filter membership"
    );
}

#[test]
fn accepted_mention_only_append_updates_cached_tab_membership() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    state.github_issue_tab = IssueListTab::MentioningMe;
    assert_eq!(visible_evidence(&state, None), (vec![1], 1));

    let (generation, cursor, _) = state.begin_github_issues_page_request().unwrap();
    assert!(state.finish_github_issues_page_request(
        generation,
        &cursor,
        Ok(snapshot(
            vec![issue(2, "duplicate")],
            vec![2],
            "github.com/a/one",
            None,
        )),
    ));
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1, 2], 2),
        "a page can change the visible mention tab without appending an issue row"
    );
}

#[test]
fn stale_refresh_keeps_cached_visible_evidence() {
    let mut state = with_first_page("github.com/a/one", None);
    state.github_issue_tab = IssueListTab::MentioningMe;
    let stale = state.begin_github_issues_request();
    let _current = state.begin_github_issues_request();
    assert_eq!(visible_evidence(&state, None), (vec![1], 1));

    assert!(!state.finish_github_issues_request(
        stale,
        Ok(snapshot(
            vec![issue(7, "stale")],
            vec![7],
            "github.com/a/stale",
            None,
        )),
    ));
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1], 1),
        "a rejected completion cannot replace the rows or mention count already on screen"
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
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1, 2], 2),
        "the retained first page stays visible while its refresh is in flight"
    );
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
    assert_eq!(
        visible_evidence(&state, None),
        (vec![1, 2], 2),
        "a detached page completion cannot replace the cached visible evidence"
    );
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

/// #753: the state a list was fetched with belongs to that list, not to the
/// strip. Moving the chip leaves the rows on screen — and any append that
/// extends them — on the collection they came from; only the refresh that
/// follows adopts the new state, and it is what refuses the earlier one's
/// page.
#[test]
fn a_refreshed_state_supersedes_the_page_of_the_previous_one() {
    let mut state = with_first_page("github.com/a/one", Some("c1"));
    assert_eq!(
        state.github_issues_request_state,
        StateFilter::Open,
        "a fresh tab fetches the open list"
    );

    state.github_issue_filter.common.state = StateFilter::Closed;
    let (page_gen, cursor, _) = state
        .begin_github_issues_page_request()
        .expect("a settled list still pages while the strip moves");
    assert_eq!(
        state.github_issues_request_state,
        StateFilter::Open,
        "the append continues the collection on screen, not the one just picked"
    );

    let refresh = state.begin_github_issues_request();
    assert_eq!(
        state.github_issues_request_state,
        StateFilter::Closed,
        "the refresh is what adopts the new state"
    );
    assert!(
        !state.finish_github_issues_page_request(
            page_gen,
            &cursor,
            Ok(snapshot(
                vec![issue(3, "an open row")],
                vec![3],
                "github.com/a/one",
                Some("c2")
            ))
        ),
        "the previous state's page cannot mix into the list replacing it"
    );

    assert!(state.finish_github_issues_request(
        refresh,
        Ok(snapshot(
            vec![issue(7, "a closed row")],
            Vec::new(),
            "github.com/a/one",
            None
        ))
    ));
    assert_eq!(numbers(&state), vec![7]);
    assert_eq!(state.github_issue_mentions, Vec::<u64>::new());
    assert_eq!(state.github_issues_request_state, StateFilter::Closed);
}

/// The UI default (`Open`) and the domain default (`All`, the predicate that
/// hides nothing) are deliberately different values, so "untouched" cannot be
/// spelled as `Default::default()` here: a tab nobody touched is pristine,
/// and a picked state is intent the next tab must not inherit.
#[test]
fn the_open_default_is_pristine_but_a_picked_state_is_not() {
    let mut state = TabUiState::default();
    assert_eq!(state.is_pristine(), Ok(()));

    state.github_issue_filter.common.state = StateFilter::All;
    assert_eq!(state.is_pristine(), Err("github_issue_filter"));

    let mut state = TabUiState::default();
    state.github_pr_filter.common.labels.push("bug".into());
    assert_eq!(state.is_pristine(), Err("github_pr_filter"));
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
