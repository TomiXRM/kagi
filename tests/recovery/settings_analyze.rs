//! #1404 / #1415 / #1416: Settings draft safety and persistent form help.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, settings, theme, KagiApp};

use kagi::ui::i18n::Msg;
fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for id in [
        "settings-analyze-reset-alert",
        "settings-analyze-dirty",
        "settings-analyze-saved",
    ] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    crate::settings_controls::draw(cx, window);
}

struct IgnoreRestore(Option<Vec<u8>>);
impl Drop for IgnoreRestore {
    fn drop(&mut self) {
        let path = settings::analyze_ignore_path().unwrap();
        if let Some(bytes) = &self.0 {
            std::fs::write(path, bytes).unwrap();
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn matrix(
    cx: &mut VisualTestAppContext,
    pane: &str,
    leg: fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    matrix_host(cx, pane, true, leg);
}

fn matrix_host(
    cx: &mut VisualTestAppContext,
    pane: &str,
    native: bool,
    leg: fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    let _presentation = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _keys = crate::gui_isolation::SavedKeys::keep(&["settings_pane", "settings_window_frame"]);
    let _host = if native {
        crate::settings_window::SettingsHostGuard::native(cx)
    } else {
        crate::settings_window::SettingsHostGuard::portable(cx)
    };
    let _ignore = IgnoreRestore(std::fs::read(settings::analyze_ignore_path().unwrap()).ok());
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            for viewport in [(940., 660.), (1392., 883.)] {
                i18n::set_lang(lang);
                theme::set_zoom(zoom);
                theme::set_active(if viewport.0 == 940. {
                    "apple-light"
                } else {
                    "apple-dark"
                });
                cx.update(theme::sync_gpui_component_theme);
                settings::write_setting("settings_pane", Some(pane));
                settings::write_analyze_ignore("saved-pattern\n").unwrap();
                let fixture = build_fixture();
                crate::macos::git(fixture.path(), &["branch", "settings-plan"]);
                let (app, repo) = mount(cx, fixture.path());
                cx.update_window(repo, |_, window, cx| {
                    window.resize(gpui::size(gpui::px(viewport.0), gpui::px(viewport.1)));
                    app.update(cx, |app, cx| {
                        app.handle_menu_command("app.settings", window, cx)
                    });
                })
                .unwrap();
                cx.run_until_parked();
                let window = if native {
                    cx.read(|cx| {
                        cx.windows()
                            .into_iter()
                            .find(|w| w.window_id() != repo.window_id())
                            .unwrap()
                    })
                } else {
                    repo
                };
                draw(cx, window);
                leg(cx, &app, window);
                if native
                    && cx.read(|cx| {
                        cx.windows()
                            .iter()
                            .any(|w| w.window_id() == window.window_id())
                    })
                {
                    cx.update_window(window, |_, window, _| window.remove_window())
                        .unwrap();
                }
                unmount(cx, app, repo);
            }
        }
    }
}
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    crate::settings_controls::click(cx, window, id);
    draw(cx, window);
}
fn draft(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    let input = cx.read(|cx| app.read(cx).analyze_ignore_input.clone().unwrap());
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(text, window, cx))
    })
    .unwrap();
    draw(cx, window);
}

fn reopen(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> AnyWindowHandle {
    let repo = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != window.window_id())
            .unwrap()
    });
    cx.simulate_keystrokes(window, "cmd-w");
    cx.run_until_parked();
    cx.update_window(repo, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    let reopened = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != repo.window_id())
            .unwrap()
    });
    draw(cx, reopened);
    reopened
}

