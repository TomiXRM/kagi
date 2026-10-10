use super::*;
use gpui::{px, ListState};
use kagi_git::{ChangeKind, CommitId, FileStatus};

fn pr(number: u64, head: &str) -> PullRequest {
    PullRequest {
        number,
        head_sha: head.into(),
        base_repo: "github.com/acme/widgets".into(),
        ..Default::default()
    }
}

fn path() -> PathBuf {
    PathBuf::from("/repo")
}

fn tab(head: &str) -> PrTab {
    PrTab {
        pr: pr(1, head),
        local_refs_loading: false,
        local_refs_generation: 0,
        local_refs_visit: None,
        base: CommitId("base".into()),
        base_tip: CommitId("base-tip".into()),
        head: CommitId(head.into()),
        commits: Vec::new(),
        files: vec![FileStatus {
            path: "changed.rs".into(),
            change: ChangeKind::Modified,
        }],
        selected_commit: None,
        selected_file: Some(0),
        diff: None,
        diff_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
        diff_layout: Default::default(),
        feed_list: ListState::new(0, gpui::ListAlignment::Top, px(400.)),
        feed_entries: std::rc::Rc::new(Vec::new()),
        reviews: Vec::new(),
        comments: Vec::new(),
        line_comments: Vec::new(),
        conversation_loaded: true,
        comment_draft: "keep me".into(),
        conflicts: None,
        conflict_selected: None,
        conflict_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
        conflict_layout: Default::default(),
        conflict_at: 0,
        conflict_preview: None,
        merge_status: None,
        merge_status_loaded: false,
        viewed: Default::default(),
        threads: Default::default(),
    }
}

#[test]
fn closing_a_pr_cancels_status_reads_but_keeps_body_readable() {
    let mut controller = PrDetailController::default();
    let mut row = pr(1, "a");
    row.state = IssueState::Open;
    controller.enqueue(path(), &row, PrDetailStage::Status, false, false);
    row.state = IssueState::Closed;
    controller.reconcile_heads(std::slice::from_ref(&row), None, std::iter::empty());
    controller.enqueue(path(), &row, PrDetailStage::Status, true, true);
    controller.enqueue(path(), &row, PrDetailStage::Body, true, true);
    let read = controller.take_next(Instant::now()).unwrap();
    assert_eq!(read.request.key.stage, PrDetailStage::Body);
    assert!(controller.take_next(Instant::now()).is_none());
}

#[test]
fn queue_never_starts_more_than_two_or_duplicates_a_stage() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(2, "b"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(3, "c"), PrDetailStage::Status, false, false);
    let first = controller.take_next(Instant::now()).unwrap();
    let second = controller.take_next(Instant::now()).unwrap();
    assert_ne!(first.request.key, second.request.key);
    assert!(controller.take_next(Instant::now()).is_none());
    assert_eq!(controller.active.len(), 2);
}

#[test]
fn an_active_stage_is_not_queued_again_for_the_same_head() {
    let mut controller = PrDetailController::default();
    let pr = pr(1, "a");
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    let started = controller.take_next(Instant::now()).unwrap();
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    assert!(controller.pending.is_empty());

    let ok = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "a".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&started, &ok, Instant::now()), Some(true));
    assert!(controller.take_next(Instant::now()).is_none());
}

#[test]
fn offscreen_pending_rows_are_removed_but_opened_rows_stay() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "a"), PrDetailStage::Status, false, false);
    controller.enqueue(path(), &pr(2, "b"), PrDetailStage::Status, true, false);
    controller.observe_visible(
        BTreeSet::new(),
        BTreeSet::from([pr(2, "b").key()]),
        1,
        Instant::now(),
    );
    assert_eq!(controller.pending.len(), 1);
    assert_eq!(controller.pending[0].key.pr.number, 2);
}

#[test]
fn a_failed_refresh_keeps_success_as_stale_and_delays_retry() {
    let mut controller = PrDetailController::default();
    let pr = pr(1, "a");
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, false);
    let first = controller.take_next(Instant::now()).unwrap();
    let ok = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "a".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&first, &ok, Instant::now()), Some(true));
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, true);
    let refresh = controller.take_next(Instant::now()).unwrap();
    let failed = Err(kagi_git::github::PrFetchError::Network("offline".into()));
    assert_eq!(
        controller.settle(&refresh, &failed, Instant::now()),
        Some(false)
    );
    assert_eq!(
        controller.availability(&pr, PrDetailStage::Status),
        PrDetailAvailability::Stale
    );
    controller.enqueue(path(), &pr, PrDetailStage::Status, false, true);
    assert!(controller.take_next(Instant::now()).is_none());
    assert_eq!(
        controller.availability(&pr, PrDetailStage::Status),
        PrDetailAvailability::Stale,
        "retry backoff must not disguise stale data as an active load"
    );
}

