//! #1349 / #1350: Appearance control semantics on the native Settings host.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, unmount};
use crate::settings_controls::{click, draw};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Msg},
    settings, theme, KagiApp,
};

fn appearance_matrix(
    cx: &mut VisualTestAppContext,
    leg: fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_pane", "settings_window_frame"]);
    let _native = crate::settings_window::SettingsHostGuard::native(cx);
    for (width, height) in [(940., 660.), (1392., 883.)] {
        for lang in [i18n::Lang::En, i18n::Lang::Ja] {
            for zoom in [1., 1.67] {
                for preset in ["apple-light", "apple-dark"] {
                    i18n::set_lang(lang);
                    theme::set_zoom(zoom);
                    theme::set_active(preset);
                    cx.update(theme::sync_gpui_component_theme);
                    settings::write_setting("settings_pane", Some("0"));
                    let fixture = build_fixture();
                    let cell = std::rc::Rc::new(std::cell::RefCell::new(None));
                    let capture = cell.clone();
                    let state = e2e::app_state(fixture.path()).unwrap();
                    let repo: AnyWindowHandle = crate::macos::open_offscreen(
                        cx,
                        gpui::size(gpui::px(width), gpui::px(height)),
                        move |w, cx| e2e::mount_root(state, w, cx, &capture),
                    )
                    .into();
                    let app = cell.borrow_mut().take().unwrap();
                    cx.run_until_parked();
                    cx.update_window(repo, |_, w, cx| {
                        app.update(cx, |app, cx| app.handle_menu_command("app.settings", w, cx));
                    })
                    .unwrap();
                    cx.run_until_parked();
                    let window = cx.read(|cx| {
                        cx.windows()
                            .into_iter()
                            .find(|w| w.window_id() != repo.window_id())
                            .unwrap()
                    });
                    leg(cx, &app, window);
                    cx.update_window(window, |_, w, _| w.remove_window())
                        .unwrap();
                    unmount(cx, app, repo);
                }
            }
        }
    }
}

fn focus_theme(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    cx.update_window(window, |_, w, cx| {
        use gpui::Focusable as _;
        let focus = app
            .read(cx)
            .theme_select
            .as_ref()
            .unwrap()
            .read(cx)
            .focus_handle(cx);
        focus.focus(w, cx);
    })
    .unwrap();
}

fn theme_ax_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    draw(cx, window);
    let ax = kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
        .expect("rendered theme picker");
    assert_eq!(ax.role, gpui::Role::ComboBox);
    assert_eq!(ax.name, Msg::SettingsTheme.t());
    assert_eq!(ax.value, theme::theme().display_name());
    assert_eq!(ax.expanded, Some(false));
    let slug = theme::theme().slug.to_string();
    focus_theme(cx, app, window);
    kagi::ui::settings_view::take_theme_option_ax_for_e2e();
    keys(cx, window, "enter");
    draw(cx, window);
    assert_eq!(
        kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
            .unwrap()
            .expanded,
        Some(true)
    );
    let options = kagi::ui::settings_view::take_theme_option_ax_for_e2e();
    assert!(!options.is_empty());
    assert!(options
        .iter()
        .all(|ax| ax.role == gpui::Role::ListBoxOption && !ax.name.is_empty()));
    keys(cx, window, "down escape");
    draw(cx, window);
    assert_eq!(theme::theme().slug, slug);
    assert_eq!(
        kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
            .unwrap()
            .expanded,
        Some(false)
    );
    assert!(cx
        .update_window(window, |_, w, cx| {
            use gpui::Focusable as _;
            app.read(cx)
                .theme_select
                .as_ref()
                .unwrap()
                .read(cx)
                .focus_handle(cx)
                .is_focused(w)
        })
        .unwrap());
    keys(cx, window, "down down enter");
    draw(cx, window);
    let selected = cx.read(|cx| {
        app.read(cx)
            .theme_select
            .as_ref()
            .unwrap()
            .read(cx)
            .selected_value()
            .unwrap()
            .to_string()
    });
    assert_eq!(theme::theme().slug, selected);
    assert_eq!(
        settings::read_setting("theme").as_deref(),
        Some(selected.as_str())
    );
    assert_eq!(
        kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select")
            .unwrap()
            .value,
        theme::theme().display_name()
    );
    custom_theme_ax(cx, app, window);
}

