//! #1404 / #1415 / #1416: Settings draft safety and persistent form help.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, i18n, settings, theme, KagiApp};

use kagi::ui::i18n::Msg;
fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "settings-analyze-reset-alert");
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
    let retained = cx.read(|cx| app.read(cx).settings_entity().unwrap());
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
        cx.read(|cx| app.read(cx).settings_entity().unwrap().entity_id()),
        retained.entity_id(),
        "closing the native window retains the production Settings view"
    );
    assert_eq!(
        cx.update_window(window, |_, window, cx| {
            app.read(cx).analyze_reset_focus_for_e2e(window, cx)
        })
        .unwrap(),
        None,
        "reopening cannot resurrect an invoking-window reset prompt"
    );
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
    assert!(e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_some());
    let window = reopen(cx, app, window);
    assert!(
        e2e::control_bounds(window.window_id(), "settings-analyze-reset-alert").is_none(),
        "closing the invoking window cancels its outstanding reset prompt"
    );
    assert_eq!(settings::read_analyze_ignore_text(), "saved-pattern\n");
    assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
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
    assert_eq!(
        crate::settings_controls::probe(cx, window, "settings-analyze-status")
            .unwrap()
            .value,
        Msg::SettingsAnalyzeIgnoreClean.t(),
        "clean draft has explicit localized state"
    );
    assert!(
        crate::settings_controls::probe(cx, window, "settings-analyze-save")
            .unwrap()
            .disabled
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
    assert_eq!(
        crate::settings_controls::probe(cx, window, "settings-analyze-status")
            .unwrap()
            .value,
        Msg::SettingsAnalyzeIgnoreClean.t()
    );
    draft(cx, app, window, "unsaved-pattern\n");
    assert_eq!(
        crate::settings_controls::probe(cx, window, "settings-analyze-status")
            .unwrap()
            .value,
        Msg::SettingsAnalyzeIgnoreUnsaved.t()
    );
    click(cx, window, "settings-pane-appearance");
    click(cx, window, "settings-pane-analyze");
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
    assert_eq!(
        crate::settings_controls::probe(cx, window, "settings-analyze-status")
            .unwrap()
            .value,
        Msg::SettingsAnalyzeIgnoreSaved.t()
    );
    assert!(
        crate::settings_controls::probe(cx, window, "settings-analyze-save")
            .unwrap()
            .disabled
    );
    stale_analyze_result(cx, app, window);
}

fn stale_analyze_result(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let view = cx.read(|cx| app.read(cx).settings_entity().unwrap());
    draft(cx, app, window, "captured-save\n");
    let request = cx.read(|cx| view.read(cx).analyze_request(cx));
    // Drive the production acknowledgement only after a genuine successful
    // fixture write, while a newer editor revision is already present.
    settings::write_analyze_ignore(request.text.as_ref()).unwrap();
    draft(cx, app, window, "newer-draft\n");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.seed_analyze_result(
                kagi_ui_settings::AnalyzeResult::Saved(request.clone()),
                window,
                cx,
            );
        });
    })
    .unwrap();
    draw(cx, window);
    assert_eq!(
        cx.read(|cx| view.read(cx).analyze_request(cx).text.to_string()),
        "newer-draft\n"
    );
    assert_eq!(
        cx.read(|cx| view.read(cx).analyze_baseline().to_string()),
        "captured-save\n"
    );
    assert!(cx.read(|cx| view.read(cx).analyze_dirty(cx)));
    assert_eq!(settings::read_analyze_ignore_text(), "captured-save\n");
    click(cx, window, "settings-analyze-save");
    assert_eq!(settings::read_analyze_ignore_text(), "newer-draft\n");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.seed_analyze_result(kagi_ui_settings::AnalyzeResult::Saved(request), window, cx);
        });
    })
    .unwrap();
    assert_eq!(
        cx.read(|cx| view.read(cx).analyze_baseline().to_string()),
        "newer-draft\n",
        "an older acknowledgement cannot roll back the newer accepted baseline"
    );
    assert!(!cx.read(|cx| view.read(cx).analyze_dirty(cx)));
}
fn help_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for (pane, id, text) in [
        (
            "graph",
            "settings-avatar-help",
            Msg::SettingsLaneCompactDesc,
        ),
        ("graph", "settings-copy-help", Msg::SettingsGraphCopyDesc),
        ("network", "settings-fetch-help", Msg::SettingsAutoFetchDesc),
        (
            "appearance",
            "settings-motion-help",
            Msg::SettingsReduceMotionDesc,
        ),
        (
            "terminal",
            "settings-terminal-lock-help",
            Msg::SettingsTerminalAutoLockDesc,
        ),
        (
            "appearance",
            "settings-custom-themes-help",
            Msg::SettingsCustomThemesDesc,
        ),
        (
            "smart_commit",
            "settings-smart-enable-help",
            Msg::SettingsSmartEnableDesc,
        ),
        (
            "analyze",
            "settings-analyze-help",
            Msg::SettingsAnalyzeIgnoreDesc,
        ),
    ] {
        click(cx, window, &format!("settings-pane-{pane}"));
        let help = crate::settings_controls::probe(cx, window, id)
            .expect("full help rendered without hover");
        let viewport =
            crate::settings_controls::bounds(cx, window, "settings-detail-scroll").unwrap();
        assert_eq!(
            help.value,
            text.t(),
            "the rendered help contains the full localized explanation"
        );
        assert!(help.bounds.size.height > gpui::px(0.));
        assert!(
            help.bounds.left() >= viewport.left() && help.bounds.right() <= viewport.right(),
            "{id} wraps inside the production detail column"
        );
    }
    provider_unavailable_help(cx, app, window);
}

