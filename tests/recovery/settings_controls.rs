//! #1298 / #1299: real macOS Settings controls, not an offscreen overlay click.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, drain_native_events, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{self, Msg},
    settings,
    settings_view::SettingsHostForE2e,
    smart_commit::SmartProvider,
    theme, KagiApp, ToastKind,
};

pub(crate) fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    // Cocoa content-height requests are asynchronous; settle layout without
    // draining the toast ticker or changing its lifetime.
    for _ in 0..4 {
        cx.update_window(window, |_, w, cx| {
            w.refresh();
            w.draw(cx).clear();
        })
        .unwrap();
        drain_native_events();
    }
}

fn matrix(
    cx: &mut VisualTestAppContext,
    pane: &str,
    leg: fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "smart_commit_provider",
        "smart_commit_model",
        "settings_pane",
        "settings_window_frame",
    ]);
    let previous = cx.read(|cx| cx.global::<SettingsHostForE2e>().native);
    cx.update(|cx| cx.set_global(SettingsHostForE2e { native: true }));
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        for zoom in [1., 1.67] {
            for preset in ["apple-light", "apple-dark"] {
                i18n::set_lang(lang);
                theme::set_zoom(zoom);
                theme::set_active(preset);
                cx.update(theme::sync_gpui_component_theme);
                settings::write_setting("settings_pane", Some(pane));
                let fixture = build_fixture();
                let (app, repo) = mount(cx, fixture.path());
                cx.update_window(repo, |_, w, cx| {
                    app.update(cx, |app, cx| app.handle_menu_command("app.settings", w, cx))
                })
                .unwrap();
                cx.run_until_parked(); // finish detection before installing the deterministic catalog
                let window = cx.read(|cx| {
                    cx.windows()
                        .into_iter()
                        .find(|w| w.window_id() != repo.window_id())
                        .unwrap()
                });
                leg(cx, &app, window);
                if cx.read(|cx| {
                    cx.windows()
                        .iter()
                        .any(|w| w.window_id() == window.window_id())
                }) {
                    cx.update_window(window, |_, w, _| w.remove_window())
                        .unwrap();
                }
                unmount(cx, app, repo);
            }
        }
    }
    cx.update(|cx| cx.set_global(SettingsHostForE2e { native: previous }));
}

fn focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    models: bool,
    slot: usize,
) {
    cx.update_window(window, |_, w, cx| {
        app.update(cx, |app, cx| {
            app.focus_settings_choice_for_e2e(models, slot, w, cx)
        })
    })
    .unwrap();
}

fn provider_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    for count in [0, 1, 20] {
        app.update(cx, |app, cx| {
            app.smart_commit.provider = SmartProvider::Ollama;
            app.smart_commit.claude_available = count > 0;
            app.smart_commit.codex_available = false;
            app.smart_commit.detected_models = (0..count)
                .map(|n| format!("model-{n}-long-local-model-name-0123456789"))
                .collect();
            app.smart_commit.model = app.smart_commit.detected_models.first().cloned();
            cx.notify();
        });
        draw(cx, window);
        let tabs = cx.read(|cx| app.read(cx).settings_choices_for_e2e(false));
        assert_eq!(tabs.len(), 3, "provider chips expose named selectable tabs");
        assert_eq!(tabs.iter().filter(|t| t.2).count(), 1);
        assert!(tabs
            .iter()
            .all(|t| t.0 == gpui::Role::Tab && !t.1.is_empty()));
        assert!(tabs[2].1.contains(&i18n::provider_not_found_hint("Codex")));
        focus(cx, app, window, false, 0);
        keys(cx, window, "right enter");
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.provider),
            if count > 0 {
                SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode)
            } else {
                SmartProvider::Ollama
            }
        );
        draw(cx, window);
        keys(cx, window, "right space");
        assert_ne!(
            settings::read_setting("smart_commit_provider").as_deref(),
            Some("codex")
        );
        keys(cx, window, "home space");
        draw(cx, window);
        if count > 0 {
            focus(cx, app, window, true, 0);
            keys(cx, window, "end space");
            assert_eq!(
                settings::read_setting("smart_commit_model"),
                cx.read(|cx| app.read(cx).smart_commit.detected_models.last().cloned())
            );
            draw(cx, window);
            assert_eq!(
                cx.read(|cx| app
                    .read(cx)
                    .settings_choices_for_e2e(true)
                    .iter()
                    .filter(|t| t.2)
                    .count()),
                1
            );
        }
        assert!(
            cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
            "selection does not start generation"
        );
        let bounds = e2e::control_bounds(window.window_id(), "sc-provider-ollama").unwrap();
        let width = cx
            .update_window(window, |_, w, _| w.viewport_size().width)
            .unwrap();
        assert!(bounds.left() >= gpui::px(0.) && bounds.right() <= width);
        for model in cx.read(|cx| app.read(cx).smart_commit.detected_models.clone()) {
            let bounds =
                e2e::control_bounds(window.window_id(), &format!("sc-model-{model}")).unwrap();
            assert!(
                bounds.left() >= gpui::px(0.) && bounds.right() <= width,
                "long model fits Settings"
            );
        }
    }
}