pub fn scenario_settings_theme_select_ax(cx: &mut VisualTestAppContext) {
    appearance_matrix(cx, theme_ax_leg);
    eprintln!("[gui-e2e] PASS settings_theme_select_ax EN/JA Light/Dark 100/167%");
}

fn zoom_ax_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for zoom in [1.0, 0.7, 1.67, 1.556, 1.667] {
        theme::set_zoom(zoom);
        cx.update(theme::sync_gpui_component_theme);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        let value = format!("{:.0}%", theme::zoom() * 100.);
        let status = kagi::ui::settings_view::appearance_ax_for_e2e("zoom-value")
            .expect("rendered zoom value");
        assert_eq!(status.role, gpui::Role::Status);
        assert_eq!(status.name, i18n::settings_zoom_value(&value));
        if i18n::lang() == i18n::Lang::Ja {
            assert_eq!(status.name, format!("{} {value}", Msg::SettingsZoom.t()));
        }
        for (id, increase) in [("zoom-dec", false), ("zoom-inc", true)] {
            let ax =
                kagi::ui::settings_view::appearance_ax_for_e2e(id).expect("rendered zoom button");
            assert_eq!(ax.role, gpui::Role::Button);
            assert_eq!(ax.name, i18n::settings_zoom_action(increase, &value));
            if i18n::lang() == i18n::Lang::Ja {
                let verb = if increase { "上げる" } else { "下げる" };
                assert_eq!(
                    ax.name,
                    format!("{}を{verb} ({value})", Msg::SettingsZoom.t())
                );
            }
            assert_eq!(
                ax.disabled,
                theme::step_zoom(theme::zoom(), increase) == theme::zoom()
            );
        }
    }
    zoom_keyboard_ax(cx, app, window);
}

pub fn scenario_settings_zoom_stepper_ax(cx: &mut VisualTestAppContext) {
    appearance_matrix(cx, zoom_ax_leg);
    eprintln!("[gui-e2e] PASS settings_zoom_stepper_ax EN/JA Light/Dark 70/100/167%");
}

struct CustomThemeRestore {
    themes: Vec<theme::Theme>,
    active: String,
}

impl Drop for CustomThemeRestore {
    fn drop(&mut self) {
        theme::install_custom_themes(std::mem::take(&mut self.themes));
        theme::set_active(&self.active);
    }
}

fn custom_theme_ax(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let _restore = CustomThemeRestore {
        themes: theme::all_themes()
            .into_iter()
            .filter(|t| t.is_custom())
            .map(|t| (*t).clone())
            .collect(),
        active: theme::theme().slug.to_string(),
    };
    let themes = ["ax-theme-a", "ax-theme-b"].map(|slug| {
        let mut theme = (*theme::theme_by_slug("apple-light").unwrap()).clone();
        theme.slug = slug.into();
        theme.name = "Duplicate".into();
        theme
    });
    theme::install_custom_themes(themes.into());
    cx.update_window(window, |_, w, cx| {
        let select = app.read(cx).theme_select.clone().unwrap();
        select.update(cx, |state, cx| {
            state.set_items(
                kagi::ui::settings_view::theme_options()
                    .into_iter()
                    .filter(|t| t.slug.starts_with("ax-theme-"))
                    .collect(),
                w,
                cx,
            );
            state.set_selected_value(&"ax-theme-a".into(), w, cx);
        });
        app.update(cx, |app, cx| app.set_theme("ax-theme-a", cx));
    })
    .unwrap();
    draw(cx, window);
    focus_theme(cx, app, window);
    kagi::ui::settings_view::take_theme_option_ax_for_e2e();
    keys(cx, window, "enter");
    draw(cx, window);
    let options = kagi::ui::settings_view::take_theme_option_ax_for_e2e();
    assert_eq!(options.len(), 2);
    assert!(options
        .iter()
        .all(|ax| ax.role == gpui::Role::ListBoxOption && ax.name == "Duplicate"));
    keys(cx, window, "down enter");
    draw(cx, window);
    assert_eq!(theme::theme().slug, "ax-theme-b");
    assert_eq!(
        settings::read_setting("theme").as_deref(),
        Some("ax-theme-b")
    );
    let ax = kagi::ui::settings_view::appearance_ax_for_e2e("settings-theme-select").unwrap();
    assert_eq!(ax.value, "Duplicate");
    assert_eq!(ax.expanded, Some(false));
}

