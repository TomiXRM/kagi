//! Native window identity, stateful menus and the About task boundary.
use crate::macos::{build_fixture, git, unmount};
use crate::platform_menu_scroll::mount_short;
use gpui::Action as _;
use gpui::{AnyWindowHandle, Entity, MenuItem, Role, VisualTestAppContext};
use kagi::ui::{commands, dialog_a11y, e2e, window_menu, KagiApp};
use kagi_ui_core::i18n::{self, Lang};

fn draw(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.update_window(win, |_, w, cx| {
        w.refresh();
        w.draw(cx).clear();
    })
    .unwrap();
}
fn invoke(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    id: &'static str,
) {
    cx.update_window(win, |_, w, cx| {
        app.update(cx, |a, cx| a.handle_menu_command(id, w, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, win);
}
fn menu_labels(menus: Vec<gpui::Menu>, section: &str) -> Vec<String> {
    menus
        .into_iter()
        .find(|m| m.name.as_ref() == i18n::menu_heading(section, i18n::lang()))
        .unwrap()
        .items
        .into_iter()
        .filter_map(|i| match i {
            MenuItem::Action { name, .. } => Some(name.to_string()),
            _ => None,
        })
        .collect()
}
fn labels(cx: &mut VisualTestAppContext, section: &str) -> Vec<String> {
    menu_labels(cx.read(commands::build_menus), section)
}
fn title(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> String {
    cx.update_window(win, |_, w, _| w.window_title()).unwrap()
}
fn repo_title(repo: &std::path::Path, branch: &str) -> String {
    format!("{} — {branch}", repo.file_name().unwrap().to_string_lossy())
}
pub fn scenario_window_title_identity(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other = build_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    let linked = repo.join("linked");
    git(
        &repo,
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let (app, win) = mount_short(cx, &repo);
    draw(cx, win);
    assert_eq!(
        title(cx, win),
        repo_title(&repo, "main"),
        "native title identifies the active repository and branch"
    );
    git(&repo, &["checkout", "-b", "topic"]);
    invoke(cx, &app, win, "file.refresh");
    assert_eq!(title(cx, win), repo_title(&repo, "topic"));
    app.update(cx, |a, cx| a.open_repository(linked.clone(), cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&linked, "linked"));
    app.update(cx, |a, cx| a.open_repository(other_repo.clone(), cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&other_repo, "main"));
    invoke(cx, &app, win, "file.newTab");
    assert_eq!(title(cx, win), "Kagi");
    invoke(cx, &app, win, "file.closeTab");
    assert_eq!(title(cx, win), repo_title(&other_repo, "main"));
    app.update(cx, |a, cx| a.switch_repo(0, cx));
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(title(cx, win), repo_title(&repo, "topic"));
    unmount(cx, app, win);
}
pub fn scenario_view_menu_state_labels(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "sidebar_visible"]);
    let old = i18n::lang();
    let fixture = build_fixture();
    let other = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    let (other_app, other_win) = mount_short(cx, other.path());
    for (lang, hide, show, enter, exit) in [
        (
            Lang::En,
            "Hide Sidebar",
            "Show Sidebar",
            "Enter Full Screen",
            "Exit Full Screen",
        ),
        (
            Lang::Ja,
            "サイドバーを非表示",
            "サイドバーを表示",
            "フルスクリーンにする",
            "フルスクリーンを解除",
        ),
    ] {
        i18n::set_lang(lang);
        app.update(cx, |a, cx| {
            a.sidebar.visible = true;
            a.set_window_active_for_e2e(true, cx);
        });
        draw(cx, win);
        assert!(
            labels(cx, "View").contains(&hide.to_string()),
            "visible sidebar offers {hide}"
        );
        assert!(labels(cx, "View").contains(&enter.to_string()));
        invoke(cx, &app, win, "view.toggleSidebar");
        assert!(!cx.read(|cx| app.read(cx).sidebar.visible));
        assert!(labels(cx, "View").contains(&show.to_string()));
        other_app.update(cx, |a, cx| {
            a.sidebar.visible = true;
            a.set_window_active_for_e2e(true, cx);
        });
        draw(cx, other_win);
        assert!(
            labels(cx, "View").contains(&hide.to_string()),
            "window B owns its own sidebar label"
        );
        draw(cx, win);
        assert!(
            labels(cx, "View").contains(&show.to_string()),
            "returning to A restores A's label"
        );
        app.update(cx, |a, cx| a.set_window_active_for_e2e(false, cx));
        other_app.update(cx, |a, cx| a.set_window_active_for_e2e(false, cx));
        assert!(cx.read(|cx| cx.active_window().is_none()));
        draw(cx, other_win);
        assert!(
            labels(cx, "View").contains(&show.to_string()),
            "inactive renders retain the last foreground menu owner"
        );
        let fullscreen = commands::menus_for_state(window_menu::ViewMenuState {
            sidebar_visible: false,
            fullscreen: true,
            ..Default::default()
        });
        assert!(
            menu_labels(fullscreen, "View").contains(&exit.to_string()),
            "OS fullscreen state offers the exit action"
        );
        assert_eq!(
            commands::effective_keystroke("view.fullScreen").as_deref(),
            Some("ctrl-cmd-f")
        );
    }
    i18n::set_lang(old);
    unmount(cx, other_app, other_win);
    unmount(cx, app, win);
}
pub fn scenario_window_menu_items(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    assert_eq!(
        labels(cx, "Window"),
        [
            "Minimize",
            "Zoom",
            "Show Previous Tab",
            "Show Next Tab",
            "Bring All to Front",
            "New Window",
            "Close Window"
        ]
    );
    assert!(cx.read(|cx| matches!(
        commands::command_state(app.read(cx), "window.previousTab"),
        commands::CommandState::Disabled(_)
    )));
    let second = build_fixture();
    let third = build_fixture();
    app.update(cx, |a, cx| {
        a.open_repository(second.path().to_path_buf(), cx);
        a.open_repository(third.path().to_path_buf(), cx);
    });
    cx.run_until_parked();
    invoke(cx, &app, win, "window.previousTab");
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 1);
    invoke(cx, &app, win, "window.nextTab");
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    cx.simulate_keystrokes(win, "cmd-shift-[");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 1);
    cx.simulate_keystrokes(win, "cmd-shift-]");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    cx.simulate_keystrokes(win, "cmd-{");
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| app.read(cx).active_tab),
        1,
        "native shifted bracket representation switches to the previous tab"
    );
    cx.simulate_keystrokes(win, "cmd-}");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), 2);
    invoke(cx, &app, win, "file.newTab");
    invoke(cx, &app, win, "window.previousTab");
    assert!(!cx.read(|cx| app.read(cx).home_in_front()));
    invoke(cx, &app, win, "window.nextTab");
    assert!(cx.read(|cx| app.read(cx).home_in_front()));
    assert_eq!(
        commands::effective_keystroke("window.previousTab").as_deref(),
        Some("secondary-shift-[")
    );
    assert_eq!(
        commands::effective_keystroke("window.nextTab").as_deref(),
        Some("secondary-shift-]")
    );
    assert_eq!(cx.read(window_menu::main_window_targets), vec![win]);
    check_window_owners(cx, &app, win);
    unmount(cx, app, win);
}