fn provider_unavailable_help(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    click(cx, window, "settings-pane-smart_commit");
    // Detection has settled in matrix_host. Seed known host availability so
    // neither the explanations nor disabled choices depend on the machine PATH.
    app.update(cx, |app, cx| {
        app.smart_commit.provider = kagi::ui::smart_commit::SmartProvider::Ollama;
        app.smart_commit.ollama_available = true;
        app.smart_commit.detecting = false;
        app.smart_commit.claude_available = false;
        app.smart_commit.codex_available = false;
        app.seed_settings(cx);
    });
    draw(cx, window);
    let view = cx.read(|cx| app.read(cx).settings_entity().unwrap());
    let saved_provider = settings::read_setting("smart_commit_provider");
    let help = crate::settings_controls::probe(cx, window, "settings-provider-help").unwrap();
    assert_eq!(help.name, Msg::SettingsSmartProvider.t());
    assert!(
        help.value.contains(Msg::SettingsSmartProviderDesc.t()),
        "availability explanations supplement, not replace, the base provider help"
    );
    for provider in ["Claude Code", "Codex"] {
        let reason = i18n::provider_not_found_hint(provider);
        assert!(
            help.value.contains(reason.as_str()),
            "{provider} has an understandable unavailability explanation in the visible help"
        );
    }
    let viewport = crate::settings_controls::bounds(cx, window, "settings-detail-scroll").unwrap();
    let scroll = cx.read(|cx| view.read(cx).scroll_handle());
    scroll.set_offset(gpui::point(
        gpui::px(0.),
        scroll.offset().y - help.bounds.top() + viewport.top(),
    ));
    draw(cx, window);
    crate::recovery_layout::contained(
        viewport,
        crate::settings_controls::probe(cx, window, "settings-provider-help")
            .unwrap()
            .bounds,
        "full provider help and unavailable reasons are visible without hover",
    );
    for id in ["sc-provider-claude-code", "sc-provider-codex"] {
        let unavailable = crate::settings_controls::probe(cx, window, id).unwrap();
        assert!(unavailable.disabled && !unavailable.selected);
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| assert!(view.focus_control(id, window, cx)));
        })
        .unwrap();
        crate::keyboard_nav::keys(cx, window, "space enter");
        click(cx, window, id);
        let unchanged = crate::settings_controls::probe(cx, window, id).unwrap();
        assert!(
            unchanged.disabled && !unchanged.selected,
            "neither keyboard nor pointer activation selects an unavailable provider"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.provider),
            kagi::ui::smart_commit::SmartProvider::Ollama
        );
        assert_eq!(
            settings::read_setting("smart_commit_provider"),
            saved_provider
        );
    }
}
pub fn scenario_settings_analyze_reset_confirm(cx: &mut VisualTestAppContext) {
    matrix(cx, "analyze", reset_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_reset_confirm");
}
pub fn scenario_settings_analyze_save_state(cx: &mut VisualTestAppContext) {
    matrix(cx, "analyze", save_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_save_state");
}
pub fn scenario_settings_help_text(cx: &mut VisualTestAppContext) {
    matrix(cx, "appearance", help_leg);
    eprintln!("[gui-e2e] PASS settings_help_text");
}

fn portable_reset_leg(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    draft(cx, app, window, "portable-draft\n");
    let frame = crate::settings_controls::bounds(cx, window, "settings-frame").unwrap();
    for _ in 0..12 {
        let bounds =
            crate::settings_controls::bounds(cx, window, "settings-analyze-reset").unwrap();
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
    matrix_host(cx, "analyze", false, portable_reset_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_portable_reset");
}

pub fn scenario_settings_analyze_reset_preserves_plan(cx: &mut VisualTestAppContext) {
    matrix(cx, "analyze", preserve_plan_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_reset_preserves_plan");
}