#[test]
fn changed_head_invalidates_old_completion() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "old"), PrDetailStage::Status, false, false);
    let old = controller.take_next(Instant::now()).unwrap();
    controller.enqueue(path(), &pr(1, "new"), PrDetailStage::Status, false, false);
    let result = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "old".into(),
        ci: Default::default(),
        checks: Vec::new(),
        mergeable: Default::default(),
    }));
    assert_eq!(controller.settle(&old, &result, Instant::now()), None);
    assert_eq!(
        controller.availability(&pr(1, "new"), PrDetailStage::Status),
        PrDetailAvailability::Loading
    );
    assert!(controller.take_next(Instant::now()).is_some());
}

#[test]
fn response_for_a_different_head_never_marks_the_slot_fresh() {
    let mut controller = PrDetailController::default();
    controller.enqueue(path(), &pr(1, "old"), PrDetailStage::Status, false, false);
    let started = controller.take_next(Instant::now()).unwrap();
    let moved = Ok(DetailResult::Status(PrStatusDetail {
        number: 1,
        head_sha: "new".into(),
        ci: kagi_domain::github::CiState::Success,
        checks: Vec::new(),
        mergeable: kagi_domain::github::Mergeable::Clean,
    }));
    assert_eq!(
        controller.settle(&started, &moved, Instant::now()),
        Some(false)
    );
    assert_eq!(
        controller.availability(&pr(1, "old"), PrDetailStage::Status),
        PrDetailAvailability::Missing
    );
}

#[test]
fn list_head_change_invalidates_then_local_reload_restores_the_open_tab() {
    let mut tab = tab("old");
    let changed = std::path::Path::new("changed.rs");
    tab.viewed.set_head_blobs(&tab.files, vec!["a".repeat(40)]);
    tab.viewed.set(changed, true);
    assert!(tab.viewed.is_viewed(changed));
    let mut listed = pr(1, "new");
    listed.title = "new title".into();
    sync_open_pr_from_list(&mut tab, &listed);
    assert!(
        !tab.viewed.is_viewed(changed),
        "#351: the moved head's blobs are unknown until it is fetched"
    );
    assert_eq!(tab.pr.title, "new title");
    assert_eq!(tab.pr.head_sha, "new");
    assert_eq!(
        tab.head,
        CommitId("old".into()),
        "old local head triggers reload"
    );
    assert!(tab.files.is_empty());
    assert_eq!(tab.selected_file, None);
    assert_eq!(tab.comment_draft, "keep me");
    assert!(tab.conversation_loaded);

    install_local_pr_head(
        &mut tab,
        CommitId("new-base".into()),
        CommitId("new-base-tip".into()),
        CommitId("new".into()),
        Vec::new(),
        vec![FileStatus {
            path: "new.rs".into(),
            change: ChangeKind::Added,
        }],
        vec!["b".repeat(40)],
    );
    assert_eq!(tab.head, CommitId("new".into()));
    assert_eq!(tab.files[0].path, std::path::PathBuf::from("new.rs"));
    assert_eq!(tab.selected_file, Some(0));
}

#[test]
fn one_apply_function_updates_list_and_open_copies_only_for_the_same_pr_and_head() {
    let mut list = vec![pr(1, "same"), pr(2, "other")];
    let mut fork = pr(1, "same");
    fork.base_repo = "github.com/other/widgets".into();
    let mut opened = vec![pr(1, "same"), pr(1, "moved"), fork];
    let detail = PrStatusDetail {
        number: 1,
        head_sha: "same".into(),
        ci: kagi_domain::github::CiState::Success,
        checks: Vec::new(),
        mergeable: kagi_domain::github::Mergeable::Clean,
    };
    apply_status_copies(
        &mut list,
        opened.iter_mut(),
        "github.com/acme/widgets",
        &detail,
    );
    assert_eq!(list[0].ci, kagi_domain::github::CiState::Success);
    assert_eq!(opened[0].ci, kagi_domain::github::CiState::Success);
    assert_eq!(list[1].ci, kagi_domain::github::CiState::None);
    assert_eq!(opened[1].ci, kagi_domain::github::CiState::None);
    assert_eq!(
        opened[2].ci,
        kagi_domain::github::CiState::None,
        "#940 review: another repository's #1 at the same head is another PR"
    );
}

