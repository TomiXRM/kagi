//! #1355: side-pane fitting protects the centre without replacing preferences.
use gpui::{
    point, px, AnyWindowHandle, Bounds, Modifiers, MouseButton, Pixels, VisualTestAppContext,
};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, theme};

use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_layout::contained;

fn settle(cx: &mut VisualTestAppContext, window: AnyWindowHandle, width: f32, height: f32) {
    for name in ["sidebar-clip", "right-pane-clip", "compact-rail"] {
        e2e::clear_control_bounds(window.window_id(), name);
    }
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
        settle(
            cx,
            window,
            (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
            660.,
        );
    }
    cx.simulate_mouse_up(
        window,
        start + point(px(dx), px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    settle(
        cx,
        window,
        (theme::rem_size_px() * theme::compact_threshold_rem() as f32).ceil(),
        660.,
    );
}

pub fn scenario_pane_budget_fit(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let previous_motion = theme::reduce_motion();
    theme::set_reduce_motion(true);
    // Preserve the original1000px/167% two-pane pressure with a supported,
    // explicitly isolated desktop threshold and ordinary user preferences.
    theme::set_compact_threshold_rem(40);
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
        settle(cx, window, 1000., 660.);
        assert_diff(cx, &app, 1000.);
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
            assert_diff(cx, &app, 1000.);
        }
        theme::set_zoom(1.);
        settle(cx, window, 1392., 883.);
        assert_diff(cx, &app, 1392.);
        assert_eq!(
            cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
            prefs
        );
        // Existing 100% geometry, including both scaled 4px separators.
        assert_eq!(bounds(window, "sidebar-clip").right(), px(219.5));
        assert_eq!(bounds(window, "right-pane-clip").left(), px(1064.5));
        assert_eq!(
            cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
            (240., 360.)
        );
    }
    theme::set_zoom(1.67);
    settle(cx, window, 700., 500.);
    contained(
        Bounds::new(point(px(0.), px(0.)), gpui::size(px(700.), px(500.))),
        bounds(window, "compact-rail"),
        "sub-breakpoint compact rail",
    );
    assert!(e2e::control_bounds(window.window_id(), "sidebar-clip").is_none());
    assert!(e2e::control_bounds(window.window_id(), "right-pane-clip").is_none());
    assert_eq!(
        cx.read(|cx| (app.read(cx).sidebar.width, app.read(cx).panel_width)),
        (240., 360.)
    );
    theme::set_reduce_motion(previous_motion);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pane_budget_fit");
}

fn check_sidebar_anchor(cx: &mut VisualTestAppContext, worktree: bool) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    crate::macos::git(fixture.path(), &["branch", "feat/a"]);
    crate::macos::git(fixture.path(), &["branch", "feat/b"]);
    let linked = fixture.path().join("linked");
    crate::macos::git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "linked",
            linked.to_str().unwrap(),
        ],
    );
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
    theme::set_zoom(1.67);
    theme::set_compact_threshold_rem(40);
    settle(cx, window, 1000., 660.);
    let pane = if worktree { 2 } else { 0 };
    let key = if worktree {
        app.read_with(cx, |app, _| {
            app.sidebar_row_keys_for_e2e(pane)
                .into_iter()
                .find(|key| key.starts_with("worktree:"))
                .expect("linked worktree row")
        })
    } else {
        "group:local:feat".to_owned()
    };
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_sidebar_row_for_e2e(pane, &key, window, cx)
        });
    })
    .unwrap();
    settle(cx, window, 1000., 660.);
    crate::keyboard_nav::keys(cx, window, if worktree { "enter" } else { "shift-f10" });
    settle(cx, window, 1000., 660.);
    let name = if worktree {
        "sidebar-worktree-keyboard-card"
    } else {
        "sidebar-group-local:feat-reveal"
    };
    let card = bounds(window, name);
    let sidebar_right = bounds(window, "divider-sidebar").left();
    assert!(
        (card.left() - sidebar_right).abs() <= px(1.),
        "{name}: card left {:?} must follow painted sidebar right {:?}, not preferred width",
        card.left(),
        sidebar_right
    );
    assert_eq!(app.read_with(cx, |app, _| app.sidebar.width), 240.);
    unmount(cx, app, window);
}

pub fn scenario_pane_budget_worktree_anchor(cx: &mut VisualTestAppContext) {
    check_sidebar_anchor(cx, true);
    eprintln!("[gui-e2e] PASS pane_budget_worktree_anchor");
}

pub fn scenario_pane_budget_group_anchor(cx: &mut VisualTestAppContext) {
    check_sidebar_anchor(cx, false);
    eprintln!("[gui-e2e] PASS pane_budget_group_anchor");
}
