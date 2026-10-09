use super::super::pr_mode::PrModeState;
use super::*;

const REPO: &str = "ghe.example/acme/widgets";

fn row(number: u64, state: IssueState) -> PullRequest {
    PullRequest {
        number,
        state,
        title: format!("PR {number}"),
        head_sha: format!("head-{number}"),
        base_repo: REPO.into(),
        url: format!("https://{REPO}/pull/{number}"),
        ..Default::default()
    }
}

fn page(numbers: std::ops::Range<u64>, state: IssueState, cursor: Option<&str>) -> PrListSnapshot {
    PrListSnapshot {
        prs: numbers.map(|number| row(number, state)).collect(),
        base_repo: REPO.into(),
        next_cursor: cursor.map(str::to_owned),
    }
}

fn open() -> TabUiState {
    let mut ui = TabUiState {
        pr_mode: Some(PrModeState::default()),
        ..Default::default()
    };
    let generation = ui.begin_github_prs_request();
    assert!(ui
        .finish_github_prs_request(
            generation,
            Ok(page(1..101, IssueState::Open, Some("  null  ")))
        )
        .unwrap()
        .error
        .is_none());
    ui
}

#[test]
fn bounded_pages_append_one_hundred_then_twenty_and_reject_duplicate_delivery() {
    let mut ui = open();
    let epoch = ui.github_prs_epoch;
    let request = ui.begin_pr_page_request().unwrap();
    assert_eq!(request.cursor, "  null  ");
    assert_eq!(request.base_repo, REPO);
    assert_eq!(request.state, StateFilter::Open);
    assert!(ui.begin_pr_page_request().is_none(), "one append slot");
    let final_page = page(101..121, IssueState::Open, None);
    assert!(ui.finish_pr_page_request(&request, Ok(final_page.clone())));
    assert_eq!(ui.github_prs.len(), 120);
    assert_eq!(ui.github_prs.last().unwrap().number, 120);
    assert_eq!(
        ui.github_prs_gen, request.generation,
        "append is not refresh"
    );
    assert_ne!(
        ui.github_prs_epoch, epoch,
        "old projections cannot index a new collection"
    );
    assert!(!ui.pr_list_has_more());
    assert!(ui.begin_pr_page_request().is_none());
    assert!(!ui.finish_pr_page_request(&request, Ok(final_page)));
    assert_eq!(ui.github_prs.len(), 120);
}

#[test]
fn a_real_empty_final_page_settles_without_dropping_rows() {
    let mut ui = open();
    let request = ui.begin_pr_page_request().unwrap();
    assert!(ui.finish_pr_page_request(&request, Ok(page(0..0, IssueState::Open, None))));
    assert_eq!(ui.github_prs.len(), 100);
    assert!(!ui.pr_list_has_more());
}

#[test]
fn overlapping_keys_keep_existing_position_and_details() {
    let mut ui = open();
    ui.github_prs[99].body = "already read body".into();
    ui.github_prs[99].changed_files = 42;
    let original = ui.github_prs[99].clone();
    let request = ui.begin_pr_page_request().unwrap();
    let mut next = page(100..121, IssueState::Open, None);
    next.prs[0].title = "overlap must not replace existing value".into();
    next.prs.push(row(120, IssueState::Open));
    assert!(ui.finish_pr_page_request(&request, Ok(next)));
    assert_eq!(ui.github_prs.len(), 120);
    assert_eq!(ui.github_prs[99], original);
}

#[test]
fn failed_append_preserves_cursor_and_retry_refuses_an_older_attempt() {
    let mut ui = open();
    let before = ui.github_prs.clone();
    let request = ui.begin_pr_page_request().unwrap();
    assert!(ui.finish_pr_page_request(&request, Err(PrFetchError::Network("offline".into()))));
    assert_eq!(ui.github_prs, before);
    assert_eq!(ui.github_prs_paging.cursor.as_deref(), Some("  null  "));
    assert!(ui.github_error.is_some());
    let retry = ui.begin_pr_page_request().unwrap();
    assert_eq!(retry.cursor, request.cursor);
    assert_ne!(retry.attempt, request.attempt);
    assert!(ui.github_error.is_none());
    assert!(!ui.finish_pr_page_request(&request, Ok(page(101..121, IssueState::Open, None))));
    assert!(ui.pr_list_loading_more());
    assert!(ui.finish_pr_page_request(&retry, Ok(page(101..121, IssueState::Open, None))));
    assert_eq!(ui.github_prs.len(), 120);
}

