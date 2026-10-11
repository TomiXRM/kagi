//! #1198: production macOS Settings host, EN/JA × 100/167%.
use crate::macos::{build_fixture, mount, unmount};
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

fn matrix(cx: &mut VisualTestAppContext, leg: Leg) {
    let _saved = GlobalSettings::capture();
    let _keys = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "lang",
        "ui_zoom",
        "settings_pane",
        "settings_window_position",
    ]);
    cx.update(|cx| cx.set_global(SettingsHostForE2e { native: true }));
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            run_cell(cx, lang, zoom, leg);
        }
    }
}

fn run_cell(cx: &mut VisualTestAppContext, lang: i18n::Lang, zoom: f32, leg: Leg) {
    i18n::set_lang(lang);
    theme::set_zoom(zoom);
    theme::set_active("apple-light");
    kagi::ui::settings::write_setting("settings_pane", Some("0"));
    cx.update(theme::sync_gpui_component_theme);
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
