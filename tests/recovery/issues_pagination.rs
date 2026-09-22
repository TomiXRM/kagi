//! Exercise the production virtual viewport, append, failure and retry path.
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::{Issue, IssueListSnapshot, IssueState};
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
    KagiApp::queue_issue_list_fetch_for_e2e(gpui::Task::ready(Ok(page(1..101, Some("page-2")))));
    app.update(cx, |app, cx| app.refresh_github_issues(cx));
    cx.run_until_parked();
    measure(cx, win, "issue-main-list");
    cx.read(|cx| {
        let ui = &app.read(cx).ui[&owner];
        assert_eq!(ui.github_issues.len(), 100);
        assert_eq!(ui.github_issues_cursor.as_deref(), Some("page-2"));
        assert_eq!(ui.github_issues_list.logical_scroll_top().item_ix, 0);
    });
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS issues_pagination");
}
