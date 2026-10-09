//! Bounded PR pages through the real table, sidebar, filters and owner receivers.
use crate::evidence_support::{deferred, pull_request};
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::IssueState;
use kagi_git::github::PrFetchError;

#[path = "pr_pagination_fixture.rs"]
mod fixture;
use fixture::{page, ready, refresh, sidebar_refresh_gh};

#[path = "pr_pagination_periodic.rs"]
mod periodic;
pub use periodic::{
    scenario_pr_paging_survives_periodic_tick,
    scenario_pr_periodic_pending_sidebar_and_other_collections,
};

fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(win.window_id(), id);
    cx.update_window(win, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("missing control {id}"))
}

fn click(cx: &mut VisualTestAppContext, win: AnyWindowHandle, id: &str) {
    let bounds = measure(cx, win, id);
    cx.simulate_click(win, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn bottom(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    let bounds = measure(cx, win, "pr-home-table-viewport");
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    measure(cx, win, "pr-home-table-viewport");
    cx.run_until_parked();
}

fn info(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> (Vec<u64>, Option<String>, bool) {
    cx.read(|cx| {
        let (keys, repo, cursor, loading) = e2e::pr_page_info(app.read(cx));
        assert_eq!(repo.as_deref(), Some("github.com/example/repo"));
        (keys.iter().map(|key| key.number).collect(), cursor, loading)
    })
}

fn assert_sidebar_page(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, phase: &str) {
    let (numbers, cursor, loading_more) = info(cx, app);
    assert_eq!(
        (numbers, cursor.is_some(), loading_more),
        ((1..101).collect(), true, false),
        "{phase}: the accepted sidebar page must retain continuation authority: {:?}",
        cx.read(|cx| e2e::pr_list_read_status(app.read(cx))),
    );
}

fn assert_lazy_demand(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, appended: bool) {
    cx.read(|cx| {
        let (visible, opened, pending, active) = e2e::pr_detail_demand(app.read(cx));
        assert!(
            active <= 2,
            "page append must retain bounded detail concurrency"
        );
        for (key, body) in &pending {
            assert!(
                visible.contains(key) || opened.contains(key),
                "offscreen unopened PR queued for detail: {key:?}"
            );
            if *body {
                assert!(opened.contains(key), "body work is only for opened PRs");
            }
        }
        if appended {
            let targets: std::collections::BTreeSet<_> = visible
                .iter()
                .chain(opened.iter())
                .chain(pending.iter().map(|(key, _)| key))
                .filter(|key| (101..121).contains(&key.number))
                .collect();
            assert!(
                targets.len() < 20,
                "appending a page must not demand details for all twenty new PRs"
            );
        } else {
            assert!(
                !visible.is_empty(),
                "the real table must report visible demand"
            );
            assert!(
                visible.len() < 100,
                "the detail controller cannot demand the whole loaded collection"
            );
        }
    });
}

pub fn scenario_pr_pagination(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo);
    refresh(cx, &app, page(1..101, Some("page-2")));
    app.update(cx, |app, cx| {
        app.github_host_logins
            .insert(Some("github.com".into()), "alice".into());
        app.show_pr_mode(cx);
    });
    cx.run_until_parked();
    let (task, failed) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    measure(cx, win, "pr-home-table-viewport");
    cx.run_until_parked();
    assert_lazy_demand(cx, &app, false);
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("page-2".into()), false),
        "overdraw cannot request page two"
    );
    bottom(cx, win);
    assert!(
        info(cx, &app).2,
        "the clipped table tail must request a page"
    );
    // Repeated drawn tail demand cannot overlap an outstanding request.
    bottom(cx, win);
    assert_eq!(info(cx, &app).0.len(), 100);
    failed.send(Err(PrFetchError::Network("offline page".into())));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("page-2".into()), false)
    );
    assert!(cx.read(|cx| app.read(cx).ui().github_error.is_some()));
    // The failure notice changes the viewport height. Scroll the actual
    // table again, rather than clicking an overdrawn/off-screen measurement.
    bottom(cx, win);
    let retry_bounds = measure(cx, win, "pr-main-page-retry");
    let viewport = measure(cx, win, "pr-home-table-viewport");
    assert!(
        viewport.contains(&retry_bounds.center()),
        "retry must be reachable inside the clipped table viewport",
    );
    let (task, retry) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "pr-main-page-retry");
    assert!(
        info(cx, &app).2,
        "the real native retry click must claim one continuation request",
    );
    let anchor = cx.read(|cx| {
        app.read(cx)
            .pr_mode()
            .unwrap()
            .dashboard_scroll
            .0
            .borrow()
            .base_handle
            .offset()
    });
    let row = measure(cx, win, "pr-home-row-100");
    cx.simulate_mouse_down(
        win,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_up(
        win,
        row.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().pr_menu.as_ref().map(|(pr, _)| pr.number)),
        Some(100)
    );
    // One overlapping row is ignored; twenty genuinely new rows are appended.
    let mut overlap = page(100..121, None);
    overlap.prs[0].title = "overlap must not replace the original row".into();
    retry.send(Ok(overlap));
    cx.run_until_parked();
    measure(cx, win, "pr-home-table-viewport");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ui().github_prs[99].title, "PR 100");
        assert!(app.ui().github_error.is_none());
        assert_eq!(
            app.ui().pr_menu.as_ref().map(|(pr, _)| pr.number),
            Some(100)
        );
        assert_eq!(
            app.pr_mode()
                .unwrap()
                .dashboard_scroll
                .0
                .borrow()
                .base_handle
                .offset(),
            anchor,
            "append retains the viewport anchor"
        );
    });
    assert_lazy_demand(cx, &app, true);
    // The preserved menu occludes the workspace. Dismiss it through the
    // actual outside-click route before demanding rows by scrolling.
    let viewport = measure(cx, win, "pr-home-table-viewport");
    cx.simulate_click(
        win,
        viewport.origin + gpui::point(gpui::px(4.), gpui::px(4.)),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().pr_menu.is_none()));
    bottom(cx, win);
    measure(cx, win, "pr-home-row-120");
    assert_eq!(
        info(cx, &app).0.len(),
        120,
        "a final page never drains again"
    );

    // A real zero-match InputState filter must retain an explicit continuation.
    refresh(cx, &app, page(1..101, Some("filtered-page")));
    click(cx, win, "list-filter-text");
    cx.simulate_keystrokes(win, "no matching PR");
    cx.run_until_parked();
    measure(cx, win, "pr-filter-load-more");
    assert_eq!(
        info(cx, &app),
        ((1..101).collect(), Some("filtered-page".into()), false),
        "client zero matches cannot auto-drain"
    );
    ready(page(101..121, None));
    click(cx, win, "pr-filter-load-more");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    click(cx, win, "list-filter-clear");

    // The sidebar's explicit demand shares the same bounded receiver, while a
    // real opened tab and its actual composer entity survive an append.
    refresh(cx, &app, page(1..101, Some("sidebar-page")));
    assert_sidebar_page(cx, &app, "after refreshing the sidebar first page");
    // The real local-ref failure refreshes L1 through gh. Answer that read
    // through the backend parser instead of letting the host's auth replace
    // the fixture's accepted continuation authority.
    let _gh = sidebar_refresh_gh();
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    let selected = cx.read(|cx| app.read(cx).ui().github_prs[0].clone());
    app.update(cx, |app, cx| app.pr_mode_open(&selected, cx));
    cx.run_until_parked();
    assert_sidebar_page(cx, &app, "after opening PR 1");
    measure(cx, win, "pr-composer-mode-toggle");
    let composer = cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .clone()
            .expect("real composer")
    });
    cx.update_window(win, |_, window, cx| {
        composer.update(cx, |input, cx| {
            input.set_value("retained reply", window, cx)
        })
    })
    .unwrap();
    let composer_id = composer.entity_id();
    drop(composer);
    cx.run_until_parked();
    measure(cx, win, "pr-composer-mode-toggle");
    let sidebar = measure(cx, win, "pr-mode-left-pane");
    let list_viewport = measure(cx, win, "pr-sidebar-list-viewport");
    assert_sidebar_page(
        cx,
        &app,
        "after drawing the composer and sidebar, before wheel",
    );
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: list_viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let demand = measure(cx, win, "pr-sidebar-load-more");
    assert!(
        demand.center().y >= sidebar.top() && demand.center().y <= sidebar.bottom(),
        "sidebar continuation must be actually in the clipped viewport"
    );
    ready(page(101..121, None));
    click(cx, win, "pr-sidebar-load-more");
    assert_eq!(info(cx, &app), ((1..121).collect(), None, false));
    assert_lazy_demand(cx, &app, true);
    cx.read(|cx| {
        let app = app.read(cx);
        let mode = app.pr_mode().unwrap();
        assert_eq!(mode.active, Some(0));
        assert_eq!(mode.tabs[0].pr.key(), selected.key());
        assert_eq!(
            app.pr_comment_input.as_ref().unwrap().entity_id(),
            composer_id
        );
        assert_eq!(
            app.pr_comment_input
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .as_str(),
            "retained reply"
        );
    });
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: list_viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let last_card = measure(cx, win, "pr-mode-card-120");
    assert!(
        last_card.center().y >= list_viewport.top()
            && last_card.center().y <= list_viewport.bottom(),
        "the appended sidebar tail must be reachable in its real clipped viewport"
    );
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_pagination");
}

