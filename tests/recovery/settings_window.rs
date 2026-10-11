//! #1198: production macOS Settings host, EN/JA × 100/167%.
use crate::macos::{build_fixture, drain_native_events, mount, mount_state, unmount};
use crate::recovery_layout::GlobalSettings;
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, i18n, settings_view::SettingsHostForE2e, theme};

fn draw(cx: &mut VisualTestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

type Leg = fn(&mut VisualTestAppContext, AnyWindowHandle, AnyWindowHandle);
fn settle(cx: &mut VisualTestAppContext, handle: AnyWindowHandle) {
    // Cocoa content-size updates and their bounds notifications are queued on
    // the native main loop, not the deterministic GPUI dispatcher.
    for _ in 0..4 {
        draw(cx, handle);
        drain_native_events();
        cx.run_until_parked();
    }
}

struct SettingsHostGuard {
    cx: VisualTestAppContext,
    previous: bool,
}

impl SettingsHostGuard {
    fn native(cx: &mut VisualTestAppContext) -> Self {
        let previous = cx.read(|cx| cx.global::<SettingsHostForE2e>().native);
        cx.update(|cx| cx.set_global(SettingsHostForE2e { native: true }));
        Self {
            cx: cx.clone(),
            previous,
        }
    }
}

impl Drop for SettingsHostGuard {
    fn drop(&mut self) {
        self.cx.update(|cx| {
            cx.set_global(SettingsHostForE2e {
                native: self.previous,
            })
        });
    }
}

struct AutoFetchGuard(bool);

impl Drop for AutoFetchGuard {
    fn drop(&mut self) {
        theme::set_auto_fetch(self.0);
    }
}

struct QuitGuard(VisualTestAppContext);

impl QuitGuard {
    fn capture(cx: &mut VisualTestAppContext) -> Self {
        cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
        Self(cx.clone())
    }
}

impl Drop for QuitGuard {
    fn drop(&mut self) {
        self.0.update(|cx| {
            cx.remove_global::<e2e::QuitForE2e>();
        });
    }
}

fn configurations(cx: &mut VisualTestAppContext, mut run: impl FnMut(&mut VisualTestAppContext)) {
    let _saved = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "lang",
        "ui_zoom",
        "settings_pane",
        "settings_window_position",
        "settings_window_height",
        "settings_window_frame",
        "auto_fetch",
    ]);
    let _host = SettingsHostGuard::native(cx);
    let _auto_fetch = AutoFetchGuard(theme::auto_fetch());
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            i18n::set_lang(lang);
            theme::set_zoom(zoom);
            theme::set_active("apple-light");
            kagi::ui::settings::write_setting("settings_pane", Some("0"));
            cx.update(theme::sync_gpui_component_theme);
            run(cx);
        }
    }
}

fn matrix(cx: &mut VisualTestAppContext, leg: Leg) {
    configurations(cx, |cx| run_cell(cx, leg));
}

fn run_cell(cx: &mut VisualTestAppContext, leg: Leg) {
    let lang = i18n::lang();
    let zoom = theme::zoom();
    let fixture = build_fixture();
    let (app, repo) = mount(cx, fixture.path());
    let tab_state = cx.read(|cx| {
        let app = app.read(cx);
        (app.tabs.clone(), app.active_tab, app.repo_path.clone())
    });
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let windows = cx.read(|cx| cx.windows());
    assert_eq!(
        windows.len(),
        2,
        "Cmd-, opens one separate Settings window ({lang:?}/{zoom})"
    );
    let settings = *windows
        .iter()
        .find(|w| w.window_id() != repo.window_id())
        .unwrap();
    draw(cx, settings);
    leg(cx, repo, settings);
    assert_eq!(
        cx.read(|cx| {
            let app = app.read(cx);
            (app.tabs.clone(), app.active_tab, app.repo_path.clone())
        }),
        tab_state,
        "Settings never changes repository tabs or selection"
    );
    if cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == settings.window_id())
    }) {
        cx.update_window(settings, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
    }
    unmount(cx, app, repo);
}

