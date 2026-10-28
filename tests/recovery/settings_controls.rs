//! #1298 / #1299: real macOS Settings controls, not an offscreen overlay click.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, drain_native_events, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    KagiApp, ToastKind, e2e,
    i18n::{self, Msg},
    settings,
    settings_view::SettingsHostForE2e,
    smart_commit::SmartProvider,
    theme,
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

pub(crate) fn probe(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> Option<kagi_ui_settings::ControlProbe> {
    cx.update_window(window, |_, window, cx| {
        kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx)
            .and_then(|view| view.read(cx).control_probe(id))
    })
    .unwrap()
}

pub(crate) fn bounds(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    probe(cx, window, id)
        .map(|probe| probe.bounds)
        .or_else(|| e2e::control_bounds(window.window_id(), id))
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

fn open_popup(cx: &mut VisualTestAppContext, window: AnyWindowHandle, models: bool) {
    click(
        cx,
        window,
        if models {
            "sc-model-select"
        } else {
            "sc-provider-select"
        },
    );
}

fn assert_popup_radio_tree(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    visible: &kagi_ui_settings::ControlProbe,
) {
    let tree = crate::native_ax::tree(cx, window);
    let mut matches = tree["nodes"].as_object().unwrap().values().filter(|node| {
        node["aria"]["role"] == "MenuItemRadio" && node["aria"]["label"] == visible.name
    });
    let node = matches.next().unwrap_or_else(|| {
        panic!(
            "missing actual named popup radio root {}: {tree}",
            visible.name
        )
    });
    assert!(
        matches.next().is_none(),
        "one semantic root per visible menu item"
    );
    assert_eq!(
        node["aria"]["toggled"],
        if visible.selected { "True" } else { "False" }
    );
}

fn provider_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    crate::native_ax::activate(cx);
    draw(cx, window);
    for count in [0, 1, 20] {
        app.update(cx, |app, cx| {
            app.smart_commit.provider = SmartProvider::Ollama;
            app.smart_commit.ollama_available = true;
            app.smart_commit.detecting = false;
            app.smart_commit.claude_available = count > 0;
            app.smart_commit.codex_available = false;
            app.smart_commit.detected_models = (0..count)
                .map(|n| format!("model-{n}-long-local-model-name-0123456789"))
                .collect();
            app.smart_commit.model = app.smart_commit.detected_models.first().cloned();
            app.seed_settings(cx);
        });
        draw(cx, window);
        let saved_provider = settings::read_setting("smart_commit_provider");
        open_popup(cx, window, false);
        assert_eq!(
            settings::read_setting("smart_commit_provider"),
            saved_provider
        );
        let options = ["ollama", "claude-code", "codex"].map(|slug| {
            probe(cx, window, &format!("sc-provider-{slug}")).expect("actual open menu option")
        });
        assert_eq!(
            options.len(),
            3,
            "all providers remain discoverable in the real menu"
        );
        assert_eq!(options.iter().filter(|option| option.selected).count(), 1);
        assert!(
            options
                .iter()
                .all(|option| option.role == gpui::Role::MenuItemRadio && !option.name.is_empty())
        );
        assert_eq!(options[2].name, "Codex");
        assert!(options[2].disabled && !options[2].selected);
        for option in &options {
            assert_popup_radio_tree(cx, window, option);
        }
        assert!(
            probe(cx, window, "settings-provider-availability")
                .unwrap()
                .value
                .contains(&i18n::provider_not_found_hint("Codex"))
        );
        click(cx, window, "sc-provider-codex");
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.provider),
            SmartProvider::Ollama
        );
        assert_eq!(
            settings::read_setting("smart_commit_provider"),
            saved_provider
        );
        keys(cx, window, "escape");
        draw(cx, window);
        assert!(probe(cx, window, "sc-provider-select").unwrap().focused);
        // PopupMenu owns keyboard focus; disabled rows are skipped, not fake
        // independently focusable radios. Down starts at the first menu item.
        keys(cx, window, "enter down down");
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.provider),
            SmartProvider::Ollama,
            "menu navigation does not select a provider",
        );
        draw(cx, window);
        let tree = crate::native_ax::tree(cx, window);
        let active = tree["active_descendant_focus"]
            .as_str()
            .unwrap_or_else(|| panic!("owning popup focus must name its active row: {tree}"));
        let active = &tree["nodes"][active]["aria"];
        assert_eq!(active["role"], "MenuItemRadio");
        assert_eq!(active["label"], options[usize::from(count > 0)].name);
        assert_eq!(
            active["toggled"],
            if count > 0 { "False" } else { "True" },
            "keyboard focus does not change the checked preference"
        );
        keys(cx, window, "enter");
        draw(cx, window);
        assert_eq!(
            cx.read(|cx| app.read(cx).smart_commit.provider),
            if count > 0 {
                SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode)
            } else {
                SmartProvider::Ollama
            },
            "keyboard skips unavailable choices and commits only an enabled provider",
        );
        assert_ne!(
            settings::read_setting("smart_commit_provider").as_deref(),
            Some("codex")
        );
        open_popup(cx, window, false);
        click(cx, window, "sc-provider-ollama");
        if count > 0 {
            open_popup(cx, window, true);
            // Up from the unopened selection traverses to the actual last item,
            // replacing the segmented bank's Home/End shortcut contract.
            keys(cx, window, "up enter");
            draw(cx, window);
            assert_eq!(
                settings::read_setting("smart_commit_model"),
                cx.read(|cx| app.read(cx).smart_commit.detected_models.last().cloned()),
            );
            open_popup(cx, window, true);
            let selected = cx.read(|cx| {
                app.read(cx)
                    .settings_entity()
                    .unwrap()
                    .read(cx)
                    .control_probes()
                    .iter()
                    .filter(|probe| {
                        probe.id.starts_with("sc-model-")
                            && probe.id != "sc-model-select"
                            && probe.selected
                    })
                    .count()
            });
            assert_eq!(selected, 1, "one actual checked model menu item");
            let width = cx
                .update_window(window, |_, w, _| w.viewport_size().width)
                .unwrap();
            for model in cx.read(|cx| app.read(cx).smart_commit.detected_models.clone()) {
                let rect = reveal(cx, window, &format!("sc-model-{model}"));
                assert!(
                    rect.left() >= gpui::px(0.) && rect.right() <= width,
                    "long model fits Settings"
                );
                let option = probe(cx, window, &format!("sc-model-{model}")).unwrap();
                assert_popup_radio_tree(cx, window, &option);
            }
            let selected_model = cx
                .read(|cx| app.read(cx).smart_commit.model.clone())
                .unwrap();
            let persisted_model = settings::read_setting("smart_commit_model");
            app.update(cx, |app, cx| {
                app.smart_commit.provider =
                    SmartProvider::Cli(kagi_git::message_gen::CliProvider::ClaudeCode);
                app.seed_settings(cx);
            });
            draw(cx, window);
            assert!(
                probe(cx, window, "sc-provider-select").unwrap().focused,
                "an obsolete open model menu repairs focus to the effective provider trigger"
            );
            assert!(probe(cx, window, "sc-model-select").is_none());
            assert!(probe(cx, window, &format!("sc-model-{selected_model}")).is_none());
            assert_eq!(
                settings::read_setting("smart_commit_model"),
                persisted_model,
                "focus repair cannot persist a different model"
            );
            app.update(cx, |app, cx| {
                app.smart_commit.provider = SmartProvider::Ollama;
                app.smart_commit.detected_models.clear();
                app.seed_settings(cx);
            });
            draw(cx, window);
            let unavailable = probe(cx, window, "sc-model-select").unwrap();
            assert!(
                unavailable.disabled && unavailable.value.contains(&selected_model),
                "the disabled opener keeps the saved unavailable model readable"
            );
            let actual = crate::native_ax::node(cx, window, "sc-model-select".into());
            assert_eq!(actual["aria"]["role"], "ComboBox");
            assert!(
                actual["aria"]["value"]
                    .as_str()
                    .unwrap()
                    .contains(&selected_model)
            );
            click(cx, window, "sc-model-select");
            keys(cx, window, "tab");
            draw(cx, window);
            assert!(
                !probe(cx, window, "sc-model-select").unwrap().focused,
                "the disabled opener is not a keyboard tab stop"
            );
            assert!(
                probe(cx, window, "settings-popup-menu").is_none(),
                "a disabled popup opener stays inert"
            );
            assert_eq!(
                settings::read_setting("smart_commit_model"),
                persisted_model
            );
        }
        assert!(
            cx.read(|cx| app.read(cx).smart_commit_modal().is_none()),
            "selection does not start generation"
        );
        let rect = bounds(cx, window, "sc-provider-select").unwrap();
        let width = cx
            .update_window(window, |_, w, _| w.viewport_size().width)
            .unwrap();
        assert!(rect.left() >= gpui::px(0.) && rect.right() <= width);
    }
    popup_reopens_in_new_native_host(cx, window);
}

