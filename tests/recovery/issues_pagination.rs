//! Exercise the production virtual viewport, append, failure and retry path.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::{Issue, IssueListSnapshot, IssueListTab, IssueState};
use kagi_git::github::PrFetchError;

fn page(numbers: std::ops::Range<u64>, next_cursor: Option<&str>) -> IssueListSnapshot {
    IssueListSnapshot {
        issues: numbers
            .map(|number| Issue {
                number,
                title: format!("Issue {number}"),
                state: IssueState::Open,
                url: format!("https://github.com/example/fixture/issues/{number}"),
                author: String::new(),
                assignees: Vec::new(),
                labels: Vec::new(),
                body: String::new(),
                comments: Vec::new(),
                comment_count: 0,
                created_at: String::new(),
                updated_at: format!("{:020}", 1000 - number),
            })
            .collect(),
        mentioned_numbers: Vec::new(),
        base_repo: "github.com/example/fixture".into(),
        next_cursor: next_cursor.map(str::to_owned),
    }
}

fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    id: &str,
) -> gpui::Bounds<gpui::Pixels> {
    e2e::clear_control_bounds(win.window_id(), id);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(win.window_id(), id).unwrap_or_else(|| panic!("missing control {id}"))
}

fn scroll_bottom(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    let bounds = measure(cx, win, "issue-main-list");
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: bounds.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    // Virtual rows enter the viewport during layout, not in the wheel callback.
    measure(cx, win, "issue-main-list");
    cx.run_until_parked();
}