fn check_window_owners(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_window_frame", "settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::native(cx);
    let other = build_fixture();
    let (other_app, other_win) = mount_short(cx, other.path());
    let other_tab = cx.read(|cx| other_app.read(cx).active_tab);
    invoke(cx, app, win, "app.settings");
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|handle| *handle != win && *handle != other_win)
            .expect("Settings utility host")
    });
    draw(cx, settings);
    assert_eq!(
        cx.read(window_menu::main_window_targets),
        vec![win, other_win],
        "Bring All to Front enumerates both main windows, never Settings"
    );
    cx.update_window(settings, |_, window, cx| {
        assert!(!window.is_action_available(&commands::PreviousTab, cx));
        assert!(!window.is_action_available(&commands::NextTab, cx));
        assert!(
            window
                .available_actions(cx)
                .iter()
                .any(|action| action.name() == commands::BringAllToFront.name()),
            "Bring All to Front is application-wide, including Settings"
        );
    })
    .unwrap();
    cx.simulate_keystrokes(settings, "cmd-shift-[");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).home_in_front()),
        "Settings cannot navigate repository tabs"
    );
    assert_eq!(cx.read(|cx| other_app.read(cx).active_tab), other_tab);
    cx.simulate_keystrokes(settings, "cmd-w");
    cx.run_until_parked();
    assert_eq!(
        title(cx, win),
        "Kagi",
        "Settings leaves the main title alone"
    );
    unmount(cx, other_app, other_win);
}
pub fn scenario_window_menu_front_order(cx: &mut VisualTestAppContext) {
    let first = build_fixture();
    let second = build_fixture();
    let (app, win) = mount_short(cx, first.path());
    let (other_app, other_win) = mount_short(cx, second.path());
    for handle in [other_win, win] {
        cx.update_window(handle, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
    }
    let before = cx.read(|cx| cx.window_stack().expect("native window stack"));
    assert_eq!(before, vec![win, other_win]);
    let key_before = cx.read(|cx| cx.active_window());
    let activation_order = cx.read(window_menu::bring_all_activation_order);
    assert_eq!(
        activation_order,
        vec![other_win, win],
        "raise back-to-front, restoring the invoking/frontmost window last"
    );
    assert_eq!(activation_order.last(), before.first());
    invoke(cx, &app, win, "window.bringAllToFront");
    assert_eq!(
        cx.read(|cx| cx.window_stack()),
        Some(before),
        "Bring All to Front preserves main-window front-to-back order"
    );
    assert_eq!(
        cx.read(|cx| cx.active_window()),
        key_before,
        "Bring All to Front preserves native key-window ownership"
    );
    unmount(cx, other_app, other_win);
    unmount(cx, app, win);
}
pub fn scenario_about_dialog(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "theme", "ui_zoom"]);
    let old_lang = i18n::lang();
    let old_theme = kagi_ui_core::theme::theme().slug.to_string();
    let old_zoom = kagi_ui_core::theme::zoom();
    let fixture = build_fixture();
    let (app, win) = mount_short(cx, fixture.path());
    for slug in ["apple-light", "apple-dark"] {
        for zoom in [1., 1.67] {
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                kagi_ui_core::theme::set_zoom(zoom);
                cx.notify();
            });
            for (lang, name) in [(Lang::En, "About Kagi"), (Lang::Ja, "Kagiについて")] {
                i18n::set_lang(lang);
                check_about(cx, &app, win, name);
            }
        }
    }
    app.update(cx, |app, cx| {
        app.set_theme(&old_theme, cx);
        kagi_ui_core::theme::set_zoom(old_zoom);
        cx.notify();
    });
    i18n::set_lang(old_lang);
    unmount(cx, app, win);
}