#[test]
fn invalid_metadata_repository_and_state_keep_the_retry_position() {
    let mut ui = open();
    let mut foreign = page(101..121, IssueState::Open, None);
    foreign.base_repo = "github.com/other/repository".into();
    let mut foreign_row = page(101..121, IssueState::Open, None);
    foreign_row.prs[0].base_repo = "github.com/other/repository".into();
    for snapshot in [
        foreign,
        foreign_row,
        page(101..121, IssueState::Closed, None),
        page(101..121, IssueState::Open, Some("  null  ")),
        page(101..121, IssueState::Open, Some(" ")),
        page(0..0, IssueState::Open, Some("new-cursor")),
    ] {
        let request = ui.begin_pr_page_request().unwrap();
        assert!(ui.finish_pr_page_request(&request, Ok(snapshot)));
        assert_eq!(ui.github_prs.len(), 100);
        assert_eq!(ui.github_prs_paging.cursor.as_deref(), Some("  null  "));
        assert!(ui.github_error.is_some());
    }
}

#[test]
fn refresh_replaces_the_generation_and_a_failed_refresh_revokes_continuation() {
    let mut ui = open();
    let old = ui.begin_pr_page_request().unwrap();
    let generation = ui.begin_github_prs_request();
    assert!(!ui.finish_pr_page_request(&old, Ok(page(101..121, IssueState::Open, None))));
    assert!(ui.begin_pr_page_request().is_none());
    let outcome = ui
        .finish_github_prs_request(generation, Err(PrFetchError::Auth("expired".into())))
        .unwrap();
    assert!(outcome.error.is_some());
    assert_eq!(ui.github_prs.len(), 100);
    assert!(ui.github_prs_paging.cursor.is_none());
    assert!(ui.begin_pr_page_request().is_none());
    let obsolete = ui.begin_github_prs_request();
    let newest = ui.begin_github_prs_request();
    assert!(ui
        .finish_github_prs_request(obsolete, Ok(page(2..3, IssueState::Open, None)))
        .is_none());
    assert!(ui.github_prs_loading);
    assert!(ui
        .finish_github_prs_request(newest, Ok(page(9..10, IssueState::Open, None)))
        .is_some());
    assert_eq!(ui.github_prs[0].number, 9);
}

#[test]
fn closed_all_and_open_have_separate_membership_and_requests() {
    let mut ui = open();
    let shared = ui.github_prs.clone();
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::Closed;
    let generation = ui.begin_pr_strip_request(StateFilter::Closed);
    assert!(ui
        .finish_pr_strip_request(
            generation,
            StateFilter::Closed,
            Ok(page(200..300, IssueState::Closed, Some("closed-next")))
        )
        .unwrap()
        .error
        .is_none());
    let old = ui.begin_pr_page_request().unwrap();
    assert_eq!(old.state, StateFilter::Closed);
    assert_eq!(old.cursor, "closed-next");
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::All;
    let all = ui.begin_pr_strip_request(StateFilter::All);
    let mut snapshot = page(4..6, IssueState::Closed, Some("all-next"));
    snapshot.prs.push(row(1, IssueState::Open));
    assert!(ui
        .finish_pr_strip_request(all, StateFilter::All, Ok(snapshot))
        .unwrap()
        .error
        .is_none());
    assert!(!ui.finish_pr_page_request(&old, Ok(page(300..320, IssueState::Closed, None))));
    assert_eq!(ui.github_prs, shared);
    assert_eq!(ui.pr_list_rows().len(), 3);
    let request = ui.begin_pr_page_request().unwrap();
    assert_eq!(request.state, StateFilter::All);
    assert!(ui.finish_pr_page_request(&request, Ok(page(6..7, IssueState::Closed, None))));
    assert_eq!(ui.pr_list_rows().len(), 4);
    assert_eq!(ui.github_prs, shared);
}