pub fn scenario_settings_window_open_raise(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(
            windows.len(),
            2,
            "second Cmd-, never creates a third window"
        );
        assert!(
            windows
                .iter()
                .any(|w| w.window_id() == settings.window_id()),
            "same host survives raise"
        );
        cx.simulate_keystrokes(settings, "cmd-,");
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| cx.windows().len()), 2);
        cx.simulate_keystrokes(settings, "escape");
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            2,
            "Escape does not close Settings"
        );
        let pane = e2e::control_bounds(settings.window_id(), "settings-pane-1")
            .expect("Language pane")
            .center();
        cx.simulate_click(settings, pane, gpui::Modifiers::none());
        draw(cx, settings);
        assert!(e2e::control_bounds(settings.window_id(), "settings-language-heading").is_some());
        cx.simulate_keystrokes(settings, "cmd-w");
        cx.run_until_parked();
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let reopened = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .expect("reopened Settings")
        });
        draw(cx, reopened);
        assert!(
            e2e::control_bounds(reopened.window_id(), "settings-language-heading").is_some(),
            "reopen restores the last pane"
        );
        cx.update_window(reopened, |_, window, _| {
            assert_eq!(
                window.viewport_size().width,
                gpui::px(715.),
                "fixed logical width at either zoom"
            );
        })
        .unwrap();
        cx.simulate_keystrokes(reopened, "cmd-w");
        cx.run_until_parked();
    });
    eprintln!("[gui-e2e] PASS settings_window_open_raise");
}

pub fn scenario_settings_window_close_keeps_repo(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        // Record repository root identity as well as all measured tab geometry;
        // Cmd-W must act in the settings dispatch tree, not the repository one.
        let before = cx
            .update_window(repo, |root, _, _| root.entity_id())
            .unwrap();
        cx.simulate_keystrokes(settings, "cmd-w");
        cx.run_until_parked();
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(windows.len(), 1, "Cmd-W closes only Settings");
        assert_eq!(windows[0].window_id(), repo.window_id());
        assert_eq!(
            cx.update_window(repo, |root, _, _| root.entity_id())
                .unwrap(),
            before
        );
        draw(cx, repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_close_keeps_repo");
}

pub fn scenario_settings_window_live_apply(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        let fixture = build_fixture();
        let (other_app, other_repo) = mount(cx, fixture.path());
        let before_token = theme::theme().panel;
        let picker = e2e::control_bounds(settings.window_id(), "settings-theme-select")
            .expect("existing theme picker in host");
        cx.simulate_click(settings, picker.center(), gpui::Modifiers::none());
        draw(cx, settings);
        cx.simulate_keystrokes(settings, "up enter");
        cx.run_until_parked();
        assert_ne!(
            theme::theme().slug,
            "apple-light",
            "picker applies a different theme through its existing callback"
        );
        let token = theme::theme().panel;
        assert_ne!(
            token, before_token,
            "the selected theme changes repository chrome"
        );
        for repo in [repo, other_repo] {
            draw(cx, repo);
            let at = e2e::control_bounds(repo.window_id(), "toolbar-drag-region")
                .expect("blank repository chrome")
                .center();
            cx.update_window(repo, |_, window, _| {
                let image = window.render_to_image().expect("repository native image");
                let scale = window.scale_factor();
                let pixel = image
                    .get_pixel(
                        (f32::from(at.x) * scale) as u32,
                        (f32::from(at.y) * scale) as u32,
                    )
                    .0;
                for (got, expected) in
                    pixel[..3]
                        .iter()
                        .zip([(token >> 16) as u8, (token >> 8) as u8, token as u8])
                {
                    assert!(
                        got.abs_diff(expected) <= 2,
                        "repository paints new theme token: {pixel:?}, expected {token:06x}"
                    );
                }
            })
            .unwrap();
        }
        unmount(cx, other_app, other_repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_live_apply");
}

pub fn scenario_settings_window_owner_release(cx: &mut VisualTestAppContext) {
    configurations(cx, |cx| {
        let fixture = build_fixture();
        let (app, repo) = mount(cx, fixture.path());
        theme::set_auto_fetch(true);
        app.update(cx, |app, cx| app.ensure_auto_fetch_ticker(cx));
        assert!(cx.read(|cx| app.read(cx).auto_fetch_ticker_alive));
        draw(cx, repo);
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let settings = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .expect("Settings opened")
        });
        draw(cx, settings);
        let other_fixture = build_fixture();
        let (other_app, other_repo) = mount(cx, other_fixture.path());
        let weak = app.downgrade();
        unmount(cx, app, repo);
        assert!(
            weak.upgrade().is_none(),
            "Settings and its rendered callbacks must not retain the closed repository"
        );
        let windows = cx.read(|cx| cx.windows());
        assert_eq!(windows.len(), 1, "owner release also closes Settings");
        assert_eq!(windows[0].window_id(), other_repo.window_id());
        draw(cx, other_repo);
        unmount(cx, other_app, other_repo);
    });
    eprintln!("[gui-e2e] PASS settings_window_owner_release");
}