/// The composer's preview is one flag for the one composer the mode draws, so
/// which tab settled decides whether it may be reset (#750 review).
fn mode_with(active: usize, numbers: [u64; 2]) -> crate::ui::pr_mode::PrModeState {
    let mut mode = crate::ui::pr_mode::PrModeState::default();
    for number in numbers {
        let mut t = tab("head");
        t.pr = pr(number, "head");
        t.comment_draft = format!("draft for #{number}");
        mode.tabs.push(t);
    }
    mode.active = Some(active);
    mode.comment_preview = true;
    mode
}

#[test]
fn settling_the_open_tab_empties_its_draft_and_returns_the_box() {
    let mut mode = mode_with(0, [7, 8]);
    mode.settle_composer_for(&pr(7, "head").key());
    assert!(mode.tabs[0].comment_draft.is_empty());
    assert!(
        !mode.comment_preview,
        "a posted comment must not leave an empty preview the reader has to press the pen to escape"
    );
}

#[test]
fn settling_a_background_tab_leaves_the_open_tab_composing() {
    let mut mode = mode_with(1, [7, 8]);
    mode.settle_composer_for(&pr(7, "head").key());
    assert!(mode.tabs[0].comment_draft.is_empty(), "#7's text is posted");
    assert_eq!(
        mode.tabs[1].comment_draft, "draft for #8",
        "the open tab's own draft is untouched"
    );
    assert!(
        mode.comment_preview,
        "a completion for another PR must not flip the box the reader is in"
    );
}

/// #940 review: one session holds A#7 and B#7. A post to B#7 empties B's
/// draft only; A's tab keeps its own.
#[test]
fn settling_a_pr_leaves_another_repositorys_same_number_alone() {
    let mut mode = mode_with(0, [7, 7]);
    mode.tabs[1].pr.base_repo = "github.com/other/widgets".into();
    mode.tabs[1].comment_draft = "draft for other #7".into();
    mode.settle_composer_for(&mode.tabs[1].pr.key());
    assert_eq!(mode.tabs[0].comment_draft, "draft for #7");
    assert!(mode.tabs[1].comment_draft.is_empty());
    assert!(
        mode.comment_preview,
        "the open tab is A#7, which did not settle"
    );
}