fn restarted_value(cx: &mut VisualTestAppContext) {
    let existing = cx.read(|cx| cx.windows());
    let fixture = build_fixture();
    let (app, repo) = mount(cx, fixture.path());
    cx.update_window(repo, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    let settings_window = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| {
                w.window_id() != repo.window_id()
                    && !existing.iter().any(|old| old.window_id() == w.window_id())
            })
            .unwrap()
    });
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        settings::DEFAULT_ANALYZE_IGNORE,
        "fresh app owner seeds its editor from the approved disk value"
    );
    cx.update_window(settings_window, |_, window, _| window.remove_window())
        .unwrap();
    unmount(cx, app, repo);
}
fn reset_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    draft(cx, app, window, "unsaved-pattern\n");
    click(cx, window, "settings-analyze-reset");
    assert_eq!(
        settings::read_analyze_ignore_text(),
        "saved-pattern\n",
        "Reset must not write before draft-discard confirmation"
    );
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some());
    cx.simulate_keystrokes(window, "cmd-.");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_none(),
        "Command-Period cancels the native Settings alert"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        "unsaved-pattern\n",
        "Command-Period preserves the unsaved draft"
    );
    click(cx, window, "settings-analyze-reset");
    for _ in 0..2 {
        cx.simulate_keystrokes(window, "enter");
        draw(cx, window);
        assert!(
            e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some(),
            "Return retains the destructive alert without a default action"
        );
        assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    }
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        "unsaved-pattern\n"
    );
    cx.simulate_keystrokes(window, "escape");
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_none(),
        "Escape explicitly cancels the unchanged reset alert"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    let window = reopen(cx, app, window);
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        "unsaved-pattern\n",
        "reopen preserves cancelled draft"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    click(cx, window, "settings-analyze-reset");
    for _ in 0..6 {
        cx.simulate_keystrokes(window, "tab");
        draw(cx, window);
        let focused = cx
            .update_window(window, |_, window, cx| {
                app.read(cx).analyze_reset_focus_for_e2e(window, cx)
            })
            .unwrap();
        assert_eq!(focused, Some(true), "Tab stays inside the alert");
    }
    draw(cx, window);
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some(),
        "Tab cannot dismiss alert"
    );
    let mut confirm_focused = false;
    for _ in 0..6 {
        kagi::ui::button_style::modal_focus_probe();
        cx.simulate_keystrokes(window, "tab");
        draw(cx, window);
        cx.simulate_keystrokes(window, "f19");
        if kagi::ui::button_style::modal_focus_probe() == Some("settings-reset-confirm") {
            confirm_focused = true;
            break;
        }
    }
    assert!(
        confirm_focused,
        "Reset must be explicitly keyboard reachable"
    );
    crate::keyboard_nav::keys(cx, window, "enter");
    draw(cx, window);
    assert_eq!(
        settings::read_analyze_ignore_text(),
        settings::DEFAULT_ANALYZE_IGNORE
    );
    assert!(!cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        settings::DEFAULT_ANALYZE_IGNORE
    );
    cx.simulate_keystrokes(window, "cmd-w");
    cx.run_until_parked();
    restarted_value(cx);
}
fn save_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-saved").is_some(),
        "saved draft has explicit localized state"
    );
    click(cx, window, "settings-analyze-save");
    assert!(
        cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .is_empty()),
        "unchanged Save is disabled"
    );
    draft(cx, app, window, "saved-pattern\nx");
    assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    draft(cx, app, window, "saved-pattern\n");
    assert!(!cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-saved").is_some());
    draft(cx, app, window, "unsaved-pattern\n");
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-dirty").is_some());
    click(cx, window, "settings-pane-0");
    click(cx, window, "settings-pane-3");
    assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .analyze_ignore_input
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        "unsaved-pattern\n"
    );
    click(cx, window, "settings-analyze-save");
    assert_eq!(settings::read_analyze_ignore_text(), "unsaved-pattern\n");
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-saved").is_some());
}
fn help_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for id in [
        "settings-avatar-help-text",
        "settings-fetch-help-text",
        "settings-copy-help-text",
        "settings-motion-help-text",
        "settings-terminal-lock-help-text",
        "settings-custom-themes-help-text",
    ] {
        let bounds =
            e2e::control_bounds(window.window_id(), id).expect("visible help without hover");
        assert!(bounds.size.height > gpui::px(0.), "{id} has laid-out text");
        assert!(
            bounds.left() >= gpui::px(0.) && bounds.right() <= gpui::px(715.),
            "{id} wraps inside the host"
        );
    }
    for (slot, id, detail) in [
        (2, "auto-fetch-toggle", Msg::SettingsAutoFetchDesc),
        (
            4,
            "terminal-auto-lock-toggle",
            Msg::SettingsTerminalAutoLockDesc,
        ),
    ] {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.focus_settings_switch_for_e2e(slot, window, cx)
            });
        })
        .unwrap();
        draw(cx, window);
        assert_eq!(
            kagi::ui::settings_view::recorded_switch_help(id).as_deref(),
            Some(detail.t())
        );
    }
    click(cx, window, "settings-pane-2");
    for id in [
        "settings-smart-enable-help-text",
        "settings-provider-help-text",
    ] {
        assert!(e2e::control_bounds(window.window_id(), id).is_some());
    }
    click(cx, window, "settings-pane-3");
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-help-text").is_some());
}
pub fn scenario_settings_analyze_reset_confirm(cx: &mut VisualTestAppContext) {
    matrix(cx, "3", reset_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_reset_confirm");
}
pub fn scenario_settings_analyze_save_state(cx: &mut VisualTestAppContext) {
    matrix(cx, "3", save_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_save_state");
}
pub fn scenario_settings_help_text(cx: &mut VisualTestAppContext) {
    matrix(cx, "0", help_leg);
    eprintln!("[gui-e2e] PASS settings_help_text");
}

fn portable_reset_leg(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    draft(cx, app, window, "portable-draft\n");
    let frame = e2e::control_bounds(window.window_id(), "settings-frame").unwrap();
    for _ in 0..12 {
        let bounds = e2e::control_bounds(window.window_id(), "settings-analyze-reset").unwrap();
        if frame.contains(&bounds.center()) {
            cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
            break;
        }
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: gpui::point(frame.center().x, frame.bottom() - gpui::px(40.)),
                delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-500.))),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        draw(cx, window);
    }
    draw(cx, window);
    assert!(
        cx.read(|cx| app.read(cx).menu_overlay.is_some()),
        "portable Reset alert must not close its Settings overlay"
    );
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some(),
        "portable Reset alert is visibly rendered inside Settings"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    for _ in 0..6 {
        cx.simulate_keystrokes(window, "tab");
        draw(cx, window);
        assert_eq!(
            cx.update_window(window, |_, window, cx| {
                app.read(cx).analyze_reset_focus_for_e2e(window, cx)
            })
            .unwrap(),
            Some(true),
            "portable Tab stays inside the Settings alert"
        );
    }
    cx.simulate_keystrokes(window, "escape");
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_some()));
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_none());
    assert_eq!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).analyze_reset_focus_for_e2e(window, cx)
        })
        .unwrap(),
        None
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    click(cx, window, "settings-analyze-reset");
    cx.simulate_keystrokes(window, "enter");
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_some()));
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some());
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    cx.simulate_keystrokes(window, "escape");
    draw(cx, window);
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_none());
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    portable_yields_to_plan(cx, app, window);
}