pub fn scenario_settings_window_position_restore(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, repo, settings| {
        let pane = e2e::control_bounds(settings.window_id(), "settings-pane-1")
            .expect("Language pane")
            .center();
        cx.simulate_click(settings, pane, gpui::Modifiers::none());
        settle(cx, settings);
        let before = cx
            .update_window(settings, |_, window, _| window.bounds())
            .unwrap();
        let mut current = settings;
        for _ in 0..3 {
            cx.simulate_keystrokes(current, "cmd-w");
            cx.run_until_parked();
            drain_native_events();
            cx.simulate_keystrokes(repo, "cmd-,");
            cx.run_until_parked();
            current = cx.read(|cx| {
                cx.windows()
                    .into_iter()
                    .find(|w| w.window_id() != repo.window_id())
                    .unwrap()
            });
            settle(cx, current);
            let after = cx
                .update_window(current, |_, window, _| window.bounds())
                .unwrap();
            assert!(
                (f32::from(before.origin.x) - f32::from(after.origin.x)).abs() <= 1.
                    && (f32::from(before.origin.y) - f32::from(after.origin.y)).abs() <= 1.
                    && (f32::from(before.size.height) - f32::from(after.size.height)).abs() <= 1.,
                "Settings frame must not drift across reopen: {before:?} -> {after:?}"
            );
        }
        cx.simulate_keystrokes(current, "cmd-w");
        cx.run_until_parked();
    });
    configurations(cx, restart_settings_after_app_quit);
    eprintln!("[gui-e2e] PASS settings_window_position_restore");
}

