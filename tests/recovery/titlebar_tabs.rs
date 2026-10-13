//! #1462: document tabs always occupy the first titlebar row, above the toolbar.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_titlebar_tabs(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let first = build_fixture();
    let second = build_fixture();
    for slug in ["apple-light", "apple-dark"] {
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for display in [
                    settings::ToolbarDisplay::IconOnly,
                    settings::ToolbarDisplay::IconAndText,
                ] {
                    settings::set_toolbar_display(display);
                    for (width, height) in [(940., 660.), (1392., 883.)] {
                        let state = e2e::app_state(first.path()).unwrap();
                        let captured = Rc::new(RefCell::new(None));
                        let output = captured.clone();
                        let win: AnyWindowHandle = crate::macos::open_offscreen(
                            cx,
                            size(px(width), px(height)),
                            move |window, cx| e2e::mount_root(state, window, cx, &output),
                        )
                        .into();
                        let app = captured.borrow().clone().unwrap();
                        app.update(cx, |app, cx| {
                            app.set_theme(slug, cx);
                            cx.notify();
                        });
                        kagi::ui::tab_panel_a11y::clear_recorded_tab_panels();
                        settle(cx, win);
                        let toolbar = e2e::control_bounds(win.window_id(), "toolbar-row").unwrap();
                        assert!(toolbar.top() > px(0.), "toolbar must be below the always-visible titlebar tabs (#1462), got {toolbar:?}");
                        check_chrome(win, "toolbar-row", "toolbar-drag-region", zoom);
                        assert!(
                            kagi::ui::tab_panel_a11y::recorded_tab_panel("repo-tab-panel")
                                .is_some()
                        );
                        check_tab(win, "repo-tab-0");
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-t");
                        cx.run_until_parked();
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        settle(cx, win);
                        check_chrome(win, "home-frame", "home-drag-region", zoom);
                        check_tab(win, "home-tab");
                        assert!(
                            kagi::ui::tab_panel_a11y::recorded_tab_panel("home-tab-panel")
                                .is_some()
                        );
                        assert_eq!(
                            e2e::control_bounds(win.window_id(), "home-frame")
                                .unwrap()
                                .size
                                .height,
                            toolbar.size.height
                        );
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        cx.run_until_parked();
                        assert!(!app.read_with(cx, |app, _| app.home_in_front()));
                        app.update(cx, |app, cx| {
                            assert!(app.open_repository(second.path().to_path_buf(), cx));
                        });
                        settle(cx, win);
                        check_tab(win, "repo-tab-0");
                        check_tab(win, "repo-tab-1");
                        let add = e2e::control_bounds(win.window_id(), "tab-add").unwrap();
                        cx.simulate_click(win, add.center(), gpui::Modifiers::none());
                        cx.run_until_parked();
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        cx.run_until_parked();
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        settle(cx, win);
                        assert_eq!(app.read_with(cx, |app, _| app.tabs.len()), 1);
                        check_tab(win, "repo-tab-0");
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        settle(cx, win);
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        check_tab(win, "home-tab");
                        assert!(
                            e2e::control_bounds(win.window_id(), "home-tab-close").is_none(),
                            "permanent Home has no inert close control"
                        );
                        check_chrome(win, "home-frame", "home-drag-region", zoom);
                        unmount(cx, app, win);
                    }
                }
            }
        }
    }
}

fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    e2e::clear_control_bounds(win.window_id(), "home-tab-close");
    for id in [
        "repo-tab-0",
        "repo-tab-1",
        "home-tab",
        "tab-add",
        "toolbar-identity",
        "tb-refresh",
        "tb-pull",
        "tb-push",
        "tb-branch",
        "tb-stash",
        "tb-pop",
        "tb-undo",
        "tb-redo",
        "tb-terminal",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
        "tb-overflow",
    ] {
        e2e::clear_control_bounds(win.window_id(), id);
    }
    for _ in 0..8 {
        crate::toolbar_overflow::frame(cx, win);
    }
}

fn focus_root(cx: &mut VisualTestAppContext, win: AnyWindowHandle, app: &gpui::Entity<KagiApp>) {
    cx.update_window(win, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
}

fn check_tab(win: AnyWindowHandle, id: &str) {
    let strip = e2e::control_bounds(win.window_id(), "repo-tab-strip").unwrap();
    let tab = e2e::control_bounds(win.window_id(), id).unwrap();
    contained(strip, tab, id);
    assert!(
        tab.left() >= px(80.),
        "native traffic-light exclusion: {id}: {tab:?}"
    );
}

fn check_chrome(win: AnyWindowHandle, row_id: &str, drag_id: &str, zoom: f32) {
    let bounds = |id| e2e::control_bounds(win.window_id(), id).unwrap();
    let strip = bounds("repo-tab-strip");
    let row = bounds(row_id);
    assert_eq!(strip.top(), px(0.), "document strip is first");
    assert!(
        (f32::from(strip.size.height + px(1.)) - f32::from(theme::scaled_px(36.))).abs() <= 0.5,
        "36 reference px titlebar: {strip:?}"
    );
    assert_eq!(
        row.top(),
        strip.bottom() + px(1.),
        "toolbar directly follows titlebar's literal 1px separator"
    );
    if zoom == 1. {
        assert!(row.size.height <= px(54.), "toolbar heights unchanged");
    }
    let add = bounds("tab-add");
    contained(strip, add, "+ belongs to the document strip");
    let title_drag = bounds("titlebar-drag-region");
    let toolbar_drag = bounds(drag_id);
    contained(strip, title_drag, "blank titlebar drag target");
    contained(row, toolbar_drag, "blank toolbar drag target");
    assert!(title_drag.size.width > px(0.));
    assert!(toolbar_drag.size.width > px(0.));
    assert!(
        title_drag.left() >= add.right(),
        "+ precedes the blank titlebar spacer"
    );
    for id in [
        "repo-tab-0",
        "repo-tab-1",
        "home-tab",
        "tab-add",
        "toolbar-identity",
        "tb-refresh",
        "tb-pull",
        "tb-push",
        "tb-branch",
        "tb-stash",
        "tb-pop",
        "tb-undo",
        "tb-redo",
        "tb-terminal",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
        "tb-overflow",
    ] {
        if let Some(control) = e2e::control_bounds(win.window_id(), id) {
            if control.size.width <= px(0.) {
                continue;
            }
            for drag in [title_drag, toolbar_drag] {
                assert!(
                    control.right() <= drag.left()
                        || control.left() >= drag.right()
                        || control.bottom() <= drag.top()
                        || control.top() >= drag.bottom(),
                    "{id} intersects blank drag target: {control:?}, {drag:?}"
                );
            }
        }
    }
}
