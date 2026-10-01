//! #864: actual Graph sidebar pointer/geometry and settings persistence.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui::{
    point, px, size, AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext,
};
use kagi::ui::{e2e, list_a11y, settings, theme, KagiApp};

use crate::macos::{build_fixture, mount, open_offscreen, unmount};
use crate::recovery_operations::paint;

fn mount_at(
    cx: &mut VisualTestAppContext,
    repo: &Path,
    height: f32,
) -> (Entity<KagiApp>, AnyWindowHandle) {
    if height == 900. {
        return mount(cx, repo);
    }
    crate::gui_evidence::fixture(repo);
    let state = e2e::app_state(repo).expect("fixture app state");
    let captured: Rc<RefCell<Option<Entity<KagiApp>>>> = Rc::default();
    let output = captured.clone();
    let window = open_offscreen(cx, size(px(1440.), px(height)), move |window, cx| {
        e2e::mount_root(state, window, cx, &output)
    });
    let app = captured.borrow().clone().expect("mounted KagiApp");
    cx.run_until_parked();
    (app, window.into())
}

fn bounds(cx: &VisualTestAppContext, app: &Entity<KagiApp>) -> [(f32, f32); 6] {
    cx.read(|cx| {
        let sidebar = &app.read(cx).sidebar;
        std::array::from_fn(|index| sidebar.pane_geom[index].get())
    })
}

fn redraw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    list_a11y::clear_recorded_lists();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn disk_layout() -> String {
    let path = settings::settings_path().expect("settings file path");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    json["sidebar_panes"]
        .as_str()
        .expect("flat string value")
        .to_string()
}