pub fn scenario_issues_pagination(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, win) = mount(cx, &repo);
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page(1..101, Some("page-2")))));
    app.update(cx, |app, cx| {
        app.seed_issue_composer_for_e2e(cx);
        app.show_issues_mode(cx);
    });
    cx.run_until_parked();
    let recent = measure(cx, win, "issue-filter-tab-3");
    cx.simulate_click(win, recent.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let owner = cx.read(|cx| *app.read(cx).ui.keys().next().expect("fixture owner"));
    let (task, failed_page) = deferred(cx);
    KagiApp::queue_issue_list_fetch_for_e2e(task);
    measure(cx, win, "issue-main-list");
    cx.run_until_parked();
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issues.len(), 100);
        assert!(
            !ui.github_issues_loading_more,
            "overdraw must not fetch unseen pages"
        );
    });
    scroll_bottom(cx, win);
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert!(
            ui.github_issues_loading_more,
            "entering the list tail must request another page: top={:?}, viewport={:?}, items={}",
            ui.github_issues_list.logical_scroll_top(),
            ui.github_issues_list.viewport_bounds(),
            ui.github_issues_list.item_count(),
        );
        assert_eq!(ui.github_issues.len(), 100);
    });
    failed_page.send(Err(PrFetchError::Network("offline".into())));
    cx.run_until_parked();
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issues.len(), 100);
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-2"));
        assert!(ui.github_issues_error.is_some());
        assert!(!ui.github_issues_loading_more);
    });
    scroll_bottom(cx, win);
    let retry = measure(cx, win, "issue-main-page-retry");
    let (task, second_page) = deferred(cx);
    KagiApp::queue_issue_list_fetch_for_e2e(task);
    cx.simulate_click(win, retry.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let before_append = cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert!(ui.github_issues_loading_more);
        ui.github_issues_list.logical_scroll_top().item_ix
    });
    second_page.send(Ok(page(101..201, Some("page-3"))));
    cx.run_until_parked();
    measure(cx, win, "issue-main-list");
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(
            ui.github_issues
                .iter()
                .map(|issue| issue.number)
                .collect::<Vec<_>>(),
            (1..201).collect::<Vec<_>>()
        );
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-3"));
        assert!(ui.github_issues_error.is_none());
        assert_eq!(
            ui.github_issues_list.logical_scroll_top().item_ix,
            before_append,
            "append must retain the viewport anchor"
        );
    });
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page(201..202, None))));
    scroll_bottom(cx, win);
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issues.len(), 201);
        assert!(ui.github_issues_cursor.is_none());
        assert!(!ui.github_issues_loading_more);
    });
    // Refresh back to one page, and have that page mention half its rows: a
    // client-side membership predicate with a nonempty — but partial — match
    // set, which is exactly what must not keep draining the repository.
    let mut mentioning = page(1..101, Some("page-2"));
    mentioning.mentioned_numbers = (1..51).collect();
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(mentioning)));
    app.update(cx, |app, cx| app.refresh_github_issues(cx));
    cx.run_until_parked();
    measure(cx, win, "issue-main-list");
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issues.len(), 100);
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-2"));
        assert_eq!(ui.github_issues_list.logical_scroll_top().item_ix, 0);
    });
    let filtered = measure(cx, win, "issue-filter-tab-2");
    cx.simulate_click(win, filtered.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issue_tab, IssueListTab::MentioningMe);
        assert_eq!(ui.github_issue_mentions.len(), 50, "a partial match set");
    });
    // No page is queued here on purpose: an automatic continuation would fall
    // through to the real transport and surface as an error or a moved cursor.
    scroll_bottom(cx, win);
    measure(cx, win, "issue-main-list");
    cx.run_until_parked();
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert!(
            !ui.github_issues_loading_more,
            "a filtered subset must not auto-page at its tail"
        );
        assert_eq!(ui.github_issues.len(), 100);
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-2"));
        assert!(ui.github_issues_error.is_none());
    });
    let load_more = measure(cx, win, "issue-filter-load-more");
    let mut appended = page(101..201, Some("page-3"));
    appended.mentioned_numbers = (101..151).collect();
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(appended)));
    cx.simulate_click(win, load_more.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    measure(cx, win, "issue-main-list");
    cx.run_until_parked();
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(
            ui.github_issues.len(),
            200,
            "the explicit control continues through the same pagination path"
        );
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-3"));
        assert_eq!(ui.github_issue_mentions.len(), 100);
        assert!(ui.github_issues_error.is_none());
        assert!(
            !ui.github_issues_loading_more,
            "one click continues once; the new tail must not re-arm"
        );
    });
    // The expanded Mentioning collection places the Recent header below its
    // rows. Scroll that real sidebar viewport before clicking the header.
    let sidebar = measure(cx, win, "issue-mode-left-pane");
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: sidebar.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let recent = measure(cx, win, "issue-filter-tab-3");
    cx.simulate_click(win, recent.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().github_issue_tab),
        IssueListTab::RecentlyUpdated
    );
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page(201..301, None))));
    scroll_bottom(cx, win);
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(
            ui.github_issues.len(),
            300,
            "an unfiltered tail still pages on its own"
        );
        assert!(ui.github_issues_cursor.is_none());
        assert!(!ui.github_issues_loading_more);
    });

    // Settle the append's one derived-view recomputation, then exercise a
    // separately drawn wheel frame. Scrolling back toward the top also puts
    // the real filter input back in the virtual viewport for the positive
    // control below.
    let list = measure(cx, win, "issue-main-list");
    let scroll_before = cx.read(|cx| {
        app.read(cx).ui[&owner]
            .github_issues_list
            .logical_scroll_top()
            .item_ix
    });
    let recomputations_before_scroll = KagiApp::issue_view_recomputations_for_e2e();
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position: list.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    measure(cx, win, "issue-main-list");
    let scroll_after = cx.read(|cx| {
        app.read(cx).ui[&owner]
            .github_issues_list
            .logical_scroll_top()
            .item_ix
    });
    assert!(
        scroll_after < scroll_before,
        "the 300-row production viewport must actually move on wheel input: before={scroll_before}, after={scroll_after}"
    );
    assert_eq!(
        KagiApp::issue_view_recomputations_for_e2e(),
        recomputations_before_scroll,
        "a drawn scroll frame must reuse the derived Issue view"
    );

    // One real InputState Change mutates IssueFilter. The following full-window
    // draw renders both the sidebar and main feed; their shared cache must run
    // apply_issues once, rather than once per consumer.
    let filter_input = measure(cx, win, "list-filter-text");
    cx.simulate_click(win, filter_input.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let recomputations_before_filter = KagiApp::issue_view_recomputations_for_e2e();
    cx.simulate_keystrokes(win, "3");
    cx.run_until_parked();
    measure(cx, win, "issue-main-list");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui[&owner]
            .github_issue_filter
            .common
            .text
            .clone()),
        "3",
        "the positive control must mutate the production IssueFilter"
    );
    assert_eq!(
        KagiApp::issue_view_recomputations_for_e2e(),
        recomputations_before_filter + 1,
        "one filter mutation must recompute once across the sidebar and main feed"
    );
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS issues_pagination");
}