fn portable_yields_to_plan(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    click(cx, window, "settings-analyze-reset");
    app.update(cx, |app, cx| app.open_plan_modal("settings-plan", cx));
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).menu_overlay.is_none()));
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    assert_eq!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).analyze_reset_focus_for_e2e(window, cx)
        })
        .unwrap(),
        None,
        "closing portable Settings retires its alert without touching the plan"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
}

fn preserve_plan_leg(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    draft(cx, app, window, "unsaved-pattern\n");
    app.update(cx, |app, cx| app.open_plan_modal("settings-plan", cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    click(cx, window, "settings-analyze-reset");
    assert!(
        cx.read(|cx| app.read(cx).plan_modal().is_some()),
        "Settings Reset must preserve the main window's checkout plan"
    );
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some());
    cx.simulate_keystrokes(window, "escape");
    draw(cx, window);
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    click(cx, window, "settings-analyze-reset");
    click(cx, window, "settings-analyze-reset-confirm");
    assert!(cx.read(|cx| app.read(cx).plan_modal().is_some()));
    assert_eq!(
        settings::read_analyze_ignore_text(),
        settings::DEFAULT_ANALYZE_IGNORE
    );
    app.update(cx, |app, _| app.clear_plan_modal());
}

pub fn scenario_settings_analyze_portable_reset(cx: &mut VisualTestAppContext) {
    matrix_host(cx, "3", false, portable_reset_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_portable_reset");
}

pub fn scenario_settings_analyze_reset_preserves_plan(cx: &mut VisualTestAppContext) {
    matrix(cx, "3", preserve_plan_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_reset_preserves_plan");
}
