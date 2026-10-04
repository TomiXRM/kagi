//! #1011: Graph header dividers track the pointer even without a sidebar.

use std::time::{Duration, Instant};

use gpui::{
    point, px, AnyWindowHandle, Bounds, Modifiers, MouseButton, Pixels, VisualTestAppContext,
};
use kagi::ui::commands::ToggleSidebar;
use kagi::ui::{e2e, theme, KagiApp};

use crate::macos::{build_fixture, mount, unmount};

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle, at: Instant) {
    e2e::set_panel_motion_clock(Some(at));
    for name in ["sidebar-clip", "divider-badge-col", "divider-graph-col"] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw the Graph header");
}

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name).expect("drawn Graph column divider")
}

fn drag_40(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &gpui::Entity<KagiApp>,
    name: &str,
    at: Instant,
    sliding: bool,
) {
    let before = bounds(window, name);
    let start = point(before.center().x, before.center().y);
    let old_width = cx.read(|cx| {
        let app = app.read(cx);
        if name == "divider-badge-col" {
            app.badge_col_w
        } else {
            app.graph_col_w
        }
    });
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    let mut cursor = start;
    let mut last_at = at;
    for (index, step) in [5., 20., 40.].into_iter().enumerate() {
        if sliding {
            last_at = at + Duration::from_millis(15 * (index as u64 + 1));
            frame(cx, window, last_at);
        }
        cursor = point(start.x + px(step), start.y);
        cx.simulate_mouse_move(window, cursor, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        frame(cx, window, last_at);
        // GPUI activates a drag only after crossing its pointer threshold.
        if sliding && step >= 20. {
            let offset = f32::from(bounds(window, name).center().x - cursor.x);
            assert!(
                offset.abs() <= 1.,
                "{name}: the sliding divider trails the pointer by {offset}px"
            );
        }
    }
    cx.simulate_mouse_up(window, cursor, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    frame(cx, window, last_at);
    let new_width = cx.read(|cx| {
        let app = app.read(cx);
        if name == "divider-badge-col" {
            app.badge_col_w
        } else {
            app.graph_col_w
        }
    });
    if sliding {
        assert!(
            new_width > old_width,
            "{name}: a closing sidebar must not suppress the column drag"
        );
    } else {
        let moved = f32::from(bounds(window, name).center().x - before.center().x);
        let logical_delta = 40.0 / theme::scaled(1.0);
        assert!(
            (new_width - old_width - logical_delta).abs() <= 1.,
            "{name}: stored width changed by {} instead of {logical_delta} logical px for a 40px drag",
            new_width - old_width
        );
        assert!(
            (moved - 40.).abs() <= 1.,
            "{name}: painted divider moved {moved}px instead of following the pointer by 40px"
        );
    }
}

pub fn scenario_graph_column_divider_hidden(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["badge_col_w", "graph_col_w", "reduce_motion"]);
    let previous_reduce_motion = theme::reduce_motion();
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let t0 = Instant::now();
    frame(cx, window, t0);

    cx.dispatch_action(window, ToggleSidebar);
    let hidden = t0 + Duration::from_millis(300);
    frame(cx, window, hidden);
    assert!(
        e2e::control_bounds(window.window_id(), "sidebar-clip").is_none(),
        "the sidebar is fully hidden before the drag"
    );
    for name in ["divider-badge-col", "divider-graph-col"] {
        drag_40(cx, window, &app, name, hidden, false);
    }
    let hidden_widths = cx.read(|cx| (app.read(cx).badge_col_w, app.read(cx).graph_col_w));

    cx.dispatch_action(window, ToggleSidebar);
    let shown = hidden + Duration::from_millis(400);
    frame(cx, window, shown);
    for name in ["divider-badge-col", "divider-graph-col"] {
        drag_40(cx, window, &app, name, shown, false);
    }
    let shown_widths = cx.read(|cx| (app.read(cx).badge_col_w, app.read(cx).graph_col_w));
    let logical_delta = 40.0 / theme::scaled(1.0);
    assert!((shown_widths.0 - hidden_widths.0 - logical_delta).abs() <= 1.);
    assert!((shown_widths.1 - hidden_widths.1 - logical_delta).abs() <= 1.);
    cx.dispatch_action(window, ToggleSidebar);
    let sliding = shown + Duration::from_millis(40);
    frame(cx, window, sliding);
    let sidebar = bounds(window, "sidebar-clip");
    assert!(
        0. < f32::from(sidebar.size.width) && f32::from(sidebar.size.width) < 300.,
        "the sidebar is partially visible while the column drags"
    );
    drag_40(cx, window, &app, "divider-badge-col", sliding, true);
    drag_40(
        cx,
        window,
        &app,
        "divider-graph-col",
        sliding + Duration::from_millis(45),
        true,
    );

    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(previous_reduce_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS graph_column_divider_hidden");
}
