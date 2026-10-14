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
    let compact = cx
        .update_window(window, |_, window, _| {
            window.viewport_size().width < px(1000.)
        })
        .unwrap();
    let graph_max = (viewport * 0.6 / scale).max(80.);
    let editor_budget = if kagi_ui_core::settings::toolbar_display(&theme::theme().slug)
        == kagi_ui_core::settings::ToolbarDisplay::IconOnly
    {
        52.
    } else {
        60.
    } + 36. / scale
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
    let tolerance = if compact { 1. / scale } else { 0.5 };
    if compact {
        assert!(
            presented >= 80. - tolerance && presented <= max + tolerance,
            "compact presentation retains the independent workspace budget"
        );
    } else {
        assert!(
            (presented - before.clamp(80., max)).abs() <= 0.5,
            "presentation must fit the independent workspace budget"
        );
    }
    let drag_height = |cx: &mut VisualTestAppContext| {
        if compact {
            f32::from(
                e2e::control_bounds(window.window_id(), "bottom-panel")
                    .unwrap()
                    .size
                    .height,
            ) / scale
                - 32.
        } else {
            cx.read(|cx| app.read(cx).bottom_panel_height)
        }
    };
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
        (drag_height(cx) - max).abs() <= tolerance,
        "upward overshoot must reach the independently calculated cap"
    );
    move_to(cx, window, start);
    assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), before);
    assert!(
        (drag_height(cx) - if compact { presented } else { before }).abs() <= tolerance,
        "returning to origin restores the initial presentation"
    );
    for step in [5., 20., 0., 40., 2000., -2000., 40.] {
        move_to(
            cx,
            window,
            point(start.x, start.y - px(theme::scaled(step))),
        );
        let actual = drag_height(cx);
        let expected = if step == 0. && !compact {
            before
        } else {
            (presented + step).clamp(80., max)
        };
        assert!(
            (actual - expected).abs() <= tolerance,
            "step {step}: body height {actual} must use the unchanged press origin"
        );
        assert_eq!(
            kagi_ui_core::settings::read_setting("bottom_panel_height"),
            setting_before,
            "live drag must not persist intermediate heights"
        );
        if step == 0. {
            assert!(
                (actual - if compact { presented } else { before }).abs() <= tolerance,
                "stationary drag frame must not jump"
            );
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
    let after = drag_height(cx);
    assert!(
        (after - (presented + 40.).clamp(80., max)).abs() <= tolerance,
        "drag +40 must use the presented origin and workspace cap: before={before}, after={after}"
    );
    cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    draw(cx, window);
    assert!((drag_height(cx) - after).abs() <= tolerance);
    if compact {
        assert_eq!(
            cx.read(|cx| app.read(cx).bottom_panel_height),
            before,
            "compact drag cannot replace the wide body preference"
        );
        assert_eq!(
            kagi_ui_core::settings::read_setting("bottom_panel_height"),
            setting_before,
            "compact release keeps its height presentation-only"
        );
    } else {
        assert_eq!(
            kagi_ui_core::settings::read_setting("bottom_panel_height"),
            Some(after.to_string()),
            "release persists the final height through the store"
        );
    }
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

/// #1484: the *entire* compact panel, including tabs/divider, is one third.
/// Its local drag height must never replace the saved wide body preference.
pub fn scenario_compact_bottom_third(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["bottom_panel_height", "ui_zoom", "reduce_motion"]);
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    for preset in ["apple-light", "apple-dark"] {
        for lang in [kagi_ui_core::i18n::Lang::En, kagi_ui_core::i18n::Lang::Ja] {
            for zoom in [1., 1.67] {
                let (app, window) = mount(cx, fixture.path());
                theme::set_zoom(zoom);
                theme::set_active(preset);
                kagi_ui_core::i18n::set_lang(lang);
                cx.update(theme::sync_gpui_component_theme);
                kagi_ui_core::settings::write_setting("bottom_panel_height", Some("240"));
                app.update(cx, |app, cx| {
                    app.bottom_panel_open = true;
                    app.bottom_panel_height = 240.;
                    cx.notify();
                });
                resize(cx, window, 1392., 883.);
                let wide = e2e::control_bounds(window.window_id(), "bottom-panel").unwrap();
                resize(cx, window, 900., 660.);
                let compact = e2e::control_bounds(window.window_id(), "bottom-panel").unwrap();
                assert!(
                    (compact.size.height - px(660. / 3.)).abs() <= px(1.),
                    "{preset}/{lang:?}/{zoom}: entire compact panel is 220px, not a 220px body plus tabs: {compact:?}"
                );
                assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), 240.);
                assert_eq!(
                    kagi_ui_core::settings::read_setting("bottom_panel_height").as_deref(),
                    Some("240"),
                    "presentation-only compact fitting never writes settings"
                );
                // A press and a round trip to the origin cannot jump to the
                // hidden wide preference. +40 uses the actual presented edge.
                let start = e2e::control_bounds(window.window_id(), "divider-bottom-panel")
                    .unwrap()
                    .center();
                cx.simulate_mouse_move(window, start, None, Modifiers::none());
                cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
                draw(cx, window);
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "bottom-panel")
                        .unwrap()
                        .size
                        .height,
                    compact.size.height,
                    "press alone preserves the compact presentation"
                );
                let end = point(start.x, start.y - px(theme::scaled(40.)));
                move_to(cx, window, end);
                let moved = e2e::control_bounds(window.window_id(), "bottom-panel").unwrap();
                assert!(
                    (moved.size.height - compact.size.height - px(theme::scaled(40.))).abs()
                        <= px(1.),
                    "compact divider grows from presented origin"
                );
                move_to(cx, window, start);
                assert!(
                    (e2e::control_bounds(window.window_id(), "bottom-panel")
                        .unwrap()
                        .size
                        .height
                        - compact.size.height)
                        .abs()
                        <= px(1.),
                    "returning to press origin restores one-third presentation"
                );
                move_to(cx, window, end);
                cx.simulate_mouse_up(window, end, MouseButton::Left, Modifiers::none());
                cx.run_until_parked();
                draw(cx, window);
                assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), 240.);
                assert_eq!(
                    kagi_ui_core::settings::read_setting("bottom_panel_height").as_deref(),
                    Some("240"),
                    "compact release retains the wide preference on disk"
                );
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "bottom-panel")
                        .unwrap()
                        .size
                        .height,
                    moved.size.height,
                    "released compact divider retains its local presentation"
                );
                resize(cx, window, 1392., 883.);
                assert_eq!(cx.read(|cx| app.read(cx).bottom_panel_height), 240.);
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "bottom-panel")
                        .unwrap()
                        .size
                        .height,
                    wide.size.height,
                    "widening restores the saved wide body height, not the compact drag height"
                );
                assert_eq!(
                    kagi_ui_core::settings::read_setting("bottom_panel_height").as_deref(),
                    Some("240")
                );
                unmount(cx, app, window);
            }
        }
    }
}

