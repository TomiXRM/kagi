//! Issue #1107: the product's ordinary 60-second ticker cannot discard a viewed page.
use std::path::Path;
use std::time::Duration;

use gpui::VisualTestAppContext;
use kagi::ui::e2e;

use super::fixture::{periodic_refresh_gh, periodic_single_page_gh};
use super::{assert_lazy_demand, bottom, click, info, measure, page, ready};
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount_state, repo_fingerprint, unmount};

fn first_page_reads(receipt: &Path) -> usize {
    std::fs::read_to_string(receipt)
        .expect("real gh fixture receipts")
        .lines()
        .filter(|argv| argv.starts_with("api graphql ") && argv.contains("-F cursor=null "))
        .count()
}

fn assert_periodic_status(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    number: u64,
    after: bool,
) {
    use kagi_domain::github::CiState;
    cx.read(|cx| {
        let app = app.read(cx);
        let shared = app
            .ui()
            .github_prs
            .iter()
            .find(|pr| pr.number == number)
            .unwrap();
        let opened = app
            .pr_mode()
            .unwrap()
            .tabs
            .iter()
            .filter(|tab| tab.pr.is(&shared.key()));
        for (consumer, pr) in
            std::iter::once(("shared", shared)).chain(opened.map(|tab| ("opened", &tab.pr)))
        {
            let expected = if after {
                CiState::Failure
            } else {
                CiState::Success
            };
            assert_eq!(
                pr.ci,
                expected,
                "periodic status consumer={consumer} key={:?} head={}",
                pr.key(),
                pr.head_sha,
            );
            assert_eq!(pr.checks.len(), 1);
            assert_eq!(pr.checks[0].state, expected);
            assert_eq!(
                pr.checks[0].name,
                if after {
                    "periodic-after"
                } else {
                    "periodic-before"
                }
            );
        }
    });
}

pub fn scenario_pr_single_page_periodic_scroll_retention(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let receipt = fixture.path().join(".git/periodic-gh-argv");
    let _gh = periodic_single_page_gh(&receipt);
    let mut state = e2e::app_state(&repo).unwrap();
    state
        .github_host_logins
        .insert(Some("github.com".into()), "alice".into());
    let (app, win) = mount_state(cx, state);
    app.update(cx, |app, cx| app.ensure_startup_repo_io(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).github_ticker_alive));
    assert_eq!(info(cx, &app), ((1..81).collect(), None, false));
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();

    // Move the real virtualized table with a wheel event, retaining a partial
    // row offset rather than only an integer row index.
    let viewport = measure(cx, win, "pr-home-table-viewport");
    let row_height = cx.read(|cx| {
        app.read(cx)
            .pr_mode()
            .unwrap()
            .dashboard_scroll
            .0
            .borrow()
            .last_item_size
            .expect("real uniform-list layout")
            .contents
            .height
            / 80.
    });
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(
                gpui::px(0.),
                -(row_height * 35. + gpui::px(7.)),
            )),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    measure(cx, win, "pr-home-table-viewport");
    let anchor_index = cx.read(|cx| {
        let scroll = app.read(cx).pr_mode().unwrap().dashboard_scroll.0.borrow();
        (-scroll.base_handle.offset().y / row_height).floor() as usize
    });
    assert!(
        (10..70).contains(&anchor_index),
        "the real table reached a mid row: index={anchor_index}, height={row_height:?}"
    );
    let anchor_id = format!("pr-home-row-{}", anchor_index + 3);
    let anchor_row = measure(cx, win, &anchor_id);
    assert!(
        viewport.contains(&anchor_row.center()),
        "mid row is visible"
    );
    cx.run_until_parked();
    let (generation, anchor_offset) = cx.read(|cx| {
        let app = app.read(cx);
        (
            app.ui().github_prs_gen,
            app.pr_mode()
                .unwrap()
                .dashboard_scroll
                .0
                .borrow()
                .base_handle
                .offset(),
        )
    });
    assert!(anchor_offset.y < gpui::px(0.));
    let reads = first_page_reads(&receipt);

    // No queued fetch, explicit refresh or mode change: the production timer
    // and strict offline gh producer must perform the next first-page read.
    cx.advance_clock(Duration::from_secs(61));
    cx.run_until_parked();
    assert_eq!(first_page_reads(&receipt), reads + 1);
    assert_eq!(info(cx, &app), ((1..81).collect(), None, false));
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_prs_gen),
        generation.wrapping_add(1),
        "a single-page list must keep ordinary automatic refresh"
    );
    measure(cx, win, "pr-home-table-viewport");
    assert_eq!(
        cx.read(|cx| {
            app.read(cx)
                .pr_mode()
                .unwrap()
                .dashboard_scroll
                .0
                .borrow()
                .base_handle
                .offset()
        }),
        anchor_offset,
        "automatic refresh preserves the exact row offset"
    );
    assert_eq!(
        measure(cx, win, &anchor_id),
        anchor_row,
        "automatic first-page refresh must not move the anchor row"
    );
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_single_page_periodic_scroll_retention");
}

