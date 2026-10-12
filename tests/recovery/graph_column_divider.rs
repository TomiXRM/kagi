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
        assert!(Instant::now() < deadline, "native Graph window resized");
    }
    kagi::ui::graph_view::take_paint_trace(window.window_id());
    frame(cx, window, Instant::now());
}

fn assert_columns(
    window: AnyWindowHandle,
    lang: kagi::ui::i18n::Lang,
    width: f32,
    height: f32,
    zoom: f32,
) {
    let row = bounds(window, "graph-commit-row");
    let subject = bounds(window, "graph-commit-subject");
    let author = bounds(window, "graph-commit-author");
    let time = bounds(window, "graph-commit-time");
    if zoom == 1. {
        for (name, actual, expected) in [
            (
                "branch divider",
                bounds(window, "divider-badge-col").left(),
                365.5,
            ),
            (
                "graph divider",
                bounds(window, "divider-graph-col").left(),
                527.5,
            ),
            ("subject", subject.left(), 531.),
            ("author", author.left(), 1251.5),
            ("time", time.left(), 1338.),
        ] {
            assert_eq!(actual, px(expected), "{lang:?}: unchanged 100% {name} x");
        }
    }
    eprintln!("[gui-e2e] graph_columns_fit {lang:?} {width}x{height} zoom={zoom}: row={row:?} subject={subject:?} author={author:?} time={time:?}");
    assert!(
        subject.size.width > px(0.),
        "{lang:?} {width}x{height} zoom={zoom}: subject must have positive width, got {subject:?}"
    );
    for (name, cell) in [("subject", subject), ("author", author), ("time", time)] {
        assert!(
            cell.left() >= row.left() && cell.right() <= row.right(),
            "{lang:?} {width}x{height} zoom={zoom}: {name} must fit row {row:?}, got {cell:?}"
        );
    }
    assert!(subject.size.width >= theme::scaled_px(120.) - px(1.));
    assert!(author.size.width >= theme::scaled_px(40.) - px(1.));
    assert!((time.size.width - theme::scaled_px(48.)).abs() <= px(1.));
    assert!(
        e2e::control_paint(window.window_id(), "graph-commit-author")
            .unwrap()
            .middle_ellipsis,
        "the actual author paint uses native middle ellipsis"
    );
    let graph_left = bounds(window, "divider-badge-col").right();
    let graph_right = bounds(window, "divider-graph-col").left();
    let (nodes, _) = kagi::ui::graph_view::take_paint_trace(window.window_id());
    assert!(
        nodes
            .iter()
            .any(|node| node.center.x >= graph_left && node.center.x <= graph_right),
        "the fitted graph must retain a genuinely painted commit lane"
    );
}

pub fn scenario_graph_columns_fit(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["badge_col_w", "graph_col_w", "reduce_motion"]);
    let previous_reduce_motion = theme::reduce_motion();
    let saved_widths = ["badge_col_w", "graph_col_w"].map(kagi_ui_core::settings::read_setting);
    let fixture = build_fixture();
    crate::macos::git(
        fixture.path(),
        &[
            "commit",
            "-q",
            "--allow-empty",
            "--author=Long Author 名前 With Identifying Suffix <graph@example.com>",
            "-m",
            "Readable subject 件名 with a deliberately long ending",
        ],
    );
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.badge_col_w = 150.;
        app.graph_col_w = 176.;
        app.sidebar.width = 240.;
        app.sidebar.visible = true;
        app.bottom_panel_open = false;
        cx.notify();
    });
    theme::set_reduce_motion(true);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        for (width, height, zoom) in [(1392., 883., 1.), (940., 660., 1.67)] {
            theme::set_zoom(zoom);
            resize(cx, window, width, height);
            assert_columns(window, lang, width, height, zoom);
            assert_eq!(
                ["badge_col_w", "graph_col_w"].map(kagi_ui_core::settings::read_setting),
                saved_widths,
                "viewport fitting must not rewrite divider settings"
            );
            assert_eq!(
                cx.read(|cx| (app.read(cx).badge_col_w, app.read(cx).graph_col_w)),
                (150., 176.)
            );
        }
    }
    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(previous_reduce_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS graph_columns_fit");
}

fn capped_drag(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<KagiApp>,
    window: AnyWindowHandle,
    name: &str,
) {
    let before = bounds(window, name);
    let prefs = cx.read(|cx| (app.read(cx).badge_col_w, app.read(cx).graph_col_w));
    let saved = ["badge_col_w", "graph_col_w"].map(kagi_ui_core::settings::read_setting);
    let start = before.center();
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    for dx in [5., 20., 30.] {
        cx.simulate_mouse_move(
            window,
            start + point(px(dx), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
        frame(cx, window, Instant::now());
    }
    cx.simulate_mouse_up(
        window,
        start + point(px(30.), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(
        cx.read(|cx| (app.read(cx).badge_col_w, app.read(cx).graph_col_w)),
        prefs,
        "{name}: a capped expansion must preserve preferred widths"
    );
    assert_eq!(
        ["badge_col_w", "graph_col_w"].map(kagi_ui_core::settings::read_setting),
        saved
    );
    assert_eq!(bounds(window, name).left(), before.left());
}

pub fn scenario_graph_columns_compressed_drag(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["badge_col_w", "graph_col_w", "reduce_motion"]);
    let previous_motion = theme::reduce_motion();
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.badge_col_w = 150.;
        app.graph_col_w = 176.;
        app.sidebar.width = 240.;
        cx.notify();
    });
    theme::set_zoom(1.67);
    resize(cx, window, 940., 660.);
    for name in ["divider-graph-col", "divider-badge-col"] {
        capped_drag(cx, &app, window, name);
    }
    theme::set_zoom(1.);
    resize(cx, window, 1392., 883.);
    assert_eq!(bounds(window, "divider-badge-col").left(), px(365.5));
    assert_eq!(bounds(window, "divider-graph-col").left(), px(527.5));
    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(previous_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS graph_columns_compressed_drag");
}

pub fn scenario_graph_columns_inspector_fit(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let previous_motion = theme::reduce_motion();
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.badge_col_w = 150.;
        app.graph_col_w = 176.;
        app.sidebar.width = 240.;
        app.panel_width = 360.;
        app.inspector_visible = true;
        app.select(0);
        cx.notify();
    });
    theme::set_zoom(1.67);
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        kagi::ui::i18n::set_lang(lang);
        resize(cx, window, 1440., 883.);
        let row = bounds(window, "graph-commit-row");
        for name in ["graph-commit-author", "graph-commit-time"] {
            let cell = bounds(window, name);
            assert!(
                cell.right() <= row.right(),
                "{lang:?} Inspector: {name} must fit row {row:?}, got {cell:?}"
            );
        }
        let subject = bounds(window, "graph-commit-subject");
        assert!(subject.size.width > px(0.));
        assert!(
            subject.size.width < theme::scaled_px(120.),
            "only sub-minimum center panes relax the subject floor"
        );
    }
    e2e::set_panel_motion_clock(None);
    theme::set_reduce_motion(previous_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS graph_columns_inspector_fit");
}