fn reappended_unopened_checks_stay_coherent(ci: kagi_domain::github::CiState) {
    use kagi_domain::github::{Check, PrListSnapshot};
    use kagi_domain::list_filter::{apply_prs, ChecksFilter};
    let page = |numbers: std::ops::Range<u64>, cursor: Option<&str>| PrListSnapshot {
        prs: numbers
            .map(|number| {
                let mut row = pr(number, &format!("head-{number}"));
                row.state = IssueState::Open;
                row
            })
            .collect(),
        base_repo: "github.com/acme/widgets".into(),
        next_cursor: cursor.map(str::to_owned),
    };
    let matches = |ui: &TabUiState, checks: ChecksFilter| {
        let mut filter = ui.github_pr_filter.clone();
        filter.checks = checks;
        apply_prs(&ui.github_prs, &filter, |row| {
            ui.pr_details.availability(row, PrDetailStage::Status)
        })
        .into_iter()
        .map(|index| ui.github_prs[index].number)
        .collect::<Vec<_>>()
    };
    let expected_filter = if ci == kagi_domain::github::CiState::Success {
        ChecksFilter::Passing
    } else {
        ChecksFilter::Failing
    };
    let opposite_filter = if expected_filter == ChecksFilter::Passing {
        ChecksFilter::Failing
    } else {
        ChecksFilter::Passing
    };
    let mut ui = TabUiState {
        pr_mode: Some(crate::ui::pr_mode::PrModeState::default()),
        ..Default::default()
    };
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(page(1..101, Some("page-two"))))
        .unwrap();
    let append = ui.begin_pr_page_request().unwrap();
    assert!(ui.finish_pr_page_request(&append, Ok(page(101..121, None))));
    let target = ui.github_prs[119].clone();
    let key = target.key();
    assert!(
        ui.opened_pr_keys().is_empty(),
        "page-two row is never opened"
    );
    let now = Instant::now();
    ui.pr_details.observe_visible(
        BTreeSet::from([key.clone()]),
        BTreeSet::new(),
        ui.github_prs_epoch,
        now,
    );
    ui.pr_details
        .enqueue(path(), &target, PrDetailStage::Status, false, false);
    let started = ui
        .pr_details
        .take_next(now)
        .expect("initial visible status request");
    let detail = PrStatusDetail {
        number: 120,
        head_sha: target.head_sha.clone(),
        ci,
        checks: vec![Check {
            name: "build".into(),
            workflow: "CI".into(),
            state: ci,
            url: String::new(),
        }],
        mergeable: kagi_domain::github::Mergeable::Clean,
    };
    assert_eq!(
        ui.pr_details
            .settle(&started, &Ok(DetailResult::Status(detail.clone())), now),
        Some(true),
    );
    apply_status_copies(
        &mut ui.github_prs,
        std::iter::empty::<&mut PullRequest>(),
        "github.com/acme/widgets",
        &detail,
    );
    assert_eq!(ui.github_prs[119].checks, detail.checks);
    assert_eq!(matches(&ui, expected_filter), vec![120]);
    assert!(matches(&ui, opposite_filter).is_empty());

    // A manual L1 refresh really replaces the collection with the first100.
    // Exercise the existing controller reconciliation that this completion runs.
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(page(1..101, Some("page-two-again"))))
        .unwrap();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert_eq!(ui.github_prs.len(), 100);
    assert!(ui.opened_pr_keys().is_empty());
    let append = ui.begin_pr_page_request().unwrap();
    assert!(ui.finish_pr_page_request(&append, Ok(page(101..121, None))));
    let row = &ui.github_prs[119];
    assert_eq!(
        row.head_sha, target.head_sha,
        "same head returns inside the L2 TTL"
    );
    assert!(now.elapsed() < STATUS_FRESH_FOR);
    if ui.pr_details.availability(row, PrDetailStage::Status) == PrDetailAvailability::Fresh {
        assert_eq!(
            row.checks, detail.checks,
            "Fresh status cannot outlive the checks payload removed by first-page replacement",
        );
        assert_eq!(row.ci, ci);
    } else {
        assert!(
            matches(&ui, expected_filter).is_empty(),
            "missing status cannot satisfy a checks filter"
        );
    }

    // Only the genuinely visible row and its two neighbouring rows demand L2.
    // No opened row exists, so no body demand is authorized.
    let visible: BTreeSet<_> = ui.github_prs[117..120]
        .iter()
        .map(PullRequest::key)
        .collect();
    let (visible_generation, _) = ui
        .pr_details
        .observe_visible(
            visible.clone(),
            BTreeSet::new(),
            ui.github_prs_epoch,
            Instant::now(),
        )
        .expect("new accepted collection rearms visible demand");
    let demanded = ui
        .pr_details
        .finish_visible_debounce(visible_generation, ui.github_prs_epoch)
        .unwrap();
    for row in ui
        .github_prs
        .iter()
        .filter(|row| demanded.contains(&row.key()))
    {
        ui.pr_details
            .enqueue(path(), row, PrDetailStage::Status, false, false);
    }
    assert!(ui.pr_details.pending.iter().all(|request| {
        request.key.stage == PrDetailStage::Status && visible.contains(&request.key.pr)
    }));
    assert!(ui.pr_details.pending.len() <= 3);
    let first = ui.pr_details.take_next(Instant::now());
    let second = ui.pr_details.take_next(Instant::now());
    assert!(ui.pr_details.active.len() <= 2);
    if first.is_some() && second.is_some() {
        assert!(
            ui.pr_details.take_next(Instant::now()).is_none(),
            "visible acquisition shares the concurrency-two budget"
        );
    }
    let mut deliveries: Vec<_> = first.into_iter().chain(second).collect();
    let mut reread_target = false;
    while let Some(started) = deliveries.pop() {
        assert!(visible.contains(&started.request.key.pr));
        assert_eq!(started.request.key.stage, PrDetailStage::Status);
        reread_target |= started.request.key.pr == key;
        let response = PrStatusDetail {
            number: started.request.key.pr.number,
            head_sha: started.request.head_sha.clone(),
            ci,
            checks: detail.checks.clone(),
            mergeable: detail.mergeable,
        };
        assert_eq!(
            ui.pr_details.settle(
                &started,
                &Ok(DetailResult::Status(response.clone())),
                Instant::now()
            ),
            Some(true),
        );
        apply_status_copies(
            &mut ui.github_prs,
            std::iter::empty::<&mut PullRequest>(),
            &started.request.key.pr.base_repo,
            &response,
        );
        if let Some(next) = ui.pr_details.take_next(Instant::now()) {
            deliveries.push(next);
        }
        assert!(ui.pr_details.active.len() <= 2);
    }
    assert!(reread_target || ui.github_prs[119].checks == detail.checks);
    assert_eq!(ui.github_prs[119].checks, detail.checks);
    assert_eq!(ui.github_prs[119].ci, ci);
    assert!(matches(&ui, expected_filter).contains(&120));
    assert!(!matches(&ui, opposite_filter).contains(&120));
    assert!(ui.pr_details.pending.is_empty());
    assert!(ui.pr_details.active.is_empty());
}

#[test]
fn reappended_unopened_pr_refetches_passing_checks_with_visible_budget() {
    reappended_unopened_checks_stay_coherent(kagi_domain::github::CiState::Success);
}