pub fn scenario_pr_paging_survives_periodic_tick(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let receipt = fixture.path().join(".git/periodic-gh-argv");
    let _gh = periodic_refresh_gh(&receipt);
    let mut state = e2e::app_state(&repo).unwrap();
    state
        .github_host_logins
        .insert(Some("github.com".into()), "alice".into());
    let (app, win) = mount_state(cx, state);
    // The offscreen mount does not arm startup I/O. Use the same production
    // startup entry as open_main_window; no first-page task is queued and no
    // refresh entry is called. The ticker probes gh and its initial shared
    // Open read goes through the real bounded GraphQL backend/parser.
    app.update(cx, |app, cx| app.ensure_startup_repo_io(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).github_ticker_alive));
    assert!(
        first_page_reads(&receipt) > 0,
        "the real ticker must read L1"
    );
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), false)
    );
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    measure(cx, win, "pr-home-table-viewport");
    assert_lazy_demand(cx, &app, false);

    // Retain a real selected PR tab and a nonempty composer, then return to
    // the paged table without closing the tab. Ref-fetch failure's ordinary
    // owner-bound L1 read uses the same truthful first-page fixture.
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    click(cx, win, "pr-home-row-1");
    measure(cx, win, "pr-composer-mode-toggle");
    let (owner, selected, composer) = cx.read(|cx| {
        let app = app.read(cx);
        let mode = app.pr_mode().unwrap();
        assert_eq!(mode.active, Some(0));
        (
            app.active_session().unwrap(),
            mode.tabs[0].pr.key(),
            app.pr_comment_input.clone().expect("real PR composer"),
        )
    });
    cx.update_window(win, |_, window, cx| {
        composer.update(cx, |input, cx| {
            input.set_value("reply survives the automatic tick", window, cx)
        });
    })
    .unwrap();
    let composer_id = composer.entity_id();
    drop(composer);
    app.update(cx, |app, cx| app.pr_mode_home(cx));
    cx.run_until_parked();
    // A nonempty real InputState keeps automatic tail demand disabled. Only
    // the user's measured Load more click admits the queued second page.
    click(cx, win, "list-filter-text");
    cx.simulate_keystrokes(win, "PR");
    cx.run_until_parked();
    bottom(cx, win);
    let button = measure(cx, win, "pr-filter-load-more");
    let viewport = measure(cx, win, "pr-home-table-viewport");
    assert!(
        viewport.contains(&button.center()),
        "Load more must be clipped-visible"
    );
    ready(page(101..121, None));
    click(cx, win, "pr-filter-load-more");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    bottom(cx, win);
    let tail = measure(cx, win, "pr-home-row-120");
    let viewport = measure(cx, win, "pr-home-table-viewport");
    assert!(
        viewport.contains(&tail.center()),
        "page two must really be in view"
    );
    assert_lazy_demand(cx, &app, true);
    let (generation, anchor, typed_filter) = cx.read(|cx| {
        let app = app.read(cx);
        assert!(
            !app.ui().github_pr_filter.common.text.is_empty(),
            "the real typed filter must disable automatic tail demand"
        );
        (
            app.ui().github_prs_gen,
            app.pr_mode()
                .unwrap()
                .dashboard_scroll
                .0
                .borrow()
                .base_handle
                .offset(),
            app.ui().github_pr_filter.common.text.clone(),
        )
    });
    let reads = first_page_reads(&receipt);

    // The ticker's timer was scheduled at startup on GPUI's controlled
    // background clock; none of the setup above advances that clock. No
    // injected first-page task, explicit refresh, state change or repo switch
    // occurs between these two controls and the acceptance assertions.
    cx.advance_clock(Duration::from_secs(59));
    cx.run_until_parked();
    assert_eq!(first_page_reads(&receipt), reads, "not a premature refresh");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "paging remains Git-read-only"
    );
    assert_periodic_status(cx, &app, 1, false);
    assert_periodic_status(cx, &app, 120, false);
    // Change only the private transport producer's server-side status. No
    // consumer payload, freshness timestamp, retry clock or guard is altered.
    std::fs::write(receipt.with_extension("status-after"), "").unwrap();
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "the periodic path remains Git-read-only"
    );
    assert_eq!(
        info(cx, &app),
        ((1..121).collect(), None, false),
        "the default 60s ticker must retain the actively viewed appended range; L1 receipts before={reads}, after={}",
        first_page_reads(&receipt),
    );
    assert_periodic_status(cx, &app, 1, true);
    assert_periodic_status(cx, &app, 120, true);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(owner));
        assert_eq!(
            app.ui().github_prs_gen,
            generation,
            "a deferred tick cannot revoke page authority"
        );
        assert_eq!(
            app.ui().github_pr_filter.common.text,
            typed_filter,
            "the automatic tick retains the actual typed filter"
        );
        let mode = app.pr_mode().unwrap();
        assert_eq!(mode.active, None, "the table remains the active consumer");
        assert_eq!(
            mode.tabs[0].pr.key(),
            selected,
            "the parked selected PR stays owner-bound"
        );
        assert_eq!(
            mode.dashboard_scroll.0.borrow().base_handle.offset(),
            anchor,
            "automatic refresh cannot jump the viewport"
        );
        let input = app.pr_comment_input.as_ref().expect("retained composer");
        assert_eq!(input.entity_id(), composer_id);
        assert_eq!(
            input.read(cx).value().as_str(),
            "reply survives the automatic tick"
        );
    });
    let retained_tail = measure(cx, win, "pr-home-row-120");
    assert_eq!(
        retained_tail, tail,
        "the actual visible page-two row keeps its anchor"
    );
    assert!(measure(cx, win, "pr-home-table-viewport").contains(&retained_tail.center()));
    assert_lazy_demand(cx, &app, true);

    // Explicit manual Refresh retains its original first-page cutover. The
    // old continuation loses both cursor and generation authority at once,
    // and its late answer cannot publish while the fresh first page is held.
    ready(page(1..101, Some("manual-page")));
    app.update(cx, |app, cx| app.pr_mode_refresh(cx));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("manual-page".into()), false)
    );
    let manual_generation = cx.read(|cx| app.read(cx).ui().github_prs_gen);
    assert_eq!(manual_generation, generation.wrapping_add(1));
    bottom(cx, win);
    let (task, old_append) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "pr-filter-load-more");
    assert!(
        info(cx, &app).2,
        "the real continuation control admits the old page"
    );
    let (task, fresh_first) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.pr_mode_refresh(cx));
    cx.run_until_parked();
    assert_eq!(info(cx, &app), ((1..101).collect(), None, false));
    assert!(cx.read(|cx| e2e::pr_list_read_status(app.read(cx)).0));
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_prs_gen),
        manual_generation.wrapping_add(1)
    );
    old_append.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), None, false),
        "manual Refresh rejects the old continuation"
    );
    fresh_first.send(Ok(page(1..101, Some("fresh-manual-page"))));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("fresh-manual-page".into()), false)
    );
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_paging_survives_periodic_tick");
}

