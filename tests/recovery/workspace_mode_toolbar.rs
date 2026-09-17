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
    ); 5] = [
        ("graph", |_, _| {}),
        ("prs", |app, cx| app.show_pr_mode(cx)),
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
        if matches!(mode, WorkspaceMode::Graph | WorkspaceMode::Prs) {
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

    // The PR list is the same left sidebar interaction surface. This minimal
    // snap implementation keeps the current page until release, then commits.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    assert!(!repo_actions_drawn(cx, win));
    let swipe_position = e2e::control_bounds(win.window_id(), "sidebar-mode-nav")
        .expect("PR mode navigation is drawn")
        .center();
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
