//! Pull/Push/Branch/Stash/Pop/Undo/Redo/Terminal belong to Graph. PRs, Editor and
//! Analyze must not draw them, and returning to Graph brings them back.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi::ui::workspace_mode::WorkspaceMode;

const REPO_ACTIONS: &str = "tb-repo-actions";

/// Draw one fresh frame and report whether it laid out the repo actions.
fn repo_actions_drawn(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> bool {
    cx.run_until_parked();
    let id = win.window_id();
    e2e::clear_control_bounds(id, REPO_ACTIONS);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(id, REPO_ACTIONS).is_some()
}

fn swipe_phase(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    position: gpui::Point<gpui::Pixels>,
    x: f32,
    touch_phase: gpui::TouchPhase,
) {
    cx.simulate_event(
        win,
        gpui::ScrollWheelEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(x), gpui::px(0.))),
            touch_phase,
            ..Default::default()
        },
    );
}

pub fn scenario_workspace_mode_toolbar(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, win) = mount(cx, &repo);

    // A non-default width must be shared, not merely equal by coincidence.
    app.update(cx, |app, cx| {
        app.sidebar.width = 287.0;
        cx.notify();
    });
    let mut graph_nav_width = None;
    let steps: [(
        &str,
        fn(&mut kagi::ui::KagiApp, &mut gpui::Context<kagi::ui::KagiApp>),
    ); 6] = [
        ("graph", |_, _| {}),
        ("prs", |app, cx| app.show_pr_mode(cx)),
        ("issues", |app, cx| app.show_issues_mode(cx)),
        ("editor", |app, cx| app.show_editor_mode(cx)),
        ("analyze", |app, cx| app.open_ecosystem_view(cx)),
        ("graph again", |app, cx| app.show_graph_mode(cx)),
    ];
    for (name, enter) in steps {
        app.update(cx, |app, cx| enter(app, cx));
        let mode = cx.read(|cx| app.read(cx).workspace_mode());
        let is_graph = mode == WorkspaceMode::Graph;
        assert_eq!(is_graph, name.starts_with("graph"), "{name}: mode {mode:?}");
        e2e::clear_control_bounds(win.window_id(), "sidebar-mode-nav");
        assert_eq!(
            repo_actions_drawn(cx, win),
            is_graph,
            "{name} ({mode:?}): repo actions drawn only in Graph"
        );
        if matches!(
            mode,
            WorkspaceMode::Graph | WorkspaceMode::Prs | WorkspaceMode::Issues
        ) {
            let bounds = e2e::control_bounds(win.window_id(), "sidebar-mode-nav")
                .expect("mode navigation is drawn");
            let width = f32::from(bounds.size.width);
            let expected = *graph_nav_width.get_or_insert(width);
            assert!(width > 200.0, "navigation must fill the sidebar");
            assert!(
                (width - expected).abs() < 1.0,
                "{name}: {width} != {expected}"
            );
        }
    }

    // The PR workspace keeps mode navigation in the fixed-width left column,
    // gives the remaining width to its centre dashboard, and does not reserve
    // a right rail until a PR is selected.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    for control in [
        "pr-mode-left-pane",
        "pr-mode-center-pane",
        "pr-mode-right-pane",
        "sidebar-mode-nav",
    ] {
        e2e::clear_control_bounds(win.window_id(), control);
    }
    assert!(!repo_actions_drawn(cx, win));
    let left =
        e2e::control_bounds(win.window_id(), "pr-mode-left-pane").expect("PR list column is drawn");
    let center =
        e2e::control_bounds(win.window_id(), "pr-mode-center-pane").expect("PR center is drawn");
    let nav = e2e::control_bounds(win.window_id(), "sidebar-mode-nav")
        .expect("PR mode navigation is drawn");
    let swipe_position = nav.center();
    assert!(
        f32::from(nav.origin.x) >= f32::from(left.origin.x)
            && f32::from(nav.origin.x + nav.size.width)
                <= f32::from(left.origin.x + left.size.width),
        "mode navigation must stay inside the PR list column"
    );
    assert!(
        f32::from(center.origin.x) >= f32::from(left.origin.x + left.size.width),
        "PR center must follow the fixed left column"
    );
    assert!(
        f32::from(center.size.width) > f32::from(left.size.width),
        "dashboard must receive the remaining workspace width"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "pr-mode-right-pane").is_none(),
        "right stack/files rail must be absent without a selected PR"
    );

    // Entering Issues starts its list request at the UI boundary, before any
    // completion can land. The takeover owns all three columns and exposes the
    // loading state rather than borrowing the graph or PR workspace.
    app.update(cx, |app, cx| app.show_issues_mode(cx));
    for control in [
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "issue-mode-right-pane",
        "issue-mode-list-loading",
    ] {
        e2e::clear_control_bounds(win.window_id(), control);
    }
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Issues
    );
    for control in [
        "issue-mode-left-pane",
        "issue-mode-center-pane",
        "issue-mode-right-pane",
        "issue-mode-list-loading",
    ] {
        assert!(
            e2e::control_bounds(win.window_id(), control).is_some(),
            "{control} must be visible when Issues opens"
        );
    }

    app.update(cx, |app, cx| app.show_empty_issues_for_e2e(cx));
    e2e::clear_control_bounds(win.window_id(), "issue-mode-list-empty");
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert!(
        e2e::control_bounds(win.window_id(), "issue-mode-list-empty").is_some(),
        "successful empty Issue slice must have a visible state"
    );

    // One committed gesture moves exactly one adjacent page. At either edge a
    // further gesture is inert; without gh the Graph edge is inert as well.
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    if kagi_git::github::gh_available() {
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues,
            "right edge must not move"
        );
        swipe_phase(cx, win, swipe_position, 70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );
    } else {
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Graph,
            "swipe must not enter a GitHub workspace without gh"
        );
    }

    app.update(cx, |app, cx| app.show_pr_mode(cx));
    swipe_phase(cx, win, swipe_position, 30.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Moved);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "swipe must not switch modes before release"
    );
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph
    );
    swipe_phase(cx, win, swipe_position, -100.0, gpui::TouchPhase::Ended);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph,
        "momentum without a new gesture must snap back"
    );

    // Opening a modal occludes the sidebar, so it must cancel the in-flight
    // gesture at the canonical modal transition rather than waiting for wheel
    // input that cannot reach the sidebar. Start in PRs so the assertion is
    // independent of whether `gh` is available in the test environment.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    assert!(!repo_actions_drawn(cx, win));
    swipe_phase(cx, win, swipe_position, 30.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Moved);
    app.update(cx, |app, _| e2e::deliver_app_notice(app, "swipe blocker"));
    assert!(cx.read(|cx| app.read(cx).app_notice().is_some()));
    app.update(cx, |app, _| app.clear_app_notice());
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "a gesture interrupted by a modal must snap back"
    );

    assert_eq!(before, repo_fingerprint(&repo), "repo mutated");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS workspace_mode_toolbar");
}
