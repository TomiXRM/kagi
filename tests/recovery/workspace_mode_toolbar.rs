//! Pull/Push/Branch/Stash/Pop/Undo/Redo/Terminal belong to Graph. PRs, Editor and
//! Analyze must not draw them, and returning to Graph brings them back.
use crate::evidence_support::pull_request;
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

/// Draw one fresh frame and report the control's laid-out bounds.
fn measure(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    control: &str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    e2e::clear_control_bounds(win.window_id(), control);
    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    e2e::control_bounds(win.window_id(), control)
}

/// Run the sidebar's settle animation to rest (ADR-0199). The settle is a
/// timer-driven spring, so the test clock has to be advanced past it before
/// the page it committed to becomes the active workspace.
fn settle_sidebar(cx: &mut VisualTestAppContext) {
    cx.advance_clock(std::time::Duration::from_millis(600));
    cx.run_until_parked();
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

    // ── Sidebar gesture navigation (ADR-0199) ─────────────────────
    // A gesture slides the sidebar's pages and nothing else: the main pane
    // neither moves nor changes content until the sidebar has settled.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    let left_before = measure(cx, win, "pr-mode-left-pane").expect("PR list column is drawn");
    let center_before = measure(cx, win, "pr-mode-center-pane").expect("PR center is drawn");
    swipe_phase(cx, win, swipe_position, 70.0, gpui::TouchPhase::Started);
    let left_mid = measure(cx, win, "pr-mode-left-pane").expect("sidebar shell stays drawn");
    let center_mid = measure(cx, win, "pr-mode-center-pane").expect("PR center stays drawn");
    // Graph is local Git data, flattened every frame, so the page this gesture
    // heads for is previewed as the real navigator — never as a shell.
    e2e::clear_control_bounds(win.window_id(), "sidebar-adjacent-page-shell");
    let adjacent =
        measure(cx, win, "sidebar-adjacent-page").expect("the adjacent page follows it in");
    assert!(
        e2e::control_bounds(win.window_id(), "sidebar-adjacent-page-shell").is_none(),
        "an already-loaded page is previewed with its own content"
    );
    assert!(
        measure(cx, win, "sidebar-gesture-shield").is_some(),
        "the gesture must own the wheel, so the page under it cannot scroll"
    );
    let offset = cx.read(|cx| e2e::sidebar_page_offset(app.read(cx)));
    assert_eq!(
        (center_before.origin.x, center_before.size.width),
        (center_mid.origin.x, center_mid.size.width),
        "the main pane must not move while the sidebar is dragged"
    );
    assert_eq!(
        (left_before.origin.x, left_before.size.width),
        (left_mid.origin.x, left_mid.size.width),
        "the sidebar shell is fixed; only the pages inside it slide"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "the workspace must not change during a gesture"
    );
    assert!(
        offset > 0.0 && offset < 70.0,
        "the resisted sidebar offset ({offset}px) must trail the 70px gesture"
    );
    assert!(
        offset < f32::from(left_mid.size.width),
        "one gesture may never carry the sidebar past one page"
    );
    assert_eq!(
        adjacent.size.width, left_before.size.width,
        "a sliding page is translated, never re-laid out narrower"
    );

    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "the committed page must wait for the sidebar to settle"
    );
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Graph
    );
    assert!(
        measure(cx, win, "sidebar-adjacent-page").is_none(),
        "a settled sidebar shows one page at offset 0"
    );
    assert!(
        measure(cx, win, "sidebar-gesture-shield").is_none(),
        "a settled sidebar hands the wheel back to the page"
    );

    // A GitHub page has a shell only until its list has arrived.
    if kagi_git::github::gh_available() {
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        assert!(
            measure(cx, win, "sidebar-adjacent-page-shell").is_some(),
            "an unloaded PR page has nothing to preview but its shape"
        );
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);

        // Cache one PR through the real fetch path, then gesture again.
        app.update(cx, |app, cx| app.show_graph_mode(cx));
        e2e::queue_github_pr_fetch(
            cx.background_executor
                .spawn(async move { Ok(vec![pull_request(7, "cached", "cached-head")]) }),
        );
        app.update(cx, |app, cx| app.refresh_github_prs(cx));
        cx.run_until_parked();
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        // Forget the shell the *previous* gesture drew, so this frame decides.
        e2e::clear_control_bounds(win.window_id(), "sidebar-adjacent-page-shell");
        assert!(
            measure(cx, win, "sidebar-adjacent-page").is_some(),
            "the PR page still slides in"
        );
        assert!(
            e2e::control_bounds(win.window_id(), "sidebar-adjacent-page-shell").is_none(),
            "a cached PR list must be shown instead of the shell"
        );
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );

        // ADR-0200: the lane pane belongs to the PR on screen. Home keeps its
        // tabs, so a pane gated on "any tab open" stood there with the lanes
        // of the PR just left (user report).
        let cached = cx.read(|cx| app.read(cx).ui().github_prs.first().cloned());
        if let Some(pr) = cached {
            app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
            cx.run_until_parked();
            app.update(cx, |app, cx| app.pr_mode_home(cx));
            assert!(
                measure(cx, win, "pr-mode-lane-pane").is_none(),
                "back on the home list there is no PR to draw a lane for"
            );
        }
    }

    // Releasing under the 20% commit boundary returns to the origin page.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "40px of a 287px sidebar is under the commit boundary"
    );

    // One committed gesture moves exactly one adjacent page, however far it
    // travels. At either edge a further gesture is inert; without gh the Graph
    // edge is inert as well.
    app.update(cx, |app, cx| app.show_graph_mode(cx));
    if kagi_git::github::gh_available() {
        swipe_phase(cx, win, swipe_position, -2000.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, -2000.0, gpui::TouchPhase::Moved);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs,
            "a huge gesture still moves exactly one page"
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues
        );
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Issues,
            "right edge must not move"
        );
        swipe_phase(cx, win, swipe_position, 70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Prs
        );
    } else {
        swipe_phase(cx, win, swipe_position, -70.0, gpui::TouchPhase::Started);
        swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
        settle_sidebar(cx);
        assert_eq!(
            cx.read(|cx| app.read(cx).workspace_mode()),
            WorkspaceMode::Graph,
            "swipe must not enter a GitHub workspace without gh"
        );
    }

    // Momentum scroll arrives as Moved/Ended with no Started (macOS maps the
    // phase that way), so it must not navigate on its own.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    swipe_phase(cx, win, swipe_position, 100.0, gpui::TouchPhase::Moved);
    swipe_phase(cx, win, swipe_position, 100.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "momentum without a new gesture must not navigate"
    );

    // Opening a modal occludes the sidebar, so it must cancel the in-flight
    // gesture at the canonical modal transition rather than waiting for wheel
    // input that cannot reach the sidebar.
    assert!(!repo_actions_drawn(cx, win));
    swipe_phase(cx, win, swipe_position, 30.0, gpui::TouchPhase::Started);
    swipe_phase(cx, win, swipe_position, 40.0, gpui::TouchPhase::Moved);
    app.update(cx, |app, _| e2e::deliver_app_notice(app, "swipe blocker"));
    assert!(cx.read(|cx| app.read(cx).app_notice().is_some()));
    app.update(cx, |app, _| app.clear_app_notice());
    swipe_phase(cx, win, swipe_position, 0.0, gpui::TouchPhase::Ended);
    settle_sidebar(cx);
    assert_eq!(
        cx.read(|cx| app.read(cx).workspace_mode()),
        WorkspaceMode::Prs,
        "a gesture interrupted by a modal must snap back"
    );

    assert_eq!(before, repo_fingerprint(&repo), "repo mutated");
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS workspace_mode_toolbar");
}
