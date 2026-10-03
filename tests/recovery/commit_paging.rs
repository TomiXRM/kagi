//! #980 — raw Home/End/Page keys page the graph only while its root owns focus.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};

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
}