#[test]
fn reappended_unopened_pr_refetches_failing_checks_with_visible_budget() {
    reappended_unopened_checks_stay_coherent(kagi_domain::github::CiState::Failure);
}

#[test]
fn retained_other_collection_same_head_checks_passing_is_fresh() {
    use kagi_domain::github::{Check, CiState, PrListSnapshot};
    use kagi_domain::list_filter::{apply_prs, ChecksFilter, StateFilter};
    let open_page = || PrListSnapshot {
        prs: (1..101)
            .map(|number| {
                let mut row = pr(number, &format!("head-{number}"));
                row.state = IssueState::Open;
                row
            })
            .collect(),
        base_repo: "github.com/acme/widgets".into(),
        next_cursor: None,
    };
    let mut ui = TabUiState {
        pr_mode: Some(crate::ui::pr_mode::PrModeState::default()),
        ..Default::default()
    };
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(open_page()))
        .unwrap();
    let target = ui.github_prs[49].clone();
    let key = target.key();
    let now = Instant::now();
    ui.pr_details.observe_visible(
        BTreeSet::from([key.clone()]),
        BTreeSet::new(),
        ui.github_prs_epoch,
        now,
    );
    ui.pr_details
        .enqueue(path(), &target, PrDetailStage::Status, false, false);
    let started = ui.pr_details.take_next(now).unwrap();
    let detail = PrStatusDetail {
        number: 50,
        head_sha: target.head_sha.clone(),
        ci: CiState::Success,
        checks: vec![Check {
            name: "build".into(),
            workflow: "CI".into(),
            state: CiState::Success,
            url: String::new(),
        }],
        mergeable: kagi_domain::github::Mergeable::Clean,
    };
    assert_eq!(
        ui.pr_details
            .settle(&started, &Ok(DetailResult::Status(detail.clone())), now,),
        Some(true)
    );
    apply_status_copies(
        &mut ui.github_prs,
        std::iter::empty::<&mut PullRequest>(),
        "github.com/acme/widgets",
        &detail,
    );
    ui.github_pr_filter.checks = ChecksFilter::Passing;
    ui.pr_details.observe_visible(
        BTreeSet::new(),
        BTreeSet::new(),
        ui.github_prs_epoch,
        Instant::now(),
    );

    // The actual selected-list receiver owns Closed membership, but the
    // same-head payload for unopened #50 is still held by shared Open.
    ui.reset_pr_strip();
    ui.github_pr_filter.common.state = StateFilter::Closed;
    let generation = ui.begin_pr_strip_request(StateFilter::Closed);
    let mut closed = pr(200, "closed-head");
    closed.state = IssueState::Closed;
    ui.finish_pr_strip_request(
        generation,
        StateFilter::Closed,
        Ok(PrListSnapshot {
            prs: vec![closed],
            base_repo: "github.com/acme/widgets".into(),
            next_cursor: None,
        }),
    )
    .unwrap();
    ui.pr_details.opened = ui.opened_pr_keys();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert_eq!(ui.github_prs[49].checks, detail.checks);
    assert!(
        ui.pr_details.pending.is_empty(),
        "collection change must not read off-screen Open"
    );
    assert!(ui.opened_pr_keys().is_empty());

    // Return to Open through the same owner transitions and L1 completion.
    // Status is within TTL and its real payload was never removed.
    ui.reset_pr_strip();
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(open_page()))
        .unwrap();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert_eq!(ui.github_pr_filter.checks, ChecksFilter::Passing);
    assert_eq!(ui.github_prs[49].head_sha, target.head_sha);
    assert_eq!(ui.github_prs[49].checks, detail.checks);
    assert!(now.elapsed() < STATUS_FRESH_FOR);
    let matches: Vec<_> = apply_prs(&ui.github_prs, &ui.github_pr_filter, |row| {
        ui.pr_details.availability(row, PrDetailStage::Status)
    })
    .into_iter()
    .map(|index| ui.github_prs[index].number)
    .collect();
    assert_eq!(
        matches,
        vec![50],
        "retained same-head passing payload must not become invisible to its own checks filter",
    );
    assert_eq!(
        ui.pr_details
            .availability(&ui.github_prs[49], PrDetailStage::Status),
        PrDetailAvailability::Fresh,
    );
    let (generation, _) = ui
        .pr_details
        .observe_visible(
            BTreeSet::from([key.clone()]),
            BTreeSet::new(),
            ui.github_prs_epoch,
            Instant::now(),
        )
        .unwrap();
    let visible = ui
        .pr_details
        .finish_visible_debounce(generation, ui.github_prs_epoch)
        .unwrap();
    for row in ui
        .github_prs
        .iter()
        .filter(|row| visible.contains(&row.key()))
    {
        ui.pr_details
            .enqueue(path(), row, PrDetailStage::Status, false, false);
    }
    assert!(ui.pr_details.pending.iter().all(|request| {
        request.key.stage == PrDetailStage::Status && visible.contains(&request.key.pr)
    }));
    assert!(ui.pr_details.active.len() <= 2);
    assert!(
        ui.pr_details.take_next(Instant::now()).is_none(),
        "retained Fresh payload needs no new L2 read, and no unopened row authorizes L3"
    );
    assert!(ui.pr_details.pending.is_empty());
    assert!(ui.opened_pr_keys().is_empty());
}