pub fn scenario_sidebar_panes(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let previous_setting = settings::read_setting("sidebar_panes");
    let previous_zoom = theme::zoom();
    theme::set_zoom(1.);
    settings::write_setting("sidebar_panes", Some("invalid:layout"));
    settings::flush();
    // Both lists contain enough rows to scroll separately even after the
    // bottom panel and all six pinned headers have taken their space.
    let git = git2::Repository::open(repo).unwrap();
    let head = git.head().unwrap().peel_to_commit().unwrap();
    for index in 0..48 {
        git.branch(&format!("scroll/{index:02}"), &head, false)
            .unwrap();
        git.reference(
            &format!("refs/remotes/origin/scroll/{index:02}"),
            head.id(),
            false,
            "fixture",
        )
        .unwrap();
    }
    drop(head);
    drop(git);

    let (app, window) = mount_at(cx, repo, 900.);
    redraw(cx, &app, window);
    assert_eq!(
        disk_layout(),
        "invalid:layout",
        "mount/draw must not repair settings"
    );
    let initial = bounds(cx, &app);
    for (index, &(top, bottom)) in initial.iter().enumerate() {
        assert!(bottom - top >= 24., "pane {index} header fits: {initial:?}");
        if index > 0 {
            assert!(top >= initial[index - 1].1, "pane order: {initial:?}");
        }
    }
    let prs = list_a11y::recorded_list("sidebar-prs").expect("empty PR pane Tree");
    assert!(
        prs.rows[&0].0.contains('0'),
        "empty PR pane header: {prs:?}"
    );

    // A malformed value remains verbatim after startup, but the first actual
    // header click commits the default weights plus the new collapse mask.
    let prs_header = e2e::control_bounds(window.window_id(), "prs").expect("PR header hitbox");
    cx.simulate_click(window, prs_header.center(), Modifiers::none());
    redraw(cx, &app, window);
    settings::flush();
    let repaired = settings::SidebarPaneLayout::parse(&disk_layout()).unwrap();
    assert_eq!(
        repaired.weights,
        settings::SidebarPaneLayout::default().weights
    );
    assert_eq!(repaired.collapsed_mask, 1);
    let prs_header = e2e::control_bounds(window.window_id(), "prs").unwrap();
    cx.simulate_click(window, prs_header.center(), Modifiers::none());
    redraw(cx, &app, window);
    assert_eq!(
        settings::SidebarPaneLayout::parse(&settings::read_setting("sidebar_panes").unwrap())
            .unwrap()
            .collapsed_mask,
        0
    );

    // Deliver real pointer down/move/up on the measured LOCAL/REMOTE divider.
    let divider = e2e::control_bounds(window.window_id(), "sidebar-local-divider")
        .expect("local pane divider hitbox");
    let center = divider.center();
    cx.simulate_mouse_move(window, center, None, Modifiers::none());
    cx.simulate_mouse_down(window, center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        window,
        point(center.x, center.y + px(8.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    paint(cx, window);
    let moved = point(center.x, center.y + px(32.));
    cx.simulate_mouse_move(window, moved, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_up(window, moved, MouseButton::Left, Modifiers::none());
    redraw(cx, &app, window);
    let after_drag = bounds(cx, &app);
    let weights = cx.read(|cx| app.read(cx).sidebar.pane_weights);
    assert!(
        after_drag[1].1 - after_drag[1].0 > initial[1].1 - initial[1].0 + 8.,
        "local grows after drag: {initial:?} -> {after_drag:?}"
    );
    assert!(
        after_drag[2].1 - after_drag[2].0 < initial[2].1 - initial[2].0 - 8.,
        "remote shrinks after drag"
    );
    assert_ne!(
        weights[1],
        settings::SidebarPaneLayout::default().weights[1]
    );
    settings::flush();
    assert_eq!(
        settings::SidebarPaneLayout::parse(&disk_layout())
            .unwrap()
            .weights,
        weights
    );

    // Scroll only LOCAL: REMOTE's drawn visible rows and position stay put.
    let remote_before = list_a11y::recorded_list("sidebar-remote").unwrap().rows;
    app.update(cx, |app, cx| {
        app.sidebar.scroll_handles[1].scroll_to_item(42, gpui::ScrollStrategy::Center);
        cx.notify();
    });
    redraw(cx, &app, window);
    let local = list_a11y::recorded_list("sidebar-local").unwrap();
    assert!(
        local.rows.keys().any(|&position| position > 30),
        "local scroll reached distant refs: {local:?}"
    );
    assert_eq!(
        list_a11y::recorded_list("sidebar-remote").unwrap().rows,
        remote_before
    );

    // A real header click persists the derived collapsed mask; expanding
    // restores the unchanged pair weights and the pane body height.
    let remote_header =
        e2e::control_bounds(window.window_id(), "remote").expect("remote header hitbox");
    cx.simulate_click(window, remote_header.center(), Modifiers::none());
    redraw(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).sidebar.collapsed.contains("remote")));
    assert_eq!(
        settings::SidebarPaneLayout::parse(&settings::read_setting("sidebar_panes").unwrap())
            .unwrap()
            .collapsed_mask
            & 4,
        4
    );
    assert_eq!(cx.read(|cx| app.read(cx).sidebar.pane_weights), weights);
    assert!(bounds(cx, &app)[1].1 - bounds(cx, &app)[1].0 > after_drag[1].1 - after_drag[1].0);
    let remote_header = e2e::control_bounds(window.window_id(), "remote").unwrap();
    cx.simulate_click(window, remote_header.center(), Modifiers::none());
    redraw(cx, &app, window);
    assert_eq!(cx.read(|cx| app.read(cx).sidebar.pane_weights), weights);
    let expanded = bounds(cx, &app);
    assert!((expanded[1].1 - expanded[1].0 - (after_drag[1].1 - after_drag[1].0)).abs() < 3.);

    // Persist a collapsed section, then mount an actual new app at 600px.
    let remote_header = e2e::control_bounds(window.window_id(), "remote").unwrap();
    cx.simulate_click(window, remote_header.center(), Modifiers::none());
    redraw(cx, &app, window);
    settings::flush();
    unmount(cx, app, window);
    let (reopened, compact_window) = mount_at(cx, repo, 600.);
    redraw(cx, &reopened, compact_window);
    assert_eq!(
        cx.read(|cx| reopened.read(cx).sidebar.pane_weights),
        weights
    );
    assert!(cx.read(|cx| reopened.read(cx).sidebar.collapsed.contains("remote")));
    let compact = bounds(cx, &reopened);
    let sidebar = e2e::control_bounds(compact_window.window_id(), "worktree-sidebar").unwrap();
    let sidebar_bottom = f32::from(sidebar.origin.y + sidebar.size.height);
    assert!(
        compact[5].1 <= sidebar_bottom + 1.,
        "six panes fit 600px: {compact:?}, sidebar={sidebar:?}"
    );
    theme::set_zoom(1.25);
    redraw(cx, &reopened, compact_window);
    let scaled = bounds(cx, &reopened);
    assert!(
        scaled.iter().all(|&(top, bottom)| bottom - top >= 30.),
        "all six scaled headers retain usable height: {scaled:?}"
    );
    let sidebar = e2e::control_bounds(compact_window.window_id(), "worktree-sidebar").unwrap();
    let pane_viewport = e2e::control_bounds(compact_window.window_id(), "sidebar-panes").unwrap();
    let viewport_bottom = f32::from(pane_viewport.origin.y + pane_viewport.size.height);
    assert!(viewport_bottom <= f32::from(sidebar.origin.y + sidebar.size.height) + 1.);
    if scaled[5].1 > viewport_bottom {
        // At 600px/125%, six scaled headers exceed the available Graph pane.
        // Scroll the real stack on the empty PR header so no leaf list eats
        // the wheel event; the last section must become reachable.
        cx.simulate_event(
            compact_window,
            gpui::ScrollWheelEvent {
                position: point(
                    pane_viewport.origin.x + px(8.),
                    pane_viewport.origin.y + px(12.),
                ),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-1000.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        redraw(cx, &reopened, compact_window);
        let scrolled = bounds(cx, &reopened);
        assert!(
            scrolled[5].1 <= viewport_bottom + 1. && scrolled[5].0 >= f32::from(pane_viewport.origin.y),
            "last header reachable by stack scroll: {scaled:?} -> {scrolled:?}, viewport={pane_viewport:?}"
        );
    }
    unmount(cx, reopened, compact_window);

    settings::write_setting("sidebar_panes", previous_setting.as_deref());
    settings::flush();
    theme::set_zoom(previous_zoom);
    eprintln!("[gui-e2e] PASS sidebar_panes: six fixed headers, measured pointer drag, independent scroll, collapse/expand ratios, malformed raw preservation, remount at 600px and 125% zoom");
}