pub fn scenario_pr_periodic_pending_sidebar_and_other_collections(cx: &mut VisualTestAppContext) {
    use kagi_domain::github::IssueState;
    use kagi_domain::list_filter::StateFilter;

    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let receipt = fixture.path().join(".git/periodic-gh-argv");
    let _gh = periodic_refresh_gh(&receipt);
    let mut state = e2e::app_state(&repo).unwrap();
    state
        .github_host_logins
        .insert(Some("github.com".into()), "alice".into());
    let (app, win) = mount_state(cx, state);
    app.update(cx, |app, cx| app.ensure_startup_repo_io(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).github_ticker_alive));
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), false)
    );
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    click(cx, win, "pr-home-row-1");
    measure(cx, win, "pr-composer-mode-toggle");
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), false)
    );
    let (owner, selected, generation) = cx.read(|cx| {
        let app = app.read(cx);
        let mode = app.pr_mode().unwrap();
        assert_eq!(mode.active, Some(0));
        (
            app.active_session().unwrap(),
            mode.tabs[0].pr.key(),
            app.ui().github_prs_gen,
        )
    });
    let viewport = measure(cx, win, "pr-sidebar-list-viewport");
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let button = measure(cx, win, "pr-sidebar-load-more");
    // Load more is a sibling of the scrolling list body, not one of its
    // clipped rows. Its owning pane and actual paint mask prove reachability.
    let sidebar = measure(cx, win, "pr-mode-left-pane");
    let paint = e2e::control_paint(win.window_id(), "pr-sidebar-load-more")
        .expect("the sidebar demand must actually paint");
    assert!(
        sidebar.contains(&button.center()) && paint.mask.contains(&button.center()),
        "the sidebar demand is actually visible inside its owning pane and paint clip",
    );
    let (task, second_page) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "pr-sidebar-load-more");
    let reads = first_page_reads(&receipt);
    assert_periodic_status(cx, &app, 1, false);
    std::fs::write(receipt.with_extension("status-after"), "").unwrap();
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), true)
    );
    cx.advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    assert_eq!(
        first_page_reads(&receipt),
        reads,
        "a tick cannot revoke a pending shared Open append"
    );
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), true)
    );
    assert_eq!(cx.read(|cx| app.read(cx).ui().github_prs_gen), generation);
    assert_periodic_status(cx, &app, 1, true);
    second_page.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let tail = measure(cx, win, "pr-mode-card-120");
    assert!(
        viewport.contains(&tail.center()),
        "the accepted sidebar page-two tail is reachable"
    );
    cx.advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    assert_eq!(
        first_page_reads(&receipt),
        reads,
        "an opened PR's paged sidebar remains an active consumer"
    );
    assert_eq!(measure(cx, win, "pr-mode-card-120"), tail);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(owner));
        let mode = app.pr_mode().unwrap();
        assert_eq!(mode.active, Some(0));
        assert_eq!(mode.tabs[0].pr.key(), selected);
    });

    // Closed/All are separate strip consumers. Even a pending strip append
    // cannot suppress or be revoked by the automatic shared Open refresh.
    app.update(cx, |app, cx| app.pr_mode_home(cx));
    cx.run_until_parked();
    click(cx, win, "list-filter-text");
    cx.simulate_keystrokes(win, "PR");
    cx.run_until_parked();
    for (state, option) in [(StateFilter::Closed, 1), (StateFilter::All, 2)] {
        let mut chosen = page(200..201, Some("chosen-next"));
        chosen.prs[0].state = IssueState::Closed;
        ready(chosen);
        click(cx, win, "list-filter-state");
        click(cx, win, &format!("list-filter-option-0-{option}"));
        measure(cx, win, "pr-home-row-200");
        let (task, strip_append) = deferred(cx);
        e2e::queue_github_pr_fetch(task);
        click(cx, win, "pr-filter-load-more");
        let reads = first_page_reads(&receipt);
        let generation = cx.read(|cx| app.read(cx).ui().github_prs_gen);
        cx.advance_clock(Duration::from_secs(60));
        cx.run_until_parked();
        assert_eq!(
            first_page_reads(&receipt),
            reads + 1,
            "{state:?} must not defer shared Open's ticker read"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().github_prs_gen),
            generation.wrapping_add(1)
        );
        assert_eq!(
            info(cx, &app),
            (vec![200], Some("chosen-next".into()), true)
        );
        cx.read(|cx| {
            let app = app.read(cx);
            assert_eq!(app.ui().github_pr_filter.common.state, state);
            assert_eq!(app.ui().github_prs.len(), 100);
        });
        let mut next = page(201..202, None);
        if state == StateFilter::Closed {
            next.prs[0].state = IssueState::Closed;
        }
        strip_append.send(Ok(next));
        cx.run_until_parked();
        assert_eq!(info(cx, &app), (vec![200, 201], None, false));
        measure(cx, win, "pr-home-row-201");
    }

    // Leaving the PR consumer resumes ordinary automatic first-page refresh;
    // accepted pages are not a persistent cache policy for graph/background UI.
    ready(page(1..101, Some("departed-page")));
    click(cx, win, "list-filter-state");
    click(cx, win, "list-filter-option-0-0");
    bottom(cx, win);
    ready(page(101..121, None));
    click(cx, win, "pr-filter-load-more");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    let reads = first_page_reads(&receipt);
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    cx.run_until_parked();
    cx.advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    assert_eq!(
        first_page_reads(&receipt),
        reads + 1,
        "departure resumes normal shared Open refresh"
    );
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("sidebar-page".into()), false)
    );
    assert_eq!(repo_fingerprint(&repo), before);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_periodic_pending_sidebar_and_other_collections");
}