fn detail_owner_page(
    prs: Vec<PullRequest>,
    next_cursor: Option<&str>,
) -> kagi_domain::github::PrListSnapshot {
    kagi_domain::github::PrListSnapshot {
        base_repo: prs
            .first()
            .map(|row| row.base_repo.clone())
            .unwrap_or_else(|| "github.com/acme/widgets".into()),
        prs,
        next_cursor: next_cursor.map(str::to_owned),
    }
}

fn ui_with_fresh_opened_details() -> TabUiState {
    let mut opened = tab("same");
    opened.pr.state = IssueState::Open;
    let target = opened.pr.clone();
    let mut mode = crate::ui::pr_mode::PrModeState::default();
    mode.tabs.push(opened);
    let mut ui = TabUiState {
        pr_mode: Some(mode),
        ..Default::default()
    };
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(
        generation,
        Ok(detail_owner_page(vec![target.clone()], None)),
    )
    .unwrap();
    ui.pr_details.opened = ui.opened_pr_keys();
    // Empty checks/body and zero statistics are real successful payloads, not
    // a reason to infer that an existing owner holds only L1.
    for (stage, result) in [
        (
            PrDetailStage::Status,
            DetailResult::Status(PrStatusDetail {
                number: target.number,
                head_sha: target.head_sha.clone(),
                ci: kagi_domain::github::CiState::Success,
                checks: Vec::new(),
                mergeable: kagi_domain::github::Mergeable::Clean,
            }),
        ),
        (
            PrDetailStage::Body,
            DetailResult::Body(PrBodyDetail {
                number: target.number,
                head_sha: target.head_sha.clone(),
                updated_at: String::new(),
                body: String::new(),
                changed_files: 0,
                additions: 0,
                deletions: 0,
            }),
        ),
    ] {
        ui.pr_details.enqueue(path(), &target, stage, true, false);
        let started = ui.pr_details.take_next(Instant::now()).unwrap();
        let result = Ok(result);
        assert_eq!(
            ui.pr_details.settle(&started, &result, Instant::now()),
            Some(true),
        );
        let opened = ui
            .pr_mode
            .iter_mut()
            .flat_map(|mode| mode.tabs.iter_mut().map(|tab| &mut tab.pr));
        match result.unwrap() {
            DetailResult::Status(detail) => {
                apply_status_copies(&mut ui.github_prs, opened, &target.base_repo, &detail);
            }
            DetailResult::Body(detail) => {
                apply_body_copies(&mut ui.github_prs, opened, &target.base_repo, &detail);
            }
        }
    }
    ui
}

#[test]
fn selected_strip_retains_same_head_details_after_shared_open_replacement() {
    use kagi_domain::list_filter::{apply_prs, ChecksFilter, StateFilter};
    let mut ui = ui_with_fresh_opened_details();
    let target = ui.github_prs[0].clone();
    ui.github_pr_filter.common.state = StateFilter::All;
    ui.github_pr_filter.checks = ChecksFilter::Passing;
    let generation = ui.begin_pr_strip_request(StateFilter::All);
    let mut incoming = pr(1, "same");
    incoming.state = IssueState::Open;
    ui.finish_pr_strip_request(
        generation,
        StateFilter::All,
        Ok(detail_owner_page(vec![incoming], None)),
    )
    .unwrap();
    ui.pr_mode.as_mut().unwrap().tabs.clear();
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(detail_owner_page(Vec::new(), None)))
        .unwrap();
    ui.pr_details.opened = ui.opened_pr_keys();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert!(ui.github_prs.is_empty());
    assert!(ui.opened_pr_keys().is_empty());
    let rows = ui.pr_list_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ci, target.ci);
    assert_eq!(rows[0].body, target.body);
    for stage in [PrDetailStage::Status, PrDetailStage::Body] {
        assert_eq!(
            ui.pr_details.availability(&rows[0], stage),
            PrDetailAvailability::Fresh,
        );
    }
    assert_eq!(
        apply_prs(rows, &ui.github_pr_filter, |row| {
            ui.pr_details.availability(row, PrDetailStage::Status)
        }),
        vec![0],
    );
    ui.pr_details
        .enqueue(path(), &target, PrDetailStage::Status, false, false);
    assert!(ui.pr_details.pending.is_empty());
    assert!(ui.pr_details.take_next(Instant::now()).is_none());
}

