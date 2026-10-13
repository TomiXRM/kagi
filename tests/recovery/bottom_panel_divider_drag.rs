//! #1372: resizing starts from the current body height, not the tab strip.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{
    point, px, AnyWindowHandle, Entity, Modifiers, MouseButton, Pixels, Point, VisualTestAppContext,
};
use kagi::ui::{e2e, theme, KagiApp};
use std::time::{Duration, Instant};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw bottom panel");
}

fn resize(cx: &mut VisualTestAppContext, window: AnyWindowHandle, width: f32, height: f32) {
    let wanted = gpui::size(px(width), px(height));
    cx.update_window(window, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(window, |_, window, _| window.viewport_size())
            .unwrap()
            == wanted
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native bottom-panel window resized"
        );
    }
    draw(cx, window);
}

fn move_to(cx: &mut VisualTestAppContext, window: AnyWindowHandle, position: Point<Pixels>) {
    // GPUI consumes the activation move; repeat it for the first drag frame.
    for _ in 0..2 {
        cx.simulate_mouse_move(window, position, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        draw(cx, window);
    }
}

fn drag(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
    editor: bool,
) {
    let before = cx.read(|cx| app.read(cx).bottom_panel_height);
    let setting_before = kagi_ui_core::settings::read_setting("bottom_panel_height");
    let divider = e2e::control_bounds(window.window_id(), "divider-bottom-panel").unwrap();
    let start = divider.center();
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_panel_height),
        before,
        "press without moving must preserve the body height"
    );
    // Calculate the contractual cap independently of the rendered/dragged
    // value. Editor reserves compound content plus its shell; other modes
    // retain the 60%-of-viewport body limit.
    let scale = theme::scaled(1.);
    let viewport = f32::from(
        cx.update_window(window, |_, window, _| window.viewport_size().height)
            .unwrap(),
    );
    let graph_max = (viewport * 0.6 / scale).max(80.);
    let editor_budget = if kagi_ui_core::settings::toolbar_display(&theme::theme().slug)
        == kagi_ui_core::settings::ToolbarDisplay::IconOnly
    {
        52.
    } else {
        60.
    } + 36.
        + 22.
        + 200.
        + 32.;
    let max = if editor {
        graph_max.min((viewport / scale - editor_budget).max(80.))
    } else {
        graph_max
    };
    let presented = f32::from(
        e2e::control_bounds(window.window_id(), "bottom-panel")
            .unwrap()
            .size
            .height,
    ) / scale
        - 32.;
    assert!(
        (presented - before.clamp(80., max)).abs() <= 0.5,
        "presentation must fit the independent workspace budget"
    );
    if (max - presented).abs() <= 0.5 {
        assert!(editor, "only the compact Editor cell begins at its cap");
        assert!(
            (max - (viewport / scale - editor_budget).max(80.)).abs() <= 0.5,
            "the capped cell must follow from the compound Editor budget"
        );
    }
    move_to(
        cx,
        window,
        point(start.x, start.y - px(theme::scaled(2000.))),
    );
    assert!(
        (cx.read(|cx| app.read(cx).bottom_panel_height) - max).abs() <= 0.5,
        "upward overshoot must reach the independently calculated cap"
    );
    move_to(cx, window, start);
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), before);
    for step in [5., 20., 0., 40., 2000., -2000., 40.] {
        move_to(
            cx,
            window,
            point(start.x, start.y - px(theme::scaled(step))),
        );
        let actual = cx.read(|cx| app.read(cx).bottom_panel_height);
        let expected = if step == 0. {
            before
        } else {
            (presented + step).clamp(80., max)
        };
        assert!(
            (actual - expected).abs() <= 0.5,
            "step {step}: body height {actual} must use the unchanged press origin"
        );
        assert_eq!(
            kagi_ui_core::settings::read_setting("bottom_panel_height"),
            setting_before,
            "live drag must not persist intermediate heights"
        );
        if step == 0. {
            assert_eq!(actual, before, "stationary drag frame must not jump");
        }
        if (20. ..=40.).contains(&step) {
            let painted = e2e::control_bounds(window.window_id(), "divider-bottom-panel").unwrap();
            assert!(
                (f32::from(painted.center().y - start.y) + theme::scaled(expected - presented))
                    .abs()
                    <= 1.,
                "painted divider must continuously track the pointer"
            );
            if max > presented + 0.5 {
                assert!(
                    actual > presented + 0.5,
                    "room to grow must produce a real drag"
                );
                assert!(painted.center().y < start.y, "divider must visibly move");
            }
        }
    }
    let end = point(start.x, start.y - px(theme::scaled(40.)));
    let after = cx.read(|cx| app.read(cx).bottom_panel_height);
    assert!(
        (after - (presented + 40.).clamp(80., max)).abs() <= 0.5,
        "drag +40 must use the presented origin and workspace cap: before={before}, after={after}"
    );
    cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    draw(cx, window);
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), after);
    assert_eq!(
        kagi_ui_core::settings::read_setting("bottom_panel_height"),
        Some(after.to_string()),
        "release persists the final height through the store"
    );
}