fn popup_reopens_in_new_native_host(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let repo = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|candidate| candidate.window_id() != window.window_id())
            .expect("owning repository remains open")
    });
    let view_id = cx
        .update_window(window, |_, window, cx| {
            kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx)
                .unwrap()
                .entity_id()
        })
        .unwrap();
    let before = (
        settings::read_setting("smart_commit_provider"),
        settings::read_setting("smart_commit_model"),
    );
    open_popup(cx, window, false);
    draw(cx, window);
    assert!(probe(cx, window, "settings-popup-menu").is_some());
    cx.simulate_keystrokes(window, "cmd-w");
    drain_native_events();
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| cx
            .windows()
            .iter()
            .map(|window| window.window_id())
            .collect::<Vec<_>>()),
        vec![repo.window_id()],
        "Cmd-W closes the Settings owner even while its real PopupMenu has focus"
    );
    keys(cx, repo, "cmd-,");
    cx.run_until_parked();
    let reopened = cx.read(|cx| {
        cx.windows()
            .into_iter()
            .find(|candidate| candidate.window_id() != repo.window_id())
            .expect("new native Settings host")
    });
    assert_ne!(window.window_id(), reopened.window_id());
    draw(cx, reopened);
    let restored_view_id = cx
        .update_window(reopened, |_, window, cx| {
            kagi::ui::settings_view::settings_view_for_window_for_e2e(window, cx)
                .unwrap()
                .entity_id()
        })
        .unwrap();
    assert_eq!(
        view_id, restored_view_id,
        "the production owner retains its shared view"
    );
    assert!(
        probe(cx, reopened, "settings-popup-menu").is_none(),
        "a new host must not paint the old popup or retain its window anchor"
    );
    assert!(
        probe(cx, reopened, "settings-pane-smart_commit")
            .unwrap()
            .selected
    );
    open_popup(cx, reopened, false);
    draw(cx, reopened);
    let option =
        probe(cx, reopened, "sc-provider-ollama").expect("fresh real menu in the new host");
    assert_popup_radio_tree(cx, reopened, &option);
    keys(cx, reopened, "escape");
    draw(cx, reopened);
    assert!(probe(cx, reopened, "settings-popup-menu").is_none());
    assert!(probe(cx, reopened, "sc-provider-select").unwrap().focused);
    assert_eq!(
        before,
        (
            settings::read_setting("smart_commit_provider"),
            settings::read_setting("smart_commit_model"),
        ),
        "opening, closing and reopening a menu never changes stored choices"
    );
    cx.simulate_keystrokes(reopened, "cmd-w");
    drain_native_events();
    cx.run_until_parked();
}