#[test]
fn opened_only_owner_retains_empty_details_for_same_head_append() {
    let mut ui = ui_with_fresh_opened_details();
    let target = ui.github_prs[0].clone();
    let mut unrelated = pr(2, "other");
    unrelated.state = IssueState::Open;
    let generation = ui.begin_github_prs_request();
    let accepted = ui
        .finish_github_prs_request(
            generation,
            Ok(detail_owner_page(vec![unrelated], Some("append-opened"))),
        )
        .expect("the current first-page owner accepts its result");
    assert!(accepted.error.is_none(), "{:?}", accepted.error);
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert!(
        ui.github_prs.iter().all(|row| row.number != target.number),
        "the target's only retained payload owner is its opened pane"
    );
    for stage in [PrDetailStage::Status, PrDetailStage::Body] {
        assert_eq!(
            ui.pr_details.availability(&target, stage),
            PrDetailAvailability::Fresh,
        );
    }
    let append = ui.begin_pr_page_request().unwrap();
    let mut incoming = pr(1, "same");
    incoming.state = IssueState::Open;
    assert!(ui.finish_pr_page_request(&append, Ok(detail_owner_page(vec![incoming], None)),));
    let row_index = ui
        .github_prs
        .iter()
        .position(|row| row.number == target.number)
        .expect("the target was accepted in the appended page");
    assert_eq!(ui.github_prs[row_index].ci, target.ci);
    assert!(ui.github_prs[row_index].checks.is_empty());
    assert!(ui.github_prs[row_index].body.is_empty());
    assert_eq!(ui.github_prs[row_index].changed_files, 0);
    for stage in [PrDetailStage::Status, PrDetailStage::Body] {
        assert_eq!(
            ui.pr_details.availability(&ui.github_prs[row_index], stage),
            PrDetailAvailability::Fresh,
        );
        ui.pr_details
            .enqueue(path(), &ui.github_prs[row_index], stage, true, false);
    }
    assert!(ui.pr_details.pending.is_empty());
    assert!(ui.pr_details.take_next(Instant::now()).is_none());

    // Once both actual copies go away, opened membership must not leave
    // authority behind for a future L1-only copy at this head.
    ui.pr_mode.as_mut().unwrap().tabs.clear();
    let generation = ui.begin_github_prs_request();
    ui.finish_github_prs_request(generation, Ok(detail_owner_page(Vec::new(), None)))
        .unwrap();
    ui.pr_details.opened = ui.opened_pr_keys();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    for stage in [PrDetailStage::Status, PrDetailStage::Body] {
        assert_eq!(
            ui.pr_details.availability(&target, stage),
            PrDetailAvailability::Missing,
        );
    }
}