#[test]
fn strip_failed_refresh_keeps_rows_without_authorizing_an_old_page() {
    let mut ui = open();
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::Closed;
    let generation = ui.begin_pr_strip_request(StateFilter::Closed);
    ui.finish_pr_strip_request(
        generation,
        StateFilter::Closed,
        Ok(page(2..4, IssueState::Closed, Some("closed-next"))),
    )
    .unwrap();
    let old = ui.begin_pr_page_request().unwrap();
    let refresh = ui.begin_pr_strip_request(StateFilter::Closed);
    assert!(!ui.finish_pr_page_request(&old, Ok(page(4..5, IssueState::Closed, None))));
    ui.finish_pr_strip_request(
        refresh,
        StateFilter::Closed,
        Err(PrFetchError::Network("offline".into())),
    )
    .unwrap();
    assert_eq!(ui.pr_list_rows().len(), 2);
    assert!(ui.pr_list_error().is_some());
    assert!(!ui.pr_list_has_more());
    assert!(ui.begin_pr_page_request().is_none());
}

#[test]
fn departure_and_revisit_refuse_the_old_append() {
    let mut ui = open();
    let request = ui.begin_pr_page_request().unwrap();
    ui.leave_pr_mode();
    ui.pr_mode = Some(PrModeState::default());
    assert!(!ui.finish_pr_page_request(&request, Ok(page(101..121, IssueState::Open, None))));
    assert_eq!(ui.github_prs.len(), 100);
    let current = ui.begin_pr_page_request().unwrap();
    assert_ne!(request.visit, current.visit);
    assert!(ui.finish_pr_page_request(&current, Ok(page(101..121, IssueState::Open, None))));
}

#[test]
fn strip_inherits_loaded_details_only_for_same_repository_number_and_head() {
    let mut ui = open();
    ui.github_prs[0].body = "loaded description".into();
    ui.github_prs[0].changed_files = 123;
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::All;
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    ui.finish_pr_strip_request(
        generation,
        StateFilter::All,
        Ok(page(1..2, IssueState::Open, Some("all-next"))),
    )
    .unwrap();
    assert_eq!(ui.pr_list_rows()[0].body, "loaded description");
    assert_eq!(ui.pr_list_rows()[0].changed_files, 123);
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    let mut changed = page(1..2, IssueState::Open, None);
    changed.prs[0].head_sha = "new-head".into();
    ui.finish_pr_strip_request(generation, StateFilter::All, Ok(changed))
        .unwrap();
    assert_eq!(ui.pr_list_rows()[0].body, "");
    assert_eq!(ui.pr_list_rows()[0].changed_files, 0);
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    let mut other = page(1..2, IssueState::Open, None);
    other.base_repo = "github.com/another/widgets".into();
    other.prs[0].base_repo.clone_from(&other.base_repo);
    ui.finish_pr_strip_request(generation, StateFilter::All, Ok(other))
        .unwrap();
    assert_eq!(ui.pr_list_rows()[0].body, "");
}

#[test]
fn only_client_membership_predicates_suppress_automatic_paging() {
    let mut ui = open();
    assert!(!ui.pr_client_membership_active());
    ui.github_pr_filter.sort.direction = kagi_domain::list_filter::SortDirection::Ascending;
    assert!(!ui.pr_client_membership_active());
    ui.github_pr_filter.common.text = "missing in loaded rows".into();
    assert!(ui.pr_client_membership_active());
    ui.github_pr_filter.common.text.clear();
    ui.github_pr_filter.draft = DraftFilter::Draft;
    assert!(ui.pr_client_membership_active());
    ui.github_pr_filter.draft = DraftFilter::All;
    ui.github_pr_filter.checks = ChecksFilter::Passing;
    assert!(ui.pr_client_membership_active());
    assert_eq!(ui.github_prs.len(), 100);
    assert!(ui.pr_list_has_more());
}