/// #1484/#1528: changed compact selection activates Changes; leaving compact
/// still restores the original desktop bottom presentation.
pub fn scenario_compact_bottom_presentation_restore(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    for (tab, open) in [
        (kagi::ui::BottomTab::Terminal, false),
        (kagi::ui::BottomTab::Activity, true),
    ] {
        let (app, window) = mount(cx, fixture.path());
        resize(cx, window, 1392., 883.);
        app.update(cx, |app, cx| {
            app.select(0);
            app.bottom_tab = tab;
            app.bottom_panel_open = open;
            cx.notify();
        });
        draw(cx, window);
        resize(cx, window, 900., 660.);
        assert_eq!(
            cx.read(|cx| (app.read(cx).bottom_tab, app.read(cx).bottom_panel_open)),
            (tab, open),
            "entering compact with a commit selected preserves the existing bottom surface"
        );
        app.update(cx, |app, cx| {
            app.select(1);
            cx.notify();
        });
        draw(cx, window);
        assert_eq!(
            cx.read(|cx| (app.read(cx).bottom_tab, app.read(cx).bottom_panel_open)),
            (kagi::ui::BottomTab::Changes, true),
            "changed compact selection opens Changes without replacing saved desktop state"
        );
        resize(cx, window, 1392., 883.);
        assert_eq!(
            cx.read(|cx| (app.read(cx).bottom_tab, app.read(cx).bottom_panel_open)),
            (tab, open),
            "wide restoration returns the original tab and open/closed presentation"
        );
        unmount(cx, app, window);
    }
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
