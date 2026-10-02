//! The tab strip's `+` opens the Home tab (#923, ADR-0219 decision 1):
//! opening a repository from it turns Home into that repository's tab,
//! clicking a repository tab only moves it to the back, ⌘W closes it, repo
//! commands are off while it is in front, and with no tab open Home is the
//! whole window.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commands::{command_state, CommandState};
use kagi::ui::{home::HomeTab, tabs, KagiApp};

use crate::app_conflict::click_control;
use crate::macos::{build_fixture, mount, unmount};
use crate::recovery_operations::press_key;

fn active_path(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> std::path::PathBuf {
    cx.read(|cx| {
        let app = app.read(cx);
        std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap()
    })
}

fn home(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<HomeTab> {
    cx.read(|cx| app.read(cx).home)
}

fn tab_count(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> usize {
    cx.read(|cx| app.read(cx).tabs.len())
}

fn menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &'static str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.handle_menu_command(id, window, cx))
    })
    .unwrap();
    cx.run_until_parked();
}

/// Forget a control's last bounds so the next click proves it is drawn now.
fn forget(window: AnyWindowHandle, name: &str) {
    kagi::ui::e2e::clear_control_bounds(window.window_id(), name);
}

pub fn scenario_home_tab(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["recent_repos"]);
    // Home reads `gh repo list` when it opens; keep that off the network.
    let _gh = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\ncase \"$1 $2\" in 'repo list') echo '[]' ;; *) exit 1 ;; esac\n",
    );
    let first = build_fixture();
    let second = build_fixture();
    let first_path = first.path().canonicalize().unwrap();
    let second_path = second.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &first_path);
    // Most recent first: `second`, then `first` (already open).
    tabs::record_recent_repo(&first_path);
    tabs::record_recent_repo(&second_path);

    // `+` puts Home in front; the repository behind it is off screen, so its
    // commands are off and ⌘W would close Home, not the repository.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    assert_eq!(
        home(cx, &app),
        Some(HomeTab { front: true }),
        "`+` opens Home"
    );
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.home_in_front());
        assert!(matches!(
            command_state(app, "file.refresh"),
            CommandState::Disabled(_)
        ));
        assert!(matches!(
            command_state(app, "file.closeTab"),
            CommandState::Enabled
        ));
    });

    // A recent row turns Home into that repository's tab.
    click_control(cx, window, "home-recent-0");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), None, "Home became the repository's tab");
    assert_eq!(tab_count(cx, &app), 2);
    assert_eq!(active_path(cx, &app), second_path);

    // Clicking a repository tab — even the one right behind Home — only
    // moves Home to the back; its tab stays in the strip.
    menu(cx, &app, window, "file.newTab");
    assert_eq!(
        home(cx, &app),
        Some(HomeTab { front: true }),
        "New Tab opens Home"
    );
    click_control(cx, window, "repo-tab-1");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), Some(HomeTab { front: false }));
    assert_eq!(active_path(cx, &app), second_path);
    assert!(cx.read(|cx| !app.read(cx).home_in_front()));
    cx.read(|cx| {
        assert!(matches!(
            command_state(app.read(cx), "file.refresh"),
            CommandState::Enabled
        ));
    });
    click_control(cx, window, "repo-tab-0");
    cx.run_until_parked();
    assert_eq!(active_path(cx, &app), first_path);

    // Its own tab brings it back; ⌘W closes Home and nothing else.
    forget(window, "home-tab");
    click_control(cx, window, "home-tab");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), Some(HomeTab { front: true }));
    menu(cx, &app, window, "file.closeTab");
    assert_eq!(home(cx, &app), None, "⌘W closes Home");
    assert_eq!(tab_count(cx, &app), 2, "and no repository tab");
    assert_eq!(active_path(cx, &app), first_path);

    // An already open repository is switched to, not opened twice.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    forget(window, "home-recent-1");
    click_control(cx, window, "home-recent-1");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), None);
    assert_eq!(tab_count(cx, &app), 2, "no duplicate tab");
    assert_eq!(active_path(cx, &app), first_path);

    // Remote Browse opens over Home; Esc closes it and leaves Home in front.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    forget(window, "home-remote");
    click_control(cx, window, "home-remote");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).remote_browse().is_some()));
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).remote_browse().is_none()));
    assert_eq!(home(cx, &app), Some(HomeTab { front: true }));

    // With no repository tab, Home is the whole window and opens a tab.
    app.update(cx, |app, cx| {
        app.close_tab(1, cx);
        app.close_tab(0, cx);
    });
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).tabs.is_empty() && app.read(cx).home_in_front()));
    forget(window, "home-recent-0");
    click_control(cx, window, "home-recent-0");
    cx.run_until_parked();
    assert_eq!(tab_count(cx, &app), 1);
    assert_eq!(active_path(cx, &app), second_path);
    assert!(cx.read(|cx| !app.read(cx).home_in_front()));

    unmount(cx, app, window);
}