fn zoom_keyboard_ax(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for (start, increase, expected) in [
        (1., false, 0.9),
        (1., true, 1.1),
        (1.556, false, 1.5),
        (1.556, true, 1.6),
        (1.667, true, 1.67),
        (0.8, false, 0.7),
    ] {
        theme::set_zoom(start);
        cx.update(theme::sync_gpui_component_theme);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        focus_zoom_by_tab(cx, app, window, "zoom-dec", None);
        if increase {
            e2e::pr_detail::clear_key_focus();
            keys(cx, window, "tab");
            draw_focus(cx, window);
            keys(cx, window, "f19");
            assert_eq!(
                e2e::pr_detail::key_focus(),
                Some(gpui::ElementId::from("zoom-inc")),
                "one Tab moves directly between zoom buttons"
            );
        }
        keys(cx, window, if increase { "space" } else { "enter" });
        draw(cx, window);
        assert_eq!(theme::zoom(), expected);
        let value = format!("{:.0}%", expected * 100.);
        assert_eq!(
            kagi::ui::settings_view::appearance_ax_for_e2e("zoom-value")
                .unwrap()
                .value,
            value
        );
        assert_eq!(
            settings::read_setting("ui_zoom").unwrap(),
            format!("{:.0}", expected * 1000.)
        );
        if expected == theme::ZOOM_MIN || expected == theme::ZOOM_MAX {
            assert_boundary_focus(cx, app, window, increase);
        }
    }
    for (zoom, id, next) in [
        (0.7, "zoom-dec", "zoom-inc"),
        (1.67, "zoom-inc", "zoom-dec"),
    ] {
        theme::set_zoom(zoom);
        cx.update(theme::sync_gpui_component_theme);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        click(cx, window, id);
        assert_eq!(theme::zoom(), zoom, "bound direction is inert");
        focus_zoom_by_tab(cx, app, window, next, Some(id));
        if zoom == theme::ZOOM_MAX {
            e2e::pr_detail::clear_key_focus();
            keys(cx, window, "tab");
            draw_focus(cx, window);
            keys(cx, window, "f19");
            assert_ne!(
                e2e::pr_detail::key_focus(),
                Some(gpui::ElementId::from(id)),
                "disabled increment is not a Tab stop"
            );
        }
    }
}

fn focus_zoom_by_tab(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    id: &'static str,
    disabled: Option<&'static str>,
) {
    focus_theme(cx, app, window);
    draw_focus(cx, window);
    for _ in 0..30 {
        keys(cx, window, "tab");
        draw_focus(cx, window);
        e2e::pr_detail::clear_key_focus();
        keys(cx, window, "f19");
        let focused = e2e::pr_detail::key_focus();
        if let Some(disabled) = disabled {
            assert_ne!(focused, Some(gpui::ElementId::from(disabled)));
        }
        if focused == Some(gpui::ElementId::from(id)) {
            return;
        }
    }
    panic!("Tab did not reach {id}");
}

// Focus-only changes need one frame, not four Cocoa content-height settles.
fn draw_focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, w, cx| {
        w.refresh();
        w.draw(cx).clear();
    })
    .unwrap();
}

fn assert_boundary_focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    increased: bool,
) {
    e2e::pr_detail::clear_key_focus();
    keys(cx, window, "f19");
    let opposite = if increased { "zoom-dec" } else { "zoom-inc" };
    assert_eq!(
        e2e::pr_detail::key_focus(),
        Some(gpui::ElementId::from(opposite)),
        "reaching a bound preserves focus on the opposite stepper"
    );
    keys(cx, window, "tab");
    draw_focus(cx, window);
    assert_eq!(cx.update_window(window, |_, w, cx| app.read(cx).settings_switch_focused_for_e2e(w)).unwrap(), Some(0), "Tab after reaching a bound continues to the adjacent Compact graph switch, not the window's first stop");
}