#[test]
fn stale_or_other_repository_owners_cannot_preserve_fresh_or_settle_old_response() {
    use kagi_domain::list_filter::{apply_prs, ChecksFilter, StateFilter};
    for other_repository in [false, true] {
        let mut ui = ui_with_fresh_opened_details();
        let target = ui.github_prs[0].clone();
        ui.github_pr_filter.common.state = StateFilter::Closed;
        let generation = ui.begin_pr_strip_request(StateFilter::Closed);
        let mut closed = pr(200, "closed");
        closed.state = IssueState::Closed;
        ui.finish_pr_strip_request(
            generation,
            StateFilter::Closed,
            Ok(detail_owner_page(vec![closed], None)),
        )
        .unwrap();
        ui.pr_details
            .enqueue(path(), &target, PrDetailStage::Status, true, true);
        let old = ui.pr_details.take_next(Instant::now()).unwrap();
        let mut alternate = pr(1, if other_repository { "same" } else { "moved" });
        alternate.state = IssueState::Open;
        if other_repository {
            alternate.base_repo = "github.com/other/widgets".into();
            ui.pr_mode.as_mut().unwrap().tabs[0].pr = alternate.clone();
        }
        let generation = ui.begin_github_prs_request();
        ui.finish_github_prs_request(generation, Ok(detail_owner_page(vec![alternate], None)))
            .unwrap();
        assert_eq!(
            sync_open_pr_tabs(&mut ui).len(),
            usize::from(!other_repository),
        );
        ui.pr_details.opened = ui.opened_pr_keys();
        ui.pr_details.reconcile_heads(
            &ui.github_prs,
            ui.github_prs_strip.rows.as_deref(),
            ui.pr_mode
                .iter()
                .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
        );
        let late = Ok(DetailResult::Status(PrStatusDetail {
            number: target.number,
            head_sha: target.head_sha.clone(),
            ci: target.ci,
            checks: target.checks.clone(),
            mergeable: target.mergeable,
        }));
        assert_eq!(
            ui.pr_details.settle(&old, &late, Instant::now()),
            None,
            "an unrelated/stale owner cannot authorize the old generation",
        );
        assert!(ui.pr_details.active.is_empty());

        ui.reset_pr_strip();
        let generation = ui.begin_github_prs_request();
        let mut incoming = pr(1, "same");
        incoming.state = IssueState::Open;
        ui.finish_github_prs_request(generation, Ok(detail_owner_page(vec![incoming], None)))
            .unwrap();
        assert_eq!(
            sync_open_pr_tabs(&mut ui).len(),
            usize::from(!other_repository),
        );
        ui.pr_details.reconcile_heads(
            &ui.github_prs,
            ui.github_prs_strip.rows.as_deref(),
            ui.pr_mode
                .iter()
                .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
        );
        ui.github_pr_filter.checks = ChecksFilter::Passing;
        for stage in [PrDetailStage::Status, PrDetailStage::Body] {
            assert_eq!(
                ui.pr_details.availability(&ui.github_prs[0], stage),
                PrDetailAvailability::Missing,
            );
        }
        assert!(apply_prs(&ui.github_prs, &ui.github_pr_filter, |row| {
            ui.pr_details.availability(row, PrDetailStage::Status)
        })
        .is_empty());
        assert!(ui.pr_details.pending.is_empty());
        ui.pr_details.enqueue(
            path(),
            &ui.github_prs[0],
            PrDetailStage::Status,
            false,
            false,
        );
        assert_eq!(ui.pr_details.pending.len(), 1);
        assert_eq!(ui.pr_details.pending[0].key.stage, PrDetailStage::Status);
        assert!(ui.pr_details.take_next(Instant::now()).is_some());
        assert!(ui.pr_details.active.len() <= 2);
    }
}

#[test]
fn selected_membership_trims_stale_visible_demand_but_keeps_shared_payload() {
    use kagi_domain::list_filter::{apply_prs, ChecksFilter, StateFilter};
    let mut ui = ui_with_fresh_opened_details();
    let target = ui.github_prs[0].clone();
    ui.pr_mode.as_mut().unwrap().tabs.clear();
    ui.pr_details.observe_visible(
        BTreeSet::from([target.key()]),
        BTreeSet::new(),
        ui.github_prs_epoch,
        Instant::now(),
    );
    ui.github_pr_filter.common.state = StateFilter::Closed;
    ui.github_pr_filter.checks = ChecksFilter::Passing;
    let generation = ui.begin_pr_strip_request(StateFilter::Closed);
    let mut closed = pr(200, "closed");
    closed.state = IssueState::Closed;
    ui.finish_pr_strip_request(
        generation,
        StateFilter::Closed,
        Ok(detail_owner_page(vec![closed], None)),
    )
    .unwrap();
    // No subsequent dashboard observe_visible(empty) is delivered: the
    // selected table has no rows satisfying Passing.
    ui.pr_details.opened = ui.opened_pr_keys();
    ui.pr_details.reconcile_heads(
        &ui.github_prs,
        ui.github_prs_strip.rows.as_deref(),
        ui.pr_mode
            .iter()
            .flat_map(|mode| mode.tabs.iter().map(|tab| &tab.pr)),
    );
    assert!(apply_prs(ui.pr_list_rows(), &ui.github_pr_filter, |row| {
        ui.pr_details.availability(row, PrDetailStage::Status)
    })
    .is_empty());
    let (visible, opened) = ui.pr_details.targets();
    assert!(visible.is_empty());
    assert!(opened.is_empty());
    assert_eq!(
        ui.pr_details.availability(&target, PrDetailStage::Status),
        PrDetailAvailability::Fresh,
    );
    // Consume the exact forced visible refresh demand. Shared Open remains an
    // available payload source, but must not become an off-screen read source.
    for row in ui
        .github_prs
        .iter()
        .filter(|row| visible.contains(&row.key()))
    {
        ui.pr_details
            .enqueue(path(), row, PrDetailStage::Status, false, true);
    }
    assert!(ui.pr_details.pending.is_empty());
    assert!(ui.pr_details.active.is_empty());
    assert!(ui.pr_details.take_next(Instant::now()).is_none());
}