pub fn scenario_pr_pagination_races(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo_a);
    refresh(cx, &app, page(1..101, Some("old-page")));
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    // Keep automatic tail demand out of these receiver-ordering legs; the
    // separate consumer scenario exercises unfiltered viewport demand.
    click(cx, win, "list-filter-text");
    cx.simulate_keystrokes(win, "PR");
    cx.run_until_parked();
    let (task, superseded) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, e2e::load_more_prs);
    refresh(cx, &app, page(201..202, Some("new-page")));
    superseded.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        (vec![201], Some("new-page".into()), false),
        "refresh rejects the old append"
    );
    e2e::queue_github_pr_fetch(gpui::Task::ready(Err(PrFetchError::Network(
        "refresh offline".into(),
    ))));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        (vec![201], None, false),
        "failed refresh preserves rows but revokes continuation"
    );
    // No injected task here: revoked continuation must return before transport.
    app.update(cx, e2e::load_more_prs);
    cx.run_until_parked();
    assert_eq!(info(cx, &app), (vec![201], None, false));
    refresh(cx, &app, page(201..202, Some("new-page")));

    let (task, wrong_repo) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, e2e::load_more_prs);
    let mut foreign = page(202..203, None);
    foreign.base_repo = "github.com/other/repo".into();
    foreign.prs[0].base_repo = foreign.base_repo.clone();
    wrong_repo.send(Ok(foreign));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        (vec![201], Some("new-page".into()), false),
        "wrong repository preserves retry cursor"
    );
    assert!(cx.read(|cx| app.read(cx).ui().github_error.is_some()));

    // Closed/All membership cannot borrow or replace the shared Open evidence.
    let (task, closed_first) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "list-filter-state");
    click(cx, win, "list-filter-option-0-1");
    let mut closed = page(301..302, Some("closed-page"));
    closed.prs[0].state = IssueState::Closed;
    closed_first.send(Ok(closed));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        (vec![301], Some("closed-page".into()), false)
    );
    let (task, old_closed) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, e2e::load_more_prs);
    ready(page(401..402, None));
    click(cx, win, "list-filter-state");
    click(cx, win, "list-filter-option-0-2");
    let mut closed_tail = page(302..303, None);
    closed_tail.prs[0].state = IssueState::Closed;
    old_closed.send(Ok(closed_tail));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app),
        (vec![401], None, false),
        "All rejects Closed continuation"
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .github_prs
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>()),
        vec![201]
    );

    // Leaving and re-entering the workspace is a different consumer visit.
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    refresh(cx, &app, page(1..101, Some("visit-page")));
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    let (task, old_visit) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, e2e::load_more_prs);
    app.update(cx, |app, cx| {
        app.show_graph_mode(cx);
        app.show_pr_mode(cx);
    });
    old_visit.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app).0,
        (1..101).collect::<Vec<_>>(),
        "old visit cannot publish append"
    );

    // Repository departure invalidates append visit ownership, not B's rows.
    refresh(cx, &app, page(1..101, Some("owner-page")));
    let owner_a = cx.read(|cx| app.read(cx).active_session().unwrap());
    let (task, background) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, e2e::load_more_prs);
    ready(page(501..502, None));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx));
    });
    cx.run_until_parked();
    background.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.ui().github_prs[0].number, 501);
        assert_eq!(
            app.ui[&owner_a].github_prs.len(),
            100,
            "departure rejects A's old append"
        );
    });
    // In contrast, a background first-page refresh still belongs to A.
    ready(page(1..101, Some("background-refresh")));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    let (task, first_page) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    ready(page(501..502, None));
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    first_page.send(Ok(page(701..702, None)));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(
            app.ui().github_prs[0].number,
            501,
            "background first page cannot replace B"
        );
        assert_eq!(
            app.ui[&owner_a].github_prs[0].number, 701,
            "background first page settles only A"
        );
    });
    ready(page(1..101, Some("detach-page")));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    let (task, detached) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    app.update(cx, |app, cx| {
        e2e::load_more_prs(app, cx);
        app.close_tab(0, cx);
    });
    ready(page(601..602, None));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_a, cx));
    });
    cx.run_until_parked();
    let new_owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    assert_ne!(owner_a, new_owner);
    detached.send(Ok(page(101..121, None)));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(!app.ui.contains_key(&owner_a));
        assert_eq!(app.ui().github_prs[0].number, 601);
    });
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS pr_pagination_races");
}