fn check_about(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    name: &str,
) {
    dialog_a11y::clear_recorded_a11y();
    invoke(cx, app, win, "app.about");
    let spec = dialog_a11y::recorded_dialog("about-card")
        .expect("About is a named dialog, not an Info overlay");
    assert_eq!(spec.role, Role::Dialog);
    assert_eq!(spec.label, name);
    let card = e2e::control_bounds(win.window_id(), "about-card").expect("About frame");
    let close = e2e::control_bounds(win.window_id(), "about-close").expect("Close Button");
    assert!(card.top() >= gpui::px(0.) && card.bottom() <= gpui::px(480.));
    assert!(close.top() >= card.top() && close.bottom() <= card.bottom());
    let before = cx.read(|cx| app.read(cx).active_tab);
    for key in ["tab", "shift-tab", "tab", "tab"] {
        cx.simulate_keystrokes(win, key);
        cx.run_until_parked();
        assert!(
            cx.update_window(win, |_, w, cx| app
                .read(cx)
                .modal_focus
                .as_ref()
                .unwrap()
                .contains_focused(w, cx))
                .unwrap(),
            "{key} stays in About"
        );
    }
    cx.simulate_keystrokes(win, "escape");
    cx.run_until_parked();
    draw(cx, win);
    assert_eq!(cx.read(|cx| app.read(cx).active_tab), before);
    assert!(cx.read(|cx| app.read(cx).about_modal().is_none()));
    invoke(cx, app, win, "app.about");
    cx.simulate_keystrokes(win, "enter");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).about_modal().is_none()),
        "Return closes only About"
    );
    invoke(cx, app, win, "app.about");
    close_about_button(cx, app, win);
}

fn close_about_button(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    let close = e2e::control_bounds(win.window_id(), "about-close").expect("Close Button");
    cx.simulate_event(
        win,
        gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: close.center(),
            click_count: 1,
            ..Default::default()
        },
    );
    cx.simulate_event(
        win,
        gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: close.center(),
            click_count: 1,
            ..Default::default()
        },
    );
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).about_modal().is_none()),
        "Close Button dismisses About"
    );
}
