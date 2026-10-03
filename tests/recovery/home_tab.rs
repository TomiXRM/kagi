//! The tab strip's `+` opens the Home tab (#923, ADR-0219 decision 1):
//! opening a repository from it turns Home into that repository's tab,
//! clicking a repository tab only moves it to the back, ⌘W closes it, repo
//! commands are off while it is in front, and with no tab open Home is the
//! whole window.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::commands::{command_state, CommandState};
use kagi::ui::{tabs, KagiApp};

use crate::app_conflict::click_control;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::press_key;

fn active_path(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> std::path::PathBuf {
    cx.read(|cx| {
        let app = app.read(cx);
        std::fs::canonicalize(&app.tabs[app.active_tab].path).unwrap()
    })
}

/// Whether Home is open, and if so whether it is in front.
fn home(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<bool> {
    cx.read(|cx| app.read(cx).home.map(|home| home.front))
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

/// Draw, then say whether `name` was laid out in this frame.
fn drawn(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) -> bool {
    forget(window, name);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    kagi::ui::e2e::control_bounds(window.window_id(), name).is_some()
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

    // Home ends the visit of the tab it covers, like a tab switch: a plan
    // still being built for that repository is dropped instead of landing
    // in the modal slot behind Home, where Enter could confirm it unseen
    // (#927 review).
    git(&first_path, &["branch", "feature", "HEAD~1"]);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.open_merge_modal("feature".into(), None, cx);
            app.open_home_tab(window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.home_in_front());
        assert!(app.merge_modal().is_none(), "the plan was dropped");
        assert!(!kagi::ui::e2e::active_modal_present(app));
    });
    // Leaving Home enters that tab again — its visit ended, so this is not
    // the no-op of re-selecting the tab on screen (#488).
    app.update(cx, |app, cx| {
        app.close_home_tab(cx);
        assert!(
            app.ui().panes_revalidating(),
            "the tab Home covered is re-entered"
        );
    });
    cx.run_until_parked();

    // `+` puts Home in front; the repository behind it is off screen, so its
    // commands are off and ⌘W would close Home, not the repository.
    click_control(cx, window, "tab-add");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), Some(true), "`+` opens Home");
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
    assert_eq!(home(cx, &app), Some(true), "New Tab opens Home");
    click_control(cx, window, "repo-tab-1");
    cx.run_until_parked();
    assert_eq!(home(cx, &app), Some(false));
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
    assert_eq!(home(cx, &app), Some(true));
    // Window-global overlays are drawn over Home: an AppNotice (Enter and
    // Esc reach it, so it must be readable first) and the menu overlays
    // such as Settings (#927 review).
    app.update(cx, |app, _| {
        kagi::ui::e2e::deliver_app_notice(app, "home notice")
    });
    assert!(
        drawn(cx, window, "active-modal/app-notice"),
        "an AppNotice is drawn over Home"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    menu(cx, &app, window, "app.settings");
    assert!(
        drawn(cx, window, "settings-theme-select"),
        "Settings is drawn over Home"
    );
    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
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
    // ... and is drawn over Home, centred in the window, not after Home's
    // content at the bottom edge.
    forget(window, "remote-browse-card");
    let (card, viewport) = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            (
                kagi::ui::e2e::control_bounds(
                    window.window_handle().window_id(),
                    "remote-browse-card",
                ),
                window.viewport_size(),
            )
        })
        .unwrap();
    let card = card.expect("the Remote Browse card is drawn");
    let centre = card.center();
    assert!(
        card.origin.y > gpui::px(0.) && card.bottom() < viewport.height,
        "the card lies inside the window: {card:?} in {viewport:?}"
    );
    assert!(
        (centre.y - viewport.height / 2.).abs() < viewport.height / 4.
            && (centre.x - viewport.width / 2.).abs() < viewport.width / 4.,
        "the card is centred: {card:?} in {viewport:?}"
    );
    press_key(cx, &app, window, "escape");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).remote_browse().is_none()));
    assert_eq!(home(cx, &app), Some(true));

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
