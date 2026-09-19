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

    // The PR workspace keeps mode navigation in the fixed-width left column
    // and gives every remaining pixel to its centre: there is no outer right
    // rail at all any more (ADR-0200), with or without a PR selected.
    app.update(cx, |app, cx| app.show_pr_mode(cx));
    for control in [
        "pr-mode-left-pane",
        "pr-mode-center-pane",
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
    let viewport = cx
        .update_window(win, |_, window, _| window.viewport_size())
        .unwrap();
    assert!(
        f32::from(viewport.width) - f32::from(center.origin.x + center.size.width) < 2.0,
        "the PR body must reach the window's right edge: no outer stack/files rail"
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
        e2e::queue_github_pr_fetch(cx.background_executor.spawn(async move {
            Ok(vec![kagi_domain::github::PullRequest {
                // One check, so the page's checks card exists to fold and
                // unfold (mock 7a/7b).
                checks: vec![kagi_domain::github::Check {
                    name: "build".into(),
                    workflow: "ci".into(),
                    state: kagi_domain::github::CiState::Success,
                    url: "https://example.com/run/1".into(),
                }],
                // `main` is the fixture's only branch, and a head the repository
                // actually has is what lets the PR open a tab at all - which is
                // what the feed assertions below need.
                ..pull_request(7, "cached", "main")
            }])
        }));
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
        // A PR tab only opens against branches the repository has actually
        // fetched, so give the fixture the remote-tracking ref its PR names.
        crate::macos::git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        app.update(cx, |app, cx| app.reload(cx));
        cx.run_until_parked();
        let cached = cx.read(|cx| app.read(cx).ui().github_prs.first().cloned());
        if let Some(pr) = cached {
            // The conversation lands through the injected read: two reviews
            // and one issue comment, which must become three entries of the
            // page's list. The first virtualized feed flattened the entries
            // *before* assigning what had landed and showed none (review
            // finding, w5:p19).
            e2e::queue_github_pr_conversation(cx.background_executor.spawn(async move {
                use kagi_domain::github::{Comment, Review};
                (
                    Ok((
                        vec![
                            Review {
                                author: "alice".into(),
                                state: "APPROVED".into(),
                                body: "looks good".into(),
                                submitted_at: "2026-09-01T00:00:00Z".into(),
                            },
                            Review {
                                author: "bob".into(),
                                state: "COMMENTED".into(),
                                body: "one nit".into(),
                                submitted_at: "2026-09-02T00:00:00Z".into(),
                            },
                        ],
                        vec![Comment {
                            author: "carol".into(),
                            body: "thanks".into(),
                            created_at: "2026-09-03T00:00:00Z".into(),
                        }],
                    )),
                    Ok(Vec::new()),
                )
            }));
            app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
            cx.run_until_parked();
            cx.read(|cx| {
                let m = app.read(cx).pr_mode().expect("PR mode");
                let tab = &m.tabs[m.active.unwrap()];
                assert!(tab.conversation_loaded, "the injected conversation landed");
                assert_eq!(
                    tab.feed_entries.len(),
                    3,
                    "every review and comment that landed is an entry of the page"
                );
            });
            // ADR-0200: 概要 and レビュー are two anchors into ONE virtualized
            // list, and the tabs are navigation into it. Each tab must draw
            // its own anchor, and both must be looking at the same list - the
            // same item count - where a tab that swapped the body would give
            // each view a list of its own. (The other anchor may legitimately
            // be off screen: that is what virtualization means.)
            let mut counts = Vec::new();
            for (view, anchor) in [
                (kagi::ui::pr_mode::PrView::Overview, "pr-mode-headline"),
                (kagi::ui::pr_mode::PrView::Review, "pr-feed-review"),
            ] {
                app.update(cx, |app, cx| app.pr_mode_show(view, cx));
                e2e::clear_control_bounds(win.window_id(), anchor);
                // A virtualized list measures its items on one frame and lays
                // them out on the next, so two draws are what "the page is on
                // screen" means here.
                for _ in 0..2 {
                    cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                        .unwrap();
                }
                assert!(
                    measure(cx, win, anchor).is_some(),
                    "{view:?} must reveal its own anchor on the page"
                );
                counts.push(cx.read(|cx| {
                    let m = app.read(cx).pr_mode().expect("PR mode");
                    m.tabs[m.active.unwrap()].feed_list.item_count()
                }));
            }
            assert!(
                counts[0] == counts[1] && counts[0] > 2,
                "both tabs read one list: {counts:?}"
            );
            // The レビュー tab is on screen: the entries under its heading
            // must actually be drawn, not merely counted (user report: "PR
            // reviews are not shown").
            for _ in 0..2 {
                cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                    .unwrap();
            }
            let drawn: Vec<bool> = (0..3)
                .map(|i| {
                    e2e::control_bounds(win.window_id(), &format!("pr-feed-entry-{i}")).is_some()
                })
                .collect();
            let heights: Vec<Option<f32>> = (0..3)
                .map(|i| {
                    e2e::control_bounds(win.window_id(), &format!("pr-feed-entry-{i}"))
                        .map(|b| f32::from(b.size.height))
                })
                .collect();
            let _ = heights;
            assert!(
                drawn.iter().any(|d| *d),
                "at least the first entry under the heading is drawn: {drawn:?}"
            );
            // ...and the heading sits at the top of the page, not at its
            // bottom edge with the reviews below the fold: pressing レビュー
            // must show reviews, which is the whole point of the tab.
            let heading = e2e::control_bounds(win.window_id(), "pr-feed-review")
                .expect("the heading is drawn after the レビュー tab");
            let pane = e2e::control_bounds(win.window_id(), "pr-mode-center-pane")
                .expect("the centre pane is measured");
            let from_top = f32::from(heading.origin.y) - f32::from(pane.origin.y);
            assert!(
                from_top < f32::from(pane.size.height) / 2.0,
                "the レビュー tab must put its heading in the upper half of the page, got {from_top}px from the top of a {}px pane",
                f32::from(pane.size.height)
            );

            // mock 7a/7b: the checks card is folded on the page, and opens to
            // the per-check rows in place. The fixture PR carries one check,
            // so the card exists and the disclosure must change the pane's
            // height rather than open a second surface.
            let folded = measure(cx, win, "pr-mode-checks").expect("the checks card is drawn");
            app.update(cx, |app, cx| app.pr_mode_toggle_checks(cx));
            e2e::clear_control_bounds(win.window_id(), "pr-mode-checks");
            cx.update_window(win, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            let opened = measure(cx, win, "pr-mode-checks").expect("the checks card stays drawn");
            assert!(
                f32::from(opened.size.height) > f32::from(folded.size.height),
                "opening the checks card must reveal its rows in place"
            );
            app.update(cx, |app, cx| app.pr_mode_toggle_checks(cx));

            // ADR-0200 §11: the gear on a properties row opens the field
            // picker with what the PR already carries, before any read of
            // what the repository offers has returned - so a value can be
            // removed offline. Confirm is dead until something changed.
            app.update(cx, |app, cx| {
                app.open_pr_fields_modal(kagi::ui::modals::PrField::Reviewers, cx)
            });
            cx.read(|cx| {
                let modal = app
                    .read(cx)
                    .pr_fields_modal()
                    .cloned()
                    .expect("the picker is the active modal");
                assert_eq!(modal.number, 7);
                assert_eq!(
                    modal.selected, modal.current,
                    "opens on the PR's own values"
                );
            });
            app.update(cx, |app, cx| app.pr_fields_toggle("octocat".into(), cx));
            cx.read(|cx| {
                let modal = app.read(cx).pr_fields_modal().cloned().unwrap();
                assert!(modal.selected.contains(&"octocat".to_string()));
                let (add, remove) =
                    kagi_domain::github::PrFieldEdit::diff(&modal.current, &modal.selected);
                assert_eq!(add, vec!["octocat".to_string()]);
                assert!(remove.is_empty());
            });
            app.update(cx, |app, cx| {
                app.clear_pr_fields_modal();
                cx.notify();
            });
            cx.read(|cx| assert!(app.read(cx).pr_fields_modal().is_none()));

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
