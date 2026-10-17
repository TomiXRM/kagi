//! #1652: pointer disclosure must survive focus redraws and navigator remounts.
//! Keep this beside the compact harness so both hosts use the same native setup.
use super::*;
use kagi::ui::pr_mode::PrFocus;
use kagi_domain::github::CiState;

const MINE: &str = "pr-mode-section-1";

fn folds(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> [bool; 4] {
    cx.read(|cx| app.read(cx).pr_mode().unwrap().sections_open)
}

fn seed_inbox(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    seed_prs(cx, app, 3);
    app.update(cx, |app, cx| {
        let ui = app.ui_mut().unwrap();
        for pr in &mut ui.github_prs {
            pr.ci = CiState::Failure;
        }
        // Deliberately overlap buckets: expanding Mine cannot fold Inbox or
        // turn the same PR's other section membership into a selection.
        ui.github_prs[0].reviewers = vec!["alice".into()];
        ui.github_prs[0].assignees = vec!["alice".into()];
        ui.github_prs_epoch = ui.github_prs_epoch.wrapping_add(1);
        cx.notify();
    });
}

fn pointer_disclosure(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    at: f32,
) {
    settle(cx, win);
    // Inbox can legitimately put Mine below a short popup's clipped viewport.
    // Reach it through the real scroll owner before measuring this disclosure.
    for _ in 0..12 {
        let viewport = bounds(win, "pr-sidebar-list-viewport");
        let target = bounds(win, MINE);
        if target.top() >= viewport.top() && target.bottom() <= viewport.bottom() {
            break;
        }
        let delta = if target.top() < viewport.top() {
            viewport.top() - target.top() + px(4.)
        } else {
            viewport.bottom() - target.bottom() - px(4.)
        };
        wheel(cx, win, viewport, f32::from(delta));
    }
    let before = folds(cx, app);
    let demand = cx.read(|cx| e2e::pr_detail_demand(app.read(cx)));
    let page = cx.read(|cx| e2e::pr_page_info(app.read(cx)));
    let list_read = cx.read(|cx| e2e::pr_list_read_status(app.read(cx)));
    let owner = cx.read(|cx| app.read(cx).active_session());
    let target = bounds(win, MINE);
    contained(
        bounds(win, "pr-sidebar-list-viewport"),
        target,
        "visible Mine hit area",
    );
    let pointer = point(target.left() + target.size.width * at, target.center().y);
    cx.simulate_mouse_move(win, pointer, None, Modifiers::none());
    settle(cx, win);
    cx.simulate_mouse_down(win, pointer, gpui::MouseButton::Left, Modifiers::none());
    // simulate_click sends down/up in one turn. A real mouse press can instead
    // draw the newly focused header before release; retain that armed click.
    settle(cx, win);
    assert_eq!(folds(cx, app), before, "mouse-down must not disclose early");
    cx.simulate_mouse_up(win, pointer, gpui::MouseButton::Left, Modifiers::none());
    settle(cx, win);
    let mut expected = before;
    expected[1] = !expected[1];
    assert_eq!(
        folds(cx, app),
        expected,
        "Mine toggles exactly once at {at}"
    );
    assert!(
        focused(cx, win, MINE),
        "pointer disclosure retains header focus"
    );
    section_ax(cx, win, MINE, Msg::PrSectionMine.t(), expected[1], false);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.active_session(), owner);
        assert_eq!(
            e2e::pr_detail_demand(app),
            demand,
            "disclosure admits no detail reads"
        );
        assert_eq!(
            e2e::pr_page_info(app),
            page,
            "disclosure admits no continuation"
        );
        assert_eq!(e2e::pr_list_read_status(app), list_read);
        assert!(app.write_busy_op.is_none(), "disclosure admits no writer");
        assert!(
            app.pr_mode().unwrap().tabs.is_empty(),
            "disclosure opens no PR"
        );
    });
}

fn repeated(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    for focus in [PrFocus::List, PrFocus::Commits, PrFocus::Files] {
        app.update(cx, |app, cx| {
            app.ui_mut().unwrap().pr_mode.as_mut().unwrap().focus = focus;
            cx.notify();
        });
        root(cx, app, win);
        // Both the label/chevron side and the count side are actionable.
        for at in [0.1, 0.5, 0.9, 0.5] {
            pointer_disclosure(cx, app, win, at);
        }
    }
    let before = folds(cx, app);
    press(cx, win, "enter");
    assert_ne!(folds(cx, app)[1], before[1], "Enter still discloses");
    press(cx, win, "space");
    assert_eq!(folds(cx, app), before, "Space closes only the same bucket");
    assert!(focused(cx, win, MINE));
}

fn cancel_page_preview(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let viewport = bounds(win, "pr-sidebar-list-viewport");
    let position = viewport.center();
    cx.simulate_event(
        win,
        ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(30.), px(0.))),
            touch_phase: TouchPhase::Started,
            ..Default::default()
        },
    );
    settle(cx, win);
    assert!(cx.read(|cx| e2e::sidebar_page_offset(app.read(cx))) > 0.);
    assert!(e2e::control_bounds(win.window_id(), "sidebar-gesture-shield").is_some());
    cx.simulate_event(
        win,
        ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(0.))),
            touch_phase: TouchPhase::Cancelled,
            ..Default::default()
        },
    );
    e2e::clear_control_bounds(win.window_id(), "sidebar-gesture-shield");
    settle(cx, win);
    assert_eq!(cx.read(|cx| e2e::sidebar_page_offset(app.read(cx))), 0.);
    assert!(e2e::control_bounds(win.window_id(), "sidebar-gesture-shield").is_none());
    repeated(cx, app, win);
}