fn assert_pr_collection_retention(
    cx: &mut VisualTestAppContext,
    state: kagi_domain::list_filter::StateFilter,
    option: usize,
) {
    use kagi_domain::list_filter::StateFilter;
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo_a);
    refresh(cx, &app, page(1..101, None));
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    // This is a real user predicate, not a receiver-only mock. It also
    // leaves short collections explicitly pageable instead of auto-draining.
    click(cx, win, "list-filter-text");
    cx.simulate_keystrokes(win, "PR");
    cx.run_until_parked();
    let mut chosen = page(200..201, Some("chosen-collection-next"));
    chosen.prs[0].state = IssueState::Closed;
    if state == StateFilter::All {
        chosen.prs.push(pull_request(201, "PR 201", "main"));
    }
    let retained_numbers: Vec<_> = chosen.prs.iter().map(|pr| pr.number).collect();
    ready(chosen);
    click(cx, win, "list-filter-state");
    click(cx, win, &format!("list-filter-option-0-{option}"));
    assert_eq!(info(cx, &app).0, retained_numbers);
    measure(cx, win, "pr-home-row-200");
    let owner_a = cx.read(|cx| app.read(cx).active_session().unwrap());
    let (task, old_append) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "pr-filter-load-more");
    assert!(info(cx, &app).2);
    // Keep an actually opened, session-owned PR pane and its composing state
    // across the same round trip; preserving membership must not drop panes.
    click(cx, win, "pr-home-row-200");
    measure(cx, win, "pr-composer-mode-toggle");
    let composer = cx.read(|cx| {
        app.read(cx)
            .pr_comment_input
            .clone()
            .expect("real composer")
    });
    cx.update_window(win, |_, window, cx| {
        composer.update(cx, |input, cx| {
            input.replace(
                "draft retained across repository tabs".to_owned(),
                window,
                cx,
            );
        });
    })
    .unwrap();
    drop(composer);
    measure(cx, win, "pr-composer-mode-toggle");
    let retained_pane = cx.read(|cx| {
        let mode = app.read(cx).pr_mode().unwrap();
        let tab = &mode.tabs[mode.active.unwrap()];
        assert_eq!(tab.comment_draft, "draft retained across repository tabs");
        (tab.pr.key(), tab.selected_file)
    });
    ready(page(501..502, None));
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx));
    });
    cx.run_until_parked();
    // Returning performs the existing shared Open refresh, which must not
    // replace A's chosen Closed/All workspace membership.
    ready(page(1..101, None));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_session()), Some(owner_a));
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_pr_filter.common.state),
        state,
        "repository tab switching must retain the collection the user selected",
    );
    assert_eq!(info(cx, &app).0, retained_numbers);
    assert!(
        !info(cx, &app).2,
        "a departed append cannot hold the new visit's slot"
    );
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("retained PR workspace");
        let tab = &mode.tabs[mode.active.expect("opened PR stays selected")];
        assert_eq!(tab.pr.key(), retained_pane.0);
        assert_eq!(tab.selected_file, retained_pane.1);
        assert_eq!(tab.comment_draft, "draft retained across repository tabs");
    });
    app.update(cx, |app, cx| app.pr_mode_home(cx));
    cx.run_until_parked();
    measure(cx, win, "pr-home-row-200");
    if state == StateFilter::All {
        measure(cx, win, "pr-home-row-201");
    }
    let mut stale = page(202..203, None);
    stale.prs[0].state = IssueState::Closed;
    old_append.send(Ok(stale));
    cx.run_until_parked();
    assert_eq!(
        info(cx, &app).0,
        retained_numbers,
        "old-visit append cannot publish after return"
    );
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .github_prs
            .iter()
            .map(|pr| pr.number)
            .collect::<Vec<_>>()),
        (1..101).collect::<Vec<_>>(),
        "the retained strip cannot replace shared Open evidence",
    );
    ready(page(501..502, None));
    app.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).ui().github_prs[0].number), 501);
    unmount(cx, app, win);
}

