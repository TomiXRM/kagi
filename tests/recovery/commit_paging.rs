//! #980 — raw Home/End/Page keys page the graph only when it is visible and its root owns focus.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};

use crate::app_conflict::content_fixture;
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};

const EXTRA: usize = 198;
const LAST: usize = EXTRA + 1;

fn graph(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Option<usize>, usize, bool) {
    let window_id = window.window_id();
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        let app = app.read(cx);
        let ui = app.ui();
        let viewport =
            e2e::control_bounds(window_id, "commit-list-viewport").expect("commit viewport drawn");
        let handle = ui.commit_scroll_handle.0.borrow();
        let (_, height) = app.commit_row_geometry_for_e2e(0);
        let row_height = gpui::px(height);
        let page = ((f32::from(handle.base_handle.bounds().size.height) / height).floor() as usize)
            .saturating_sub(1)
            .max(1);
        let inside = ui.selected.is_some_and(|ix| {
            let (item, _) = app.commit_row_geometry_for_e2e(ix);
            let top = handle.base_handle.bounds().top()
                + handle.base_handle.offset().y
                + row_height * item;
            top >= viewport.top() && top + row_height <= viewport.bottom()
        });
        (ui.selected, page, inside)
    })
    .unwrap()
}

pub fn scenario_commit_paging(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    for n in 0..EXTRA {
        git(
            fixture.path(),
            &["commit", "-q", "--allow-empty", "-m", &format!("extra {n}")],
        );
    }
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();
    let (_, page, _) = graph(cx, &app, window);
    assert!(
        page > 1 && page < LAST,
        "fixture needs multiple visible rows: {page}"
    );
    let assert_at = |cx: &mut VisualTestAppContext, ix| {
        let (selected, _, inside) = graph(cx, &app, window);
        assert_eq!(selected, Some(ix), "graph selection after keyboard paging");
        assert!(inside, "selected commit {ix} must be inside the viewport");
    };
    keys(cx, window, "home");
    assert_at(cx, 0);
    // Wheel scrolling does not change selection. Home must still reveal the
    // already-selected first row after it has left the viewport.
    let viewport = e2e::control_bounds(window.window_id(), "commit-list-viewport")
        .expect("commit viewport drawn");
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    let (selected, _, inside) = graph(cx, &app, window);
    assert_eq!(selected, Some(0), "wheel must not change commit selection");
    assert!(
        !inside,
        "wheel must move the selected first row out of view"
    );
    keys(cx, window, "home");
    assert_at(cx, 0);
    keys(cx, window, "pagedown");
    assert_at(cx, page);
    keys(cx, window, "pageup");
    assert_at(cx, 0);
    keys(cx, window, "end");
    assert_at(cx, LAST);
    keys(cx, window, "pageup");
    assert_at(cx, LAST - page);
    keys(cx, window, "pagedown");
    assert_at(cx, LAST);
    keys(cx, window, "cmd-up");
    assert_at(cx, 0);
    keys(cx, window, "cmd-down");
    assert_at(cx, LAST);
    // The context menu is separate from menu_overlay and must block navigation
    // even when the root still owns keyboard focus.
    keys(cx, window, "home");
    assert_at(cx, 0);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_commit_menu(0, viewport.center());
            cx.notify();
        });
        window.draw(cx).clear();
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
        assert!(
            root.is_focused(window),
            "root must own focus behind commit menu"
        );
        assert!(
            app.read(cx).commit_menu.is_some(),
            "commit menu must be open"
        );
    })
    .unwrap();
    keys(cx, window, "end");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "End behind commit menu must not move the hidden selection"
    );
    keys(cx, window, "down");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "Down behind commit menu must not move the hidden selection"
    );
    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
    // An overlay covers Graph even if focus is put back on the root.
    keys(cx, window, "home");
    assert_at(cx, 0);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        });
        window.draw(cx).clear();
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
        assert!(
            root.is_focused(window),
            "root must own focus behind Settings"
        );
    })
    .unwrap();
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "Settings must cover Graph"
    );
    keys(cx, window, "end");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "End behind Settings must not change the hidden commit selection"
    );
    keys(cx, window, "down");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "Down behind Settings must not change the hidden commit selection"
    );
    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    keys(cx, window, "down");
    assert_at(cx, 1);
    keys(cx, window, "up");
    assert_at(cx, 0);
    // Home and other center takeovers leave the covered commit untouched.
    keys(cx, window, "home");
    assert_at(cx, 0);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx))
    })
    .unwrap();
    keys(cx, window, "end");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "End on Home must not change the hidden commit selection"
    );
    app.update(cx, |app, cx| app.close_home_tab(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        app.update(cx, |app, cx| app.open_branch_cleanup_view(cx))
    })
    .unwrap();
    keys(cx, window, "end");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "End in Branch Cleanup must not change the hidden commit selection"
    );
    unmount(cx, app, window);

    // Conflict Mode replaces the entire body outside resolve_workspace. End
    // must not move the commit selected underneath that replacement.
    let fixture = content_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();
    keys(cx, window, "home");
    assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(0));
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        let app_ref = app.read(cx);
        assert!(
            app_ref.ui().conflict.is_some(),
            "conflict body must be shown"
        );
        assert!(!app_ref.ui().conflict_merge_pending);
        let root = app_ref.root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();
    keys(cx, window, "end");
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().selected),
        Some(0),
        "End in Conflict Mode must not change the hidden commit selection"
    );
    unmount(cx, app, window);
}