pub fn scenario_settings_provider_keys(cx: &mut VisualTestAppContext) {
    matrix(cx, "2", provider_leg);
    eprintln!("[gui-e2e] PASS settings_provider_keys EN/JA Light/Dark 100/167%, 0/1/20 models");
}

pub(crate) fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    draw(cx, window);
    let mut bounds = e2e::control_bounds(window.window_id(), name).unwrap();
    let viewport = cx
        .update_window(window, |_, w, _| w.viewport_size())
        .unwrap();
    for _ in 0..4 {
        if bounds.top() >= gpui::px(0.) && bounds.bottom() <= viewport.height {
            break;
        }
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: gpui::point(viewport.width - gpui::px(20.), viewport.height / 2.),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    gpui::px(0.),
                    gpui::px(if bounds.top() < gpui::px(0.) {
                        500.
                    } else {
                        -500.
                    }),
                )),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        draw(cx, window);
        bounds = e2e::control_bounds(window.window_id(), name).unwrap();
    }
    assert!(
        bounds.top() >= gpui::px(0.) && bounds.bottom() <= viewport.height,
        "{name} is visible: {bounds:?} in {viewport:?}, {:?}/{}",
        i18n::lang(),
        theme::zoom()
    );
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    draw(cx, window);
}

fn dismiss_feedback(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let id = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap()
            .id
    });
    cx.advance_clock(std::time::Duration::from_millis(400));
    draw(cx, window);
    click(cx, window, &format!("toast-dismiss-{id}"));
    cx.advance_clock(std::time::Duration::from_millis(400));
    draw(cx, window);
}

fn dismissal_keeps_settings_commands(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    draw(cx, window);
    let close = kagi::ui::toast_stack::take_close_focus_for_e2e().expect("Settings close control");
    let repo = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|w| w.window_id() != window.window_id())
            .unwrap()
    });
    // A Settings-owned notice must never acquire a repository-host control.
    draw(cx, repo);
    assert!(
        cx.read(|cx| {
            let stack = app.read(cx).toast_stack.as_ref().unwrap().read(cx);
            let toast = stack.toasts().last().unwrap();
            assert_eq!(toast.subject, kagi::ui::toast_stack::ToastSubject::Settings);
            let hosts = toast.close_focus.borrow();
            hosts.iter().any(|(id, _)| *id == window.window_id())
                && hosts.iter().all(|(id, _)| *id != repo.window_id())
                && e2e::control_bounds(repo.window_id(), &format!("toast-dismiss-{}", toast.id))
                    .is_none()
        }),
        "Settings notice retains only its owning host's close control"
    );
    for _ in 0..20 {
        if cx
            .update_window(window, |_, w, _| close.is_focused(w))
            .unwrap()
        {
            break;
        }
        keys(cx, window, "tab");
    }
    assert!(
        cx.update_window(window, |_, w, _| close.is_focused(w))
            .unwrap(),
        "Tab reaches Settings toast close"
    );
    keys(cx, window, "enter");
    cx.advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    draw(cx, window);
    let id = cx.read(|cx| {
        app.read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .map(|t| t.id)
    });
    assert!(id.is_none(), "keyboard close removes the final toast");
    assert!(
        !cx.update_window(window, |_, w, _| close.is_focused(w))
            .unwrap(),
        "retired close control releases Settings focus before further input"
    );
    keys(cx, window, "tab");
    keys(cx, window, "left");
    assert_eq!(
        settings::read_setting("settings_pane").as_deref(),
        Some("2"),
        "pane arrows work after toast removal"
    );
    keys(cx, window, "right");
    assert_eq!(
        settings::read_setting("settings_pane").as_deref(),
        Some("3")
    );
    cx.simulate_keystrokes(window, "cmd-w");
    cx.run_until_parked();
    assert!(
        !cx.read(|cx| cx
            .windows()
            .iter()
            .any(|w| w.window_id() == window.window_id())),
        "Cmd-W still reaches Settings after focused toast removal"
    );
}