pub fn scenario_pr_section_pointer(cx: &mut VisualTestAppContext) {
    interrupted_swipe(cx);
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let other = build_fixture();
    let log = tempfile::tempdir().unwrap();
    let calls = log.path().join("gh-calls");
    let _offline = crate::pr_fields_focus::OfflineGh::with_script(&format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 1\n",
        calls.display()
    ));
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        seed_inbox(cx, &app);
        click(cx, win, "compact-rail-prs");
        settle(cx, win);
        let transport = std::fs::read(&calls).unwrap_or_default();
        let repository = crate::macos::repo_fingerprint(fixture.path());
        repeated(cx, &app, win);
        press(cx, win, "escape");
        click(cx, win, "compact-rail-prs");
        repeated(cx, &app, win);
        let viewport = bounds(win, "pr-sidebar-list-viewport");
        pointer_disclosure(cx, &app, win, 0.5);
        wheel(cx, win, viewport, -90.);
        wheel(cx, win, viewport, 100000.);
        pointer_disclosure(cx, &app, win, 0.5);
        press(cx, win, "escape");

        let regular = Case {
            width: 1392_f32
                .max((theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil()),
            height: 883.,
            ..case
        };
        resize(cx, win, regular);
        repeated(cx, &app, win);
        click(cx, win, "sidebar-mode-graph");
        click(cx, win, "sidebar-mode-prs");
        repeated(cx, &app, win);
        if kagi_git::github::gh_available() {
            cancel_page_preview(cx, &app, win);
        }
        resize(cx, win, case);
        click(cx, win, "compact-rail-prs");
        repeated(cx, &app, win);
        assert_eq!(
            std::fs::read(&calls).unwrap_or_default(),
            transport,
            "disclosure and cached hosts admit no gh calls"
        );
        assert_eq!(crate::macos::repo_fingerprint(fixture.path()), repository);

        press(cx, win, "escape");
        app.update(cx, |app, cx| {
            app.open_repository(other.path().to_path_buf(), cx)
        });
        settle(cx, win);
        seed_inbox(cx, &app);
        click(cx, win, "compact-rail-prs");
        assert_eq!(
            folds(cx, &app),
            [true, false, false, false],
            "another owner starts with independent folds"
        );
        repeated(cx, &app, win);
        unmount(cx, app, win);
    });
}

/// #1652 causal seam: the gesture ends after the pointer leaves the sidebar.
/// A retained occluding shield makes every pointer disclosure inert while
/// keyboard activation and the pinned mode navigation remain available.
fn interrupted_swipe(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    let case = Case {
        width: 940.,
        height: 660.,
        zoom: 1.,
        locale: "ja",
        preset: "apple-light",
    };
    for (terminal, hide_sidebar) in [
        (TouchPhase::Ended, false),
        (TouchPhase::Cancelled, false),
        (TouchPhase::Ended, true),
        (TouchPhase::Cancelled, true),
    ] {
        let (app, win) = mount(cx, fixture.path(), case);
        seed_inbox(cx, &app);
        click(cx, win, "compact-rail-prs");
        press(cx, win, "escape");
        resize(
            cx,
            win,
            Case {
                width: 1392.,
                height: 883.,
                ..case
            },
        );
        let position = bounds(win, "pr-sidebar-list-viewport").center();
        cx.simulate_event(
            win,
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(point(px(30.), px(0.))),
                touch_phase: TouchPhase::Started,
                ..Default::default()
            },
        );
        settle(cx, win);
        assert!(cx.read(|cx| e2e::sidebar_page_offset(app.read(cx))) > 0.);
        assert!(e2e::control_bounds(win.window_id(), "sidebar-gesture-shield").is_some());
        if hide_sidebar {
            e2e::clear_control_bounds(win.window_id(), "pr-mode-left-pane");
            cx.dispatch_action(win, kagi::ui::commands::ToggleSidebar);
            settle(cx, win);
            assert!(!cx.read(|cx| app.read(cx).sidebar.visible));
            assert!(e2e::control_bounds(win.window_id(), "pr-mode-left-pane").is_none());
        }
        let outside = bounds(win, "pr-mode-center-pane").center();
        cx.simulate_mouse_move(win, outside, None, Modifiers::none());
        cx.simulate_event(
            win,
            ScrollWheelEvent {
                position: outside,
                delta: ScrollDelta::Pixels(point(px(0.), px(0.))),
                touch_phase: terminal,
                ..Default::default()
            },
        );
        for _ in 0..30 {
            cx.advance_clock(Duration::from_millis(16));
            e2e::clear_control_bounds(win.window_id(), "sidebar-gesture-shield");
            settle(cx, win);
        }
        assert_eq!(
            cx.read(|cx| e2e::sidebar_page_offset(app.read(cx))),
            0.,
            "{terminal:?} outside the sidebar must finish the short preview"
        );
        assert!(
            e2e::control_bounds(win.window_id(), "sidebar-gesture-shield").is_none(),
            "{terminal:?} outside the sidebar cannot strand a pointer-blocking shield"
        );
        if hide_sidebar {
            cx.dispatch_action(win, kagi::ui::commands::ToggleSidebar);
            settle(cx, win);
            assert!(cx.read(|cx| app.read(cx).sidebar.visible));
            bounds(win, "pr-mode-left-pane");
        }
        // The recovered owner must admit another gesture as well as clicks.
        cancel_page_preview(cx, &app, win);
        unmount(cx, app, win);
    }
}