fn restart_settings_after_app_quit(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    // Start at a different saved height, then let the actual page resize.
    let mut frame = kagi::ui::settings::settings_window_frame().unwrap_or(
        kagi::ui::settings::SettingsWindowFrame {
            x: 100.,
            y: 100.,
            height: 500.,
        },
    );
    frame.height = 500.;
    kagi::ui::settings::write_settings_window_frame(frame);
    let (app, repo) = mount(cx, fixture.path());
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != repo.window_id())
            .unwrap()
    });
    settle(cx, settings);
    let before = cx
        .update_window(settings, |_, window, _| {
            (window.bounds(), f32::from(window.viewport_size().height))
        })
        .unwrap();
    assert!(before.1 > 500., "General pane resized the initial frame");
    let _quit = QuitGuard::capture(cx);
    cx.simulate_keystrokes(repo, "cmd-q");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested),
        "repository Cmd-Q dispatched Quit"
    );
    let saved_height = kagi::ui::settings::settings_window_frame()
        .expect("frame persisted at app quit")
        .height;
    assert!(
        (saved_height - before.1).abs() <= 1.,
        "app quit persists Settings height without closing it"
    );
    // Process shutdown does not dispatch per-window close. Dispose Settings
    // first so owner-release cleanup cannot accidentally save missing geometry.
    cx.update_window(settings, |_, window, _| window.remove_window())
        .unwrap();
    unmount(cx, app, repo);
    let (app, repo) = mount(cx, fixture.path());
    draw(cx, repo);
    cx.simulate_keystrokes(repo, "cmd-,");
    cx.run_until_parked();
    let settings = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != repo.window_id())
            .unwrap()
    });
    settle(cx, settings);
    let after = cx
        .update_window(settings, |_, window, _| window.bounds())
        .unwrap();
    assert!(
        (f32::from(before.0.origin.x) - f32::from(after.origin.x)).abs() <= 1.
            && (f32::from(before.0.origin.y) - f32::from(after.origin.y)).abs() <= 1.
            && (f32::from(before.0.size.height) - f32::from(after.size.height)).abs() <= 1.,
        "Settings frame must not drift after app quit/restart: {:?} -> {after:?}",
        before.0
    );
    cx.simulate_keystrokes(settings, "cmd-w");
    cx.run_until_parked();
    unmount(cx, app, repo);
}

pub fn scenario_settings_window_dock_reopen(cx: &mut VisualTestAppContext) {
    configurations(cx, |cx| {
        let _session = crate::gui_isolation::SavedKeys::keep(&["session_repos", "session_active"]);
        let fixture = build_fixture();
        let (app, repo) = mount(cx, fixture.path());
        draw(cx, repo);
        assert!(
            cx.update(e2e::dock_reopen_state).is_none(),
            "live repository prevents duplicate"
        );
        cx.simulate_keystrokes(repo, "cmd-,");
        cx.run_until_parked();
        let settings = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|w| w.window_id() != repo.window_id())
                .unwrap()
        });
        draw(cx, settings);
        // Keep the harness's observation handle until after the reopen decision:
        // the repository root is gone, but Settings still exists in this turn.
        cx.update_window(repo, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(cx.read(|cx| cx.windows().len()), 1);
        kagi::ui::settings::write_setting("session_repos", fixture.path().to_str());
        kagi::ui::settings::write_setting("session_active", Some("0"));
        let fresh = cx
            .update(e2e::dock_reopen_state)
            .expect("Settings-only Dock reopen restores repo");
        assert_eq!(fresh.tabs.len(), 1);
        assert_eq!(fresh.tabs[0].path, fixture.path());
        let (restored, reopened) = mount_state(cx, fresh);
        assert_eq!(cx.read(|cx| cx.windows().len()), 2);
        assert!(cx.update(e2e::dock_reopen_state).is_none());
        let weak = app.downgrade();
        drop(app);
        cx.update(|_| {});
        cx.run_until_parked();
        drain_native_events();
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(
            weak.upgrade().is_none(),
            "closed repository is released after Dock reopen"
        );
        assert_eq!(
            cx.read(|cx| cx.windows().len()),
            1,
            "old owner's Settings closes on release"
        );
        draw(cx, reopened);
        unmount(cx, restored, reopened);
    });
    eprintln!("[gui-e2e] PASS settings_window_dock_reopen");
}

pub fn scenario_settings_window_host_isolation(cx: &mut VisualTestAppContext) {
    assert!(!cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _host = SettingsHostGuard::native(cx);
        assert!(cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
        std::panic::resume_unwind(Box::new("exercise Settings host guard Drop"));
    }));
    assert!(unwound.is_err());
    assert!(!cx.read(|cx| cx.global::<SettingsHostForE2e>().native));
    eprintln!("[gui-e2e] PASS settings_window_host_isolation");
}