fn unwritable_directory(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    path: &std::path::Path,
) {
    use std::os::unix::fs::PermissionsExt as _;
    struct Restore(std::path::PathBuf, std::fs::Permissions);
    impl Drop for Restore {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.0, self.1.clone()).unwrap();
        }
    }
    std::fs::write(path, b"saved bytes").unwrap();
    for name in ["settings-analyze-save", "settings-analyze-reset"] {
        let parent = path.parent().unwrap();
        let restore = Restore(
            parent.to_path_buf(),
            std::fs::metadata(parent).unwrap().permissions(),
        );
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0)).unwrap();
        click(cx, window, name);
        if name == "settings-analyze-reset" {
            click(cx, window, "settings-analyze-reset-confirm");
        }
        let error = cx
            .read(|cx| {
                app.read(cx)
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .last()
                    .cloned()
            })
            .unwrap();
        assert_eq!(error.kind, ToastKind::Error);
        assert!(error.message.contains("Permission denied"));
        assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
        drop(restore);
        assert_eq!(std::fs::read(path).unwrap(), b"saved bytes");
        dismiss_feedback(cx, app, window);
    }
}

fn analyze_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let input = cx.read(|cx| app.read(cx).analyze_ignore_input.clone().unwrap());
    cx.update_window(window, |_, w, cx| {
        input.update(cx, |s, cx| s.set_value("draft-ignore\n", w, cx))
    })
    .unwrap();
    let path = settings::analyze_ignore_path().unwrap();
    let original = std::fs::read(&path).ok();
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::create_dir(&path).unwrap();
    let sentinel = path.join("saved-bytes");
    std::fs::write(&sentinel, b"original bytes").unwrap();
    for name in ["settings-analyze-save", "settings-analyze-reset"] {
        click(cx, window, name);
        if name == "settings-analyze-reset" {
            click(cx, window, "settings-analyze-reset-confirm");
        }
        let error = cx
            .read(|cx| {
                app.read(cx)
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .last()
                    .cloned()
            })
            .unwrap();
        assert_eq!(
            error.kind,
            ToastKind::Error,
            "failed Analyze write shows an error toast"
        );
        assert!(error
            .message
            .starts_with(Msg::SettingsAnalyzeIgnoreFailed.t()));
        assert!(error.message.contains("analyze_ignore"));
        assert!(error.message.contains("directory") || error.message.contains("Directory"));
        assert!(error.message.chars().count() <= 241);
        assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
        assert!(e2e::control_bounds(window.window_id(), "settings-analyze-dirty").is_some());
        assert_eq!(
            cx.read(|cx| input.read(cx).value().to_string()),
            "draft-ignore\n",
            "failed Reset preserves draft"
        );
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"original bytes");
        dismiss_feedback(cx, app, window);
    }
    std::fs::remove_file(&sentinel).unwrap();
    std::fs::remove_dir(&path).unwrap();
    unwritable_directory(cx, app, window, &path);
    click(cx, window, "settings-analyze-save");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "draft-ignore\n");
    assert!(!cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
    assert_eq!(
        cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .last()
            .unwrap()
            .kind),
        ToastKind::Info
    );
    dismiss_feedback(cx, app, window);
    click(cx, window, "settings-analyze-reset");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        settings::DEFAULT_ANALYZE_IGNORE
    );
    assert_eq!(
        cx.read(|cx| input.read(cx).value().to_string()),
        settings::DEFAULT_ANALYZE_IGNORE
    );
    if let Some(bytes) = original {
        std::fs::write(&path, bytes).unwrap();
    } else {
        std::fs::remove_file(&path).unwrap();
    }
    drop(input);
    dismissal_keeps_settings_commands(cx, app, window);
}

pub fn scenario_settings_analyze_ignore_feedback(cx: &mut VisualTestAppContext) {
    matrix(cx, "3", analyze_leg);
    eprintln!("[gui-e2e] PASS settings_analyze_ignore_feedback EN/JA Light/Dark 100/167% Save/Reset failure and recovery");
}