fn preserve_preference(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    app: &Entity<KagiApp>,
) {
    theme::set_zoom(1.);
    app.update(cx, |app, cx| {
        app.bottom_panel_height = 400.;
        cx.notify();
    });
    kagi_ui_core::settings::write_setting("bottom_panel_height", Some("400"));
    resize(cx, window, 1392., 883.);
    let full = e2e::control_bounds(window.window_id(), "bottom-panel")
        .unwrap()
        .size
        .height;
    resize(cx, window, 1392., 500.);
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_panel_height),
        400.,
        "a smaller viewport must not replace the preferred body height"
    );
    let capped = e2e::control_bounds(window.window_id(), "bottom-panel")
        .unwrap()
        .size
        .height;
    assert!(
        capped < full,
        "presentation height fits the temporary smaller viewport"
    );
    let start = e2e::control_bounds(window.window_id(), "divider-bottom-panel")
        .unwrap()
        .center();
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), 400.);
    move_to(cx, window, point(start.x, start.y + px(theme::scaled(20.))));
    let effective = 500. * 0.6 / theme::scaled(1.);
    assert!(
        (cx.read(|cx| app.read(cx).bottom_panel_height) - effective + 20.).abs() <= 0.5,
        "a capped divider drag starts from its presented height"
    );
    move_to(cx, window, start);
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_panel_height),
        400.,
        "returning to the origin restores the capped preference"
    );
    cx.simulate_mouse_up(window, start, MouseButton::Left, Modifiers::none());
    draw(cx, window);
    resize(cx, window, 1392., 883.);
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), 400.);
    assert_eq!(
        e2e::control_bounds(window.window_id(), "bottom-panel")
            .unwrap()
            .size
            .height,
        full,
        "widening restores the full preferred body height"
    );
    assert_eq!(
        kagi_ui_core::settings::read_setting("bottom_panel_height").as_deref(),
        Some("400")
    );
    theme::set_zoom(1.67);
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_panel_height),
        400.,
        "zoom-induced compression must not replace the preference"
    );
    theme::set_zoom(1.);
    draw(cx, window);
    assert_eq!(
        e2e::control_bounds(window.window_id(), "bottom-panel")
            .unwrap()
            .size
            .height,
        full
    );
}

pub fn scenario_bottom_panel_divider_drag(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["bottom_panel_height", "ui_zoom", "reduce_motion"]);
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let reduce_before = theme::reduce_motion();
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    for (width, height) in [(1392., 883.), (940., 660.)] {
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            let (app, window) = mount(cx, &repo);
            resize(cx, window, width, height);
            if width == 1392. && zoom == 1. {
                preserve_preference(cx, window, &app);
            }
            for mode in 0..4 {
                app.update(cx, |app, cx| {
                    match mode {
                        1 => app.show_pr_mode(cx),
                        2 => app.show_issues_mode(cx),
                        3 => app.show_editor_mode(cx),
                        _ => (),
                    }
                    app.bottom_panel_open = true;
                    app.bottom_panel_height = 120.;
                    cx.notify();
                });
                cx.run_until_parked();
                draw(cx, window);
                drag(cx, window, &app, mode == 3);
            }
            unmount(cx, app, window);
        }
    }
    theme::set_reduce_motion(reduce_before);
    eprintln!("[gui-e2e] PASS bottom_panel_divider_drag");
}