pub fn scenario_settings_provider_keys(cx: &mut VisualTestAppContext) {
    matrix(cx, "smart_commit", provider_leg);
    eprintln!("[gui-e2e] PASS settings_provider_keys EN/JA Light/Dark 100/167%, 0/1/20 models");
}

pub(crate) fn reveal(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    name: &str,
) -> gpui::Bounds<gpui::Pixels> {
    draw(cx, window);
    let mut bounds = self::bounds(cx, window, name).unwrap();
    let viewport = if name.starts_with("settings-pane-") {
        self::bounds(cx, window, "settings-sidebar").expect("production sidebar viewport")
    } else if probe(cx, window, name).is_none() {
        cx.update_window(window, |_, w, _| {
            gpui::Bounds::new(gpui::point(gpui::px(0.), gpui::px(0.)), w.viewport_size())
        })
        .unwrap()
    } else {
        let viewport_id =
            if probe(cx, window, name).is_some_and(|item| item.role == gpui::Role::MenuItemRadio) {
                "settings-popup-menu"
            } else {
                "settings-detail-scroll"
            };
        self::bounds(cx, window, viewport_id).expect("production scroll viewport")
    };
    for _ in 0..20 {
        if bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom() {
            break;
        }
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    gpui::px(0.),
                    if bounds.top() < viewport.top() {
                        viewport.top() - bounds.top()
                    } else {
                        viewport.bottom() - bounds.bottom()
                    },
                )),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        draw(cx, window);
        bounds = self::bounds(cx, window, name).unwrap();
    }
    assert!(
        bounds.top() >= viewport.top() && bounds.bottom() <= viewport.bottom(),
        "{name} is visible: {bounds:?} in {viewport:?}, {:?}/{}",
        i18n::lang(),
        theme::zoom()
    );
    if probe(cx, window, name).is_some_and(|probe| {
        probe.role == gpui::Role::RadioButton && name.starts_with("settings-theme-")
    }) {
        let palette = theme::theme_by_slug(name.strip_prefix("settings-theme-").unwrap())
            .expect("theme card has an authoritative palette");
        let strip_id = if palette.dark {
            "settings-themes-dark-scroll"
        } else {
            "settings-themes-light-scroll"
        };
        let strip = self::bounds(cx, window, strip_id).unwrap();
        let margin = gpui::px(7.);
        for _ in 0..20 {
            let delta = if bounds.left() < strip.left() + margin {
                strip.left() + margin - bounds.left()
            } else if bounds.right() > strip.right() - margin {
                strip.right() - margin - bounds.right()
            } else {
                break;
            };
            cx.simulate_event(
                window,
                gpui::ScrollWheelEvent {
                    position: gpui::point(strip.center().x, bounds.center().y),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(delta, gpui::px(0.))),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                },
            );
            draw(cx, window);
            bounds = self::bounds(cx, window, name).unwrap();
        }
        assert!(
            bounds.left() >= strip.left() + margin && bounds.right() <= strip.right() - margin,
            "{name} and its focus clearance are horizontally reachable: {bounds:?} in {strip:?}"
        );
    }
    bounds
}

