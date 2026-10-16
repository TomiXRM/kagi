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
    for zoom in [1.0, 0.7, 1.67, 1.556, 1.667, 2.0] {
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
    eprintln!("[gui-e2e] PASS settings_zoom_stepper_ax EN/JA Light/Dark 70/100/167/200%");
}

fn compact_threshold_ax_leg(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    // This must precede every pointer helper: click() wheel-scrolls offscreen
    // targets itself and would otherwise mask missing keyboard reveal.
    if theme::zoom() == 1.67 {
        theme::set_compact_threshold_rem(80);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        let body = e2e::control_bounds(window.window_id(), "settings-window-scrollbar").unwrap();
        let before = e2e::control_bounds(window.window_id(), "compact-threshold-inc").unwrap();
        if i18n::lang() == i18n::Lang::Ja {
            assert!(
                before.bottom() > body.bottom(),
                "167% Japanese starts with the threshold below the visible scroll body: {before:?} in {body:?}",
            );
        }
        focus_zoom_by_tab(cx, app, window, "compact-threshold-inc", None);
        draw(cx, window);
        let focused = e2e::control_bounds(window.window_id(), "compact-threshold-inc").unwrap();
        // The actual kit focus target (verified by F19 during traversal), not
        // merely its row, must clear both the fixed tabs and the body bottom.
        crate::recovery_layout::contained(
            body,
            focused,
            "keyboard-first threshold focus ring is inside the Settings scroll body",
        );
        keys(cx, window, "space");
        draw(cx, window);
        assert_eq!(theme::compact_threshold_rem(), 81);
        assert_eq!(settings::Settings::load().compact_threshold_rem(), 81);
        assert_eq!(
            settings::read_setting("compact_threshold_rem").as_deref(),
            Some("81"),
        );

        // Retained focus is not a request to pin the body on every repaint.
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: body.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(10000.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        draw(cx, window);
        draw(cx, window);
        let after = e2e::control_bounds(window.window_id(), "compact-threshold-inc").unwrap();
        assert!(
            (after.top() - before.top()).abs() < gpui::px(1.),
            "manual scroll returns to the top despite retained stepper focus",
        );
        e2e::pr_detail::clear_key_focus();
        keys(cx, window, "f19");
        assert_eq!(
            e2e::pr_detail::key_focus(),
            Some(gpui::ElementId::from("compact-threshold-inc")),
        );
    }
    for threshold in [40, 70, 160] {
        theme::set_compact_threshold_rem(threshold);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        let status = kagi::ui::settings_view::appearance_ax_for_e2e("compact-threshold-value")
            .expect("rendered relative compact threshold");
        assert_eq!(status.role, gpui::Role::Status);
        assert_eq!(status.value, format!("{threshold}rem"));
        assert_eq!(
            status.name,
            i18n::settings_compact_threshold_value(
                threshold,
                threshold as f32 * theme::rem_size_px(),
            )
        );
        for (id, increase, disabled) in [
            ("compact-threshold-dec", false, threshold == 40),
            ("compact-threshold-inc", true, threshold == 160),
        ] {
            let ax = kagi::ui::settings_view::appearance_ax_for_e2e(id).unwrap();
            assert_eq!(ax.role, gpui::Role::Button);
            assert_eq!(
                ax.name,
                i18n::settings_compact_threshold_action(increase, threshold)
            );
            assert_eq!(ax.disabled, disabled);
            if disabled {
                click(cx, window, id);
                assert_eq!(theme::compact_threshold_rem(), threshold);
            }
        }
        let (id, skip, key, expected) = if threshold == 40 {
            (
                "compact-threshold-inc",
                Some("compact-threshold-dec"),
                "space",
                41,
            )
        } else {
            ("compact-threshold-dec", None, "enter", threshold - 1)
        };
        focus_zoom_by_tab(cx, app, window, id, skip);
        keys(cx, window, key);
        draw(cx, window);
        let viewport =
            e2e::control_bounds(window.window_id(), "settings-window-scrollbar").unwrap();
        for button in ["compact-threshold-dec", "compact-threshold-inc"] {
            crate::recovery_layout::contained(
                viewport,
                e2e::control_bounds(window.window_id(), button).unwrap(),
                "threshold stepper remains keyboard-reachable inside Settings",
            );
        }
        assert_eq!(theme::compact_threshold_rem(), expected);
        assert_eq!(settings::Settings::load().compact_threshold_rem(), expected);
        assert_eq!(
            settings::read_setting("compact_threshold_rem"),
            Some(expected.to_string())
        );
        let repo = cx.read(|cx| {
            cx.windows()
                .into_iter()
                .find(|candidate| candidate.window_id() != window.window_id())
                .unwrap()
        });
        e2e::clear_control_bounds(repo.window_id(), "compact-rail");
        draw(cx, repo);
        let width = cx
            .update_window(repo, |_, w, _| f32::from(w.viewport_size().width))
            .unwrap();
        assert_eq!(
            e2e::control_bounds(repo.window_id(), "compact-rail").is_some(),
            kagi_domain::compact::is_compact(width, theme::rem_size_px(), expected),
            "native Settings control refreshes the independent repository window",
        );
        // Exercise enabled native pointer hit-testing as well as keyboard
        // activation, using the painted control bounds rather than a handler seam.
        draw(cx, window);
        click(cx, window, "compact-threshold-inc");
        draw(cx, window);
        let clicked = expected + 1;
        assert_eq!(theme::compact_threshold_rem(), clicked);
        assert_eq!(settings::Settings::load().compact_threshold_rem(), clicked);
        assert_eq!(
            settings::read_setting("compact_threshold_rem"),
            Some(clicked.to_string()),
        );
        assert_eq!(
            kagi::ui::settings_view::appearance_ax_for_e2e("compact-threshold-value")
                .unwrap()
                .value,
            format!("{clicked}rem"),
        );
        e2e::clear_control_bounds(repo.window_id(), "compact-rail");
        draw(cx, repo);
        assert_eq!(
            e2e::control_bounds(repo.window_id(), "compact-rail").is_some(),
            kagi_domain::compact::is_compact(width, theme::rem_size_px(), clicked),
            "enabled pointer increment refreshes repository layout",
        );
    }
    for (start, id, expected, opposite) in [
        (41, "compact-threshold-dec", 40, "compact-threshold-inc"),
        (159, "compact-threshold-inc", 160, "compact-threshold-dec"),
    ] {
        theme::set_compact_threshold_rem(start);
        cx.update(|cx| cx.refresh_windows());
        draw(cx, window);
        focus_zoom_by_tab(cx, app, window, id, None);
        keys(cx, window, "enter");
        draw(cx, window);
        assert_eq!(theme::compact_threshold_rem(), expected);
        e2e::pr_detail::clear_key_focus();
        keys(cx, window, "f19");
        assert_eq!(
            e2e::pr_detail::key_focus(),
            Some(gpui::ElementId::from(opposite))
        );
    }
}

pub fn scenario_settings_compact_threshold_ax(cx: &mut VisualTestAppContext) {
    appearance_matrix(cx, compact_threshold_ax_leg);
    eprintln!(
        "[gui-e2e] PASS settings_compact_threshold_ax EN/JA limits keyboard persistence conversion"
    );
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
        (1.667, true, 1.7),
        (1.9, true, 2.),
        (2., false, 1.9),
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
    for (zoom, id, next) in [(0.7, "zoom-dec", "zoom-inc"), (2., "zoom-inc", "zoom-dec")] {
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
    // Appearance now places the relative-width stepper between Zoom and Graph.
    // Preserve the real Tab order, including disabled threshold stops, rather
    // than treating the new controls as a lost-focus regression.
    let threshold = theme::compact_threshold_rem();
    for (id, enabled) in [
        (
            "compact-threshold-dec",
            threshold > kagi_domain::compact::MIN_THRESHOLD_REM,
        ),
        (
            "compact-threshold-inc",
            threshold < kagi_domain::compact::MAX_THRESHOLD_REM,
        ),
    ] {
        if enabled {
            keys(cx, window, "tab");
            draw_focus(cx, window);
            e2e::pr_detail::clear_key_focus();
            keys(cx, window, "f19");
            assert_eq!(
                e2e::pr_detail::key_focus(),
                Some(gpui::ElementId::from(id)),
                "Tab after the zoom boundary follows the visible threshold controls",
            );
        }
    }
    keys(cx, window, "tab");
    draw_focus(cx, window);
    assert_eq!(
        cx.update_window(window, |_, w, cx| app
            .read(cx)
            .settings_switch_focused_for_e2e(w))
            .unwrap(),
        Some(0),
        "Tab after the threshold controls continues to Compact graph, not the window's first stop"
    );
}
