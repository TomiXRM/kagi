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