pub(crate) fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, name: &str) {
    let bounds = reveal(cx, window, name);
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
    // The sidebar uses manual selection: arrows move focus, Enter selects.
    click(cx, window, "settings-pane-analyze");
    keys(cx, window, "up");
    assert_eq!(
        settings::read_setting("settings_pane").as_deref(),
        Some("analyze")
    );
    keys(cx, window, "enter");
    assert_eq!(
        settings::read_setting("settings_pane").as_deref(),
        Some("smart_commit")
    );
    keys(cx, window, "down enter");
    assert_eq!(
        settings::read_setting("settings_pane").as_deref(),
        Some("analyze")
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
    let view = cx.read(|cx| app.read(cx).settings_entity().unwrap());
    let baseline = cx.read(|cx| view.read(cx).analyze_baseline().to_string());
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
        assert_eq!(
            cx.read(|cx| view.read(cx).analyze_baseline().to_string()),
            baseline
        );
        assert_eq!(
            cx.read(|cx| view.read(cx).analyze_request(cx).text.to_string()),
            "draft-ignore\n"
        );
        assert!(
            probe(cx, window, "settings-analyze-error")
                .unwrap()
                .value
                .contains("Permission denied")
        );
        drop(restore);
        assert_eq!(std::fs::read(path).unwrap(), b"saved bytes");
        dismiss_feedback(cx, app, window);
    }
}

fn analyze_leg(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let input = cx.read(|cx| app.read(cx).analyze_ignore_input.clone().unwrap());
    let view = cx.read(|cx| app.read(cx).settings_entity().unwrap());
    let baseline = cx.read(|cx| view.read(cx).analyze_baseline().to_string());
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
        assert!(
            error
                .message
                .starts_with(Msg::SettingsAnalyzeIgnoreFailed.t())
        );
        assert!(error.message.contains("analyze_ignore"));
        assert!(error.message.contains("directory") || error.message.contains("Directory"));
        assert!(error.message.chars().count() <= 241);
        assert!(cx.read(|cx| app.read(cx).analyze_ignore_dirty_for_e2e(cx)));
        assert_eq!(
            cx.read(|cx| view.read(cx).analyze_baseline().to_string()),
            baseline,
            "failed Save and Reset cannot acknowledge a new baseline"
        );
        assert_eq!(
            probe(cx, window, "settings-analyze-status").unwrap().value,
            Msg::SettingsAnalyzeIgnoreUnsaved.t()
        );
        assert!(
            probe(cx, window, "settings-analyze-error")
                .unwrap()
                .value
                .contains("analyze_ignore"),
            "the authoritative OS failure is visible inline as well as in feedback"
        );
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
    matrix(cx, "analyze", analyze_leg);
    eprintln!(
        "[gui-e2e] PASS settings_analyze_ignore_feedback EN/JA Light/Dark 100/167% Save/Reset failure and recovery"
    );
}
