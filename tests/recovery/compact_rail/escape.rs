//! #1479-S4: a rail context menu dismisses before its containing navigator.
use super::*;

pub fn scenario_compact_rail_nested_escape(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "-b",
            "nested-escape",
            path.to_str().unwrap(),
        ],
    );
    let path = path.canonicalize().unwrap();
    let key = format!("worktree:{}", path.display());
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        click(cx, win, TRIGGERS[2]);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.focus_sidebar_row_for_e2e(2, &key, window, cx)
            });
        })
        .unwrap();
        settle(cx, win);
        press(cx, win, "shift-f10");
        assert!(cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
        press(cx, win, "escape");
        assert!(!cx.read(|cx| e2e::menu_is_front(app.read(cx), cx)));
        one_open(cx, win, 2, case);
        press(cx, win, "escape");
        closed(cx, win);
        unmount(cx, app, win);
    });
}

pub fn scenario_compact_pr_menu_escape(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        seed_prs(cx, &app, 1);
        click(cx, win, TRIGGERS[5]);
        tab_to(cx, win, "pr-mode-section-1", false);
        press(cx, win, "enter");
        let opener = "#7 rail PR 7";
        crate::native_ax::focus_named(cx, win, "Button", opener);
        cx.simulate_mouse_down(
            win,
            bounds(win, "pr-mode-card-7").center(),
            gpui::MouseButton::Right,
            Modifiers::none(),
        );
        settle(cx, win);
        assert!(cx.read(|cx| app.read(cx).ui().pr_menu.is_some()));
        press(cx, win, "escape");
        assert!(
            cx.read(|cx| app.read(cx).ui().pr_menu.is_none()),
            "first Escape retires the PR context menu"
        );
        one_open(cx, win, 5, case);
        assert_eq!(
            crate::native_ax::focused(cx, win)["aria"]["label"],
            opener,
            "PR menu dismissal restores its opener"
        );
        press(cx, win, "escape");
        closed(cx, win);
        unmount(cx, app, win);
    });
}

pub fn scenario_compact_group_reveal_escape(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feat/a"]);
    git(fixture.path(), &["branch", "feat/b"]);
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        click(cx, win, TRIGGERS[0]);
        cx.update_window(win, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.focus_sidebar_row_for_e2e(0, "group:local:feat", window, cx)
            });
        })
        .unwrap();
        settle(cx, win);
        let reveal = "sidebar-group-local:feat-reveal";
        for key in ["shift-f10", "menu"] {
            press(cx, win, key);
            bounds(win, reveal);
            e2e::clear_control_bounds(win.window_id(), reveal);
            press(cx, win, "escape");
            one_open(cx, win, 0, case);
            assert!(e2e::control_bounds(win.window_id(), reveal).is_none());
            assert_eq!(
                cx.update_window(win, |_, window, cx| app
                    .read(cx)
                    .sidebar_row_focused_for_e2e(window))
                    .unwrap(),
                Some((0, "group:local:feat".into())),
                "reveal dismissal retains its group opener"
            );
        }
        press(cx, win, "escape");
        closed(cx, win);
        unmount(cx, app, win);
    });
}
