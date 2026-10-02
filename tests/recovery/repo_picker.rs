//! The tab strip's `+` opens the repository picker (#923, ADR-0219): recent
//! repositories open (or switch to) a tab, Esc closes it, Remote Browse takes
//! over the slot, and the same picker works from the Welcome screen.
use gpui::VisualTestAppContext;
use kagi::ui::{tabs, KagiApp};

use crate::app_conflict::click_control;
use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::press_key;

fn active_path(cx: &mut VisualTestAppContext, app: &gpui::Entity<KagiApp>) -> std::path::PathBuf {
    cx.read(|cx| {
        let app = app.read(cx);
        std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap()
    })
}

fn picker_open(cx: &mut VisualTestAppContext, app: &gpui::Entity<KagiApp>) -> bool {
    cx.read(|cx| app.read(cx).repo_picker().is_some())
}

pub fn scenario_repo_picker(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    let first = build_fixture();
    let second = build_fixture();
    let first_path = first.path().canonicalize().unwrap();
    let second_path = second.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &first_path);
    // Most recent first: `second`, then `first` (already open).
    tabs::record_recent_repo(&first_path);
    tabs::record_recent_repo(&second_path);

    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    cx.read(|cx| {
        let picker = app.read(cx).repo_picker().expect("`+` opens the picker");
        assert_eq!(
            picker.recent.first().map(|p| p.canonicalize().unwrap()),
            Some(second_path.clone()),
            "the most recently opened repository is listed first"
        );
    });
    click_control(cx, window, "repo-picker-recent-0");
    cx.run_until_parked();
    assert!(!picker_open(cx, &app), "opening a row closes the picker");
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), 2);
    assert_eq!(
        active_path(cx, &app),
        second_path,
        "the row opens a new tab"
    );

    // An already open repository is switched to, not opened twice.
    click_control(cx, window, "tab-add");
    click_control(cx, window, "repo-picker-recent-1");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), 2, "no duplicate tab");
    assert_eq!(active_path(cx, &app), first_path);

    // Esc closes it and changes nothing.
    click_control(cx, window, "tab-add");
    assert!(picker_open(cx, &app));
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(!picker_open(cx, &app), "Esc closes the picker");
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), 2);
    assert_eq!(active_path(cx, &app), first_path);

    // Remote Browse takes the single modal slot.
    click_control(cx, window, "tab-add");
    click_control(cx, window, "repo-picker-remote");
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.repo_picker().is_none());
        assert!(
            app.remote_browse().is_some(),
            "the button hands over to Remote Browse"
        );
    });
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();

    // New Tab (⌘T) on the Welcome screen (no tabs) opens the same picker, and
    // the Welcome compositor renders and routes it.
    app.update(cx, |app, cx| {
        app.close_tab(1, cx);
        app.close_tab(0, cx);
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).tabs.is_empty()));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("file.newTab", window, cx)
        })
    })
    .unwrap();
    assert!(picker_open(cx, &app), "New Tab opens the picker");
    kagi::ui::e2e::clear_control_bounds(window.window_id(), "repo-picker-recent-0");
    click_control(cx, window, "repo-picker-recent-0");
    cx.run_until_parked();
    assert!(!picker_open(cx, &app));
    assert_eq!(cx.read(|cx| app.read(cx).tabs.len()), 1);
    assert_eq!(active_path(cx, &app), second_path);

    unmount(cx, app, window);
}
