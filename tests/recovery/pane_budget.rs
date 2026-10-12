//! #1355: side-pane fitting protects the centre without replacing preferences.
use gpui::{
    point, px, AnyWindowHandle, Bounds, Modifiers, MouseButton, Pixels, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_layout::contained;

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle, width: f32, height: f32) {
    let wanted = gpui::size(px(width), px(height));
    cx.update_window(window, |_, window, _| window.resize(wanted))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
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
            std::time::Instant::now() < deadline,
            "native pane window resized"
        );
    }
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

fn bounds(window: AnyWindowHandle, name: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), name).unwrap_or_else(|| panic!("missing {name}"))
}

fn assert_diff(cx: &mut VisualTestAppContext, app: &gpui::Entity<KagiApp>, width: f32) {
    let diff = cx.read(|cx| app.read(cx).ui().main_diff.clone()).unwrap();
    let fit = cx.read(|cx| diff.read(cx).fit.clone());
    let row = fit.row_bounds().expect("diff header rendered");
    assert!(
        row.size.width + px(1.) >= theme::scaled_px(320.),
        "centre must retain scaled 320px: got {:?}, minimum {:?} at {width}px",
        row.size.width,
        theme::scaled_px(320.)
    );
    for name in [
        "main-diff-back",
        "main-diff-ext-editor",
        "main-diff-history",
        "diff-mode-unified",
        "diff-mode-split",
        "main-diff-stats",
    ] {
        contained(row, fit.control_bounds(name).unwrap(), name);
    }
}

fn drag(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str, dx: f32) {
    let start = bounds(window, name).center();
    cx.simulate_mouse_move(window, start, None, Modifiers::none());
    cx.simulate_mouse_down(window, start, MouseButton::Left, Modifiers::none());
    for step in [5., 20., 30.] {
        cx.simulate_mouse_move(
            window,
            start + point(px(step * dx.signum()), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
        settle(cx, window, 940., 660.);
    }
    cx.simulate_mouse_up(
        window,
        start + point(px(dx), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    settle(cx, window, 940., 660.);
}

pub fn scenario_pane_budget_fit(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let previous_motion = theme::reduce_motion();
    theme::set_reduce_motion(true);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.sidebar.width = 240.;
        app.sidebar.visible = true;
        app.panel_width = 360.;
        app.inspector_visible = true;
        app.bottom_panel_open = false;
        app.select(0);
        cx.notify();
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.open_main_diff_commit(0, cx));
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        theme::set_zoom(1.67);
        settle(cx, window, 940., 660.);
        assert_diff(cx, &app, 940.);
        let left = bounds(window, "sidebar-clip");
        let right = bounds(window, "right-pane-clip");
        assert!(left.size.width < theme::scaled_px(244.));
        assert!((f32::from(right.size.width) - theme::scaled(164.)).abs() <= 1.);
        let prefs = cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width));
        assert_eq!(prefs, (240., 360.));
        for (name, dx) in [("divider-panel", -30.), ("divider-sidebar", 30.)] {
            drag(cx, window, name, dx);
            assert_eq!(
                cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
                prefs,
                "{name}: capped expansion must not overwrite preferences"
            );
            assert_diff(cx, &app, 940.);
        }
        theme::set_zoom(1.);
        settle(cx, window, 1392., 883.);
        assert_diff(cx, &app, 1392.);
        // Existing 100% geometry, including both scaled 4px separators.
        assert_eq!(bounds(window, "sidebar-clip").right(), px(219.5));
        assert_eq!(bounds(window, "right-pane-clip").left(), px(1064.5));
        assert_eq!(
            cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
            prefs
        );
    }
    theme::set_zoom(1.67);
    settle(cx, window, 700., 500.);
    assert!(bounds(window, "sidebar-clip").size.width > px(0.));
    assert!(bounds(window, "right-pane-clip").size.width > px(0.));
    theme::set_reduce_motion(previous_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pane_budget_fit");
}