pub fn scenario_pr_pagination_closed_tab_retention(cx: &mut VisualTestAppContext) {
    assert_pr_collection_retention(cx, kagi_domain::list_filter::StateFilter::Closed, 1);
    eprintln!("[gui-e2e] PASS pr_pagination_closed_tab_retention");
}

pub fn scenario_pr_pagination_all_tab_retention(cx: &mut VisualTestAppContext) {
    assert_pr_collection_retention(cx, kagi_domain::list_filter::StateFilter::All, 2);
    eprintln!("[gui-e2e] PASS pr_pagination_all_tab_retention");
}

fn assert_pending_pr_collection_return(
    cx: &mut VisualTestAppContext,
    state: kagi_domain::list_filter::StateFilter,
    option: usize,
) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().unwrap();
    let repo_b = fixture_b.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo_a);
    refresh(cx, &app, page(1..101, None));
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    cx.run_until_parked();
    let owner_a = cx.read(|cx| app.read(cx).active_session().unwrap());
    let (task, old_first_page) = deferred(cx);
    e2e::queue_github_pr_fetch(task);
    click(cx, win, "list-filter-state");
    click(cx, win, &format!("list-filter-option-0-{option}"));
    assert!(cx.read(|cx| e2e::pr_list_read_status(app.read(cx)).0));
    measure(cx, win, "pr-list-refreshing");
    ready(page(501..502, None));
    app.update(cx, |app, cx| assert!(app.open_repository(repo_b, cx)));
    cx.run_until_parked();
    ready(page(1..101, None));
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), Some(owner_a));
        assert_eq!(app.ui().github_pr_filter.common.state, state);
        let (keys, _, _, _) = e2e::pr_page_info(app);
        let (loading, error) = e2e::pr_list_read_status(app);
        assert!(
            !keys.is_empty() || loading || error.is_some(),
            "an interrupted first Closed/All read must not become authoritative zero on return",
        );
    });
    let mut stale = page(200..201, None);
    stale.prs[0].state = IssueState::Closed;
    old_first_page.send(Ok(stale));
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(e2e::pr_page_info(app).0.iter().all(|key| key.number != 200));
        assert_eq!(app.ui[&owner_a].github_prs[0].number, 1);
    });
    // The ordinary refresh action must recover the chosen collection, not
    // silently reset it to Open. No replacement transport seam is introduced.
    let mut fresh = page(300..301, None);
    fresh.prs[0].state = IssueState::Closed;
    ready(fresh);
    app.update(cx, |app, cx| app.pr_mode_refresh(cx));
    cx.run_until_parked();
    assert_eq!(info(cx, &app), (vec![300], None, false));
    measure(cx, win, "pr-home-row-300");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_pr_filter.common.state),
        state
    );
    unmount(cx, app, win);
}

pub fn scenario_pr_pagination_pending_closed_tab_return(cx: &mut VisualTestAppContext) {
    assert_pending_pr_collection_return(cx, kagi_domain::list_filter::StateFilter::Closed, 1);
    eprintln!("[gui-e2e] PASS pr_pagination_pending_closed_tab_return");
}

pub fn scenario_pr_pagination_pending_all_tab_return(cx: &mut VisualTestAppContext) {
    assert_pending_pr_collection_return(cx, kagi_domain::list_filter::StateFilter::All, 2);
    eprintln!("[gui-e2e] PASS pr_pagination_pending_all_tab_return");
}