#[test]
fn appended_strip_rows_inherit_detail_for_same_head_but_not_changed_head() {
    let mut ui = open();
    for cached in &mut ui.github_prs[1..3] {
        cached.ci = kagi_domain::github::CiState::Success;
        cached.mergeable = kagi_domain::github::Mergeable::Clean;
        cached.body = "loaded body".into();
        cached.changed_files = 40;
    }
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::All;
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    ui.finish_pr_strip_request(
        generation,
        StateFilter::All,
        Ok(page(1..2, IssueState::Open, Some("all-next"))),
    )
    .unwrap();
    let request = ui.begin_pr_page_request().unwrap();
    let mut next = page(2..4, IssueState::Open, None);
    next.prs[1].head_sha = "changed-head".into();
    assert!(ui.finish_pr_page_request(&request, Ok(next)));
    let rows = ui.pr_list_rows();
    assert_eq!(rows[1].body, "loaded body");
    assert_eq!(rows[1].ci, kagi_domain::github::CiState::Success);
    assert_eq!(rows[1].changed_files, 40);
    assert_eq!(rows[2].body, "");
    assert_eq!(rows[2].ci, kagi_domain::github::CiState::None);
    assert_eq!(rows[2].changed_files, 0);
}

#[test]
fn interrupted_selected_first_read_keeps_intent_and_rejects_obsolete_completion() {
    for state in [StateFilter::Closed, StateFilter::All] {
        let mut ui = open();
        ui.reset_pr_strip();
        ui.github_pr_filter.common.state = state;
        let old = ui.begin_pr_strip_request(state);
        ui.invalidate_pr_list_visit();
        assert_eq!(ui.github_pr_filter.common.state, state);
        assert!(ui.pr_list_rows().is_empty());
        assert!(
            ui.pr_list_loading(),
            "unobserved membership stays pending, not authoritative zero"
        );
        assert!(ui
            .finish_pr_strip_request(old, state, Ok(page(200..201, IssueState::Closed, None)),)
            .is_none());
        let current = ui.begin_pr_strip_request(state);
        assert_ne!(old, current);
        assert!(ui
            .finish_pr_strip_request(old, state, Ok(page(200..201, IssueState::Closed, None)),)
            .is_none());
        ui.finish_pr_strip_request(current, state, Ok(page(300..301, IssueState::Closed, None)))
            .unwrap();
        assert_eq!(ui.pr_list_rows()[0].number, 300);
        assert!(!ui.pr_list_loading());
        assert_eq!(ui.github_prs.len(), 100);
    }
}

#[test]
fn shared_first_page_inherits_retained_strip_payload_only_for_the_same_head() {
    use kagi_domain::github::{Check, CiState};
    let mut ui = open();
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::All;
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    ui.finish_pr_strip_request(
        generation,
        StateFilter::All,
        Ok(page(120..122, IssueState::Open, None)),
    )
    .unwrap();
    for cached in ui.github_prs_strip.rows.as_mut().unwrap() {
        cached.ci = CiState::Success;
        cached.checks = vec![Check {
            name: "build".into(),
            workflow: "CI".into(),
            state: CiState::Success,
            url: String::new(),
        }];
        cached.body = "retained detail body".into();
        cached.changed_files = 42;
    }
    assert!(ui.opened_pr_keys().is_empty());
    assert!(ui
        .github_prs
        .iter()
        .all(|pr| pr.number != 120 && pr.number != 121));
    // Shared Open may gain a key whose only real payload owner is the strip.
    let mut incoming = page(120..122, IssueState::Open, None);
    incoming.prs[1].head_sha = "changed-head".into();
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(incoming))
        .unwrap();
    ui.reset_pr_strip();
    let retained = &ui.github_prs[0];
    assert_eq!(retained.number, 120);
    assert_eq!(retained.ci, CiState::Success);
    assert_eq!(retained.checks.len(), 1);
    assert_eq!(retained.checks[0].name, "build");
    assert_eq!(retained.body, "retained detail body");
    assert_eq!(retained.changed_files, 42);
    let changed = &ui.github_prs[1];
    assert_eq!(changed.number, 121);
    assert_eq!(changed.ci, CiState::None);
    assert!(changed.checks.is_empty());
    assert!(changed.body.is_empty());
    assert_eq!(changed.changed_files, 0);
}
