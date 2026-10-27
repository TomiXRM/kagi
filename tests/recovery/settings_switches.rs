//! Production Settings switches: focus is not ON, keyboard/pointer events persist,
//! and each pane's controls remain reachable in its single detail scroller.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};
use crate::settings_controls::{bounds, click, draw, probe};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::i18n::Msg;
use kagi::ui::{settings, KagiApp};

const SWITCHES: [(&str, &str, Option<&str>, Msg); 6] = [
    (
        "graph",
        "compact-toggle",
        Some("graph_compact"),
        Msg::SettingsCompact,
    ),
    (
        "graph",
        "lane-compact-toggle",
        Some("graph_lane_compact"),
        Msg::SettingsLaneCompact,
    ),
    (
        "network",
        "auto-fetch-toggle",
        Some("auto_fetch"),
        Msg::SettingsAutoFetch,
    ),
    (
        "appearance",
        "reduce-motion-toggle",
        Some("reduce_motion"),
        Msg::SettingsReduceMotion,
    ),
    (
        "terminal",
        "terminal-auto-lock-toggle",
        Some("terminal_auto_lock"),
        Msg::SettingsTerminalAutoLock,
    ),
    (
        "smart_commit",
        "sc-enabled-toggle",
        None,
        Msg::SettingsSmartEnable,
    ),
];

fn focus(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle, id: &str) {
    cx.update_window(window, |_, window, cx| {
        let view = app.read(cx).settings_entity().unwrap();
        view.update(cx, |view, cx| assert!(view.focus_control(id, window, cx)));
    })
    .unwrap();
    draw(cx, window);
}

fn on_off(on: bool) -> Option<String> {
    Some(if on { "true" } else { "false" }.to_string())
}

pub fn scenario_settings_switches(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "graph_compact",
        "graph_lane_compact",
        "auto_fetch",
        "reduce_motion",
        "terminal_auto_lock",
        "settings_pane",
        "smart_commit_llm_enabled",
        "smart_commit_consent_accepted",
    ]);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);

    for pane in ["appearance", "graph", "terminal", "network", "smart_commit"] {
        click(cx, window, &format!("settings-pane-{pane}"));
        let expected: Vec<_> = SWITCHES
            .iter()
            .filter(|entry| entry.0 == pane)
            .map(|entry| entry.1.to_string())
            .collect();
        let order = cx
            .update_window(window, |_, window, cx| {
                let view = app.read(cx).settings_entity().unwrap();
                view.update(cx, |view, cx| {
                    assert!(view.focus_control(&format!("settings-pane-{pane}"), window, cx));
                });
                let mut seen = Vec::new();
                for _ in 0..80 {
                    window.focus_next(cx);
                    window.draw(cx).clear();
                    if let Some(control) = view
                        .read(cx)
                        .control_probes()
                        .into_iter()
                        .find(|control| control.role == gpui::Role::Switch && control.focused)
                    {
                        if seen.contains(&control.id) {
                            break;
                        }
                        seen.push(control.id);
                    }
                }
                seen
            })
            .unwrap();
        assert_eq!(
            order, expected,
            "Tab visits this pane's switches once in drawing order"
        );
        for (_, id, key, title) in SWITCHES.iter().filter(|entry| entry.0 == pane) {
            let start = probe(cx, window, id).expect("production switch");
            assert_eq!(start.role, gpui::Role::Switch);
            assert_eq!(start.name, title.t());
            let before = key.and_then(settings::read_setting);
            let smart_enabled = settings::read_setting("smart_commit_llm_enabled");
            let consent = settings::read_setting("smart_commit_consent_accepted");
            focus(cx, &app, window, id);
            let focused = probe(cx, window, id).unwrap();
            assert!(focused.focused);
            assert_eq!(focused.selected, start.selected, "focus cannot flip ON");
            assert_eq!(key.and_then(settings::read_setting), before);
            let Some(key) = key else {
                assert_eq!(
                    settings::read_setting("smart_commit_llm_enabled"),
                    smart_enabled
                );
                assert_eq!(
                    settings::read_setting("smart_commit_consent_accepted"),
                    consent
                );
                continue;
            };
            keys(cx, window, "space");
            draw(cx, window);
            assert_eq!(
                settings::read_setting(key),
                on_off(!start.selected),
                "Space saves {key}"
            );
            assert_eq!(probe(cx, window, id).unwrap().selected, !start.selected);
            keys(cx, window, "enter");
            draw(cx, window);
            assert_eq!(
                settings::read_setting(key),
                on_off(start.selected),
                "Enter saves {key}"
            );
            assert_eq!(probe(cx, window, id).unwrap().selected, start.selected);
            click(cx, window, id);
            assert_eq!(
                settings::read_setting(key),
                on_off(!start.selected),
                "one click, one flip"
            );
            assert_eq!(probe(cx, window, id).unwrap().selected, !start.selected);
            click(cx, window, id);
            assert_eq!(settings::read_setting(key), on_off(start.selected));
            assert_eq!(probe(cx, window, id).unwrap().selected, start.selected);
        }
    }
    keys(cx, window, "escape");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_switches");
}

/// Migrated 150% retains its physical size and appears as 167%; real stepper
/// events use presets and cannot persist a value outside the allowed range.
pub fn scenario_settings_zoom_bound(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    settings::write_setting("settings_pane", Some("appearance"));
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    for (zoom, label, saved) in [(1.556, "156%", "1556"), (1.667, "167%", "1667")] {
        assert_eq!(kagi_ui_core::theme::set_zoom(zoom), zoom);
        app.update(cx, |app, cx| app.seed_settings(cx));
        draw(cx, window);
        assert_eq!(probe(cx, window, "zoom-value").unwrap().value, label);
        assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some(saved));
    }
    click(cx, window, "zoom-inc");
    assert_eq!(probe(cx, window, "zoom-value").unwrap().value, "170%");
    assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some("1700"));
    for _ in 0..3 {
        click(cx, window, "zoom-inc");
    }
    assert_eq!(probe(cx, window, "zoom-value").unwrap().value, "200%");
    assert!(probe(cx, window, "zoom-inc").unwrap().disabled);
    click(cx, window, "zoom-inc");
    assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some("2000"));
    click(cx, window, "zoom-dec");
    assert_eq!(probe(cx, window, "zoom-value").unwrap().value, "190%");
    assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some("1900"));
    keys(cx, window, "escape");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_zoom_bound legacy 167%, 170% preset, 200% upper bound and 190% lower preset");
}

/// Eight real panes replace the removed single-page group list. Every measured
/// control/description is horizontally contained and reachable by the same
/// detail scroller; the sidebar stays still while overflowing content scrolls.
pub fn scenario_settings_groups(cx: &mut VisualTestAppContext) {
    use gpui::{point, px, size};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["settings_pane"]);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, w, cx| {
        w.resize(size(px(1392.), px(883.)));
        app.update(cx, |app, cx| app.handle_menu_command("app.settings", w, cx));
        app.update(cx, |app, cx| app.set_theme("apple-light", cx));
    })
    .unwrap();
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        for zoom in [1., 1.67] {
            kagi::ui::theme::set_zoom(zoom);
            app.update(cx, |app, cx| app.seed_settings(cx));
            for pane in kagi_ui_settings::Pane::ALL {
                click(cx, window, &format!("settings-pane-{}", pane.id()));
                let view = cx.read(|cx| app.read(cx).settings_entity().unwrap());
                let scroll = cx.read(|cx| view.read(cx).scroll_handle());
                let viewport = bounds(cx, window, "settings-detail-scroll").unwrap();
                assert_eq!(
                    viewport,
                    scroll.bounds(),
                    "the probe measures the actual GPUI scroll-owner viewport"
                );
                let sidebar = bounds(cx, window, "settings-sidebar").unwrap();
                let probes = cx.read(|cx| view.read(cx).control_probes());
                let controls: Vec<_> = probes
                    .into_iter()
                    .filter(|control| {
                        !control.id.starts_with("settings-pane-")
                            && ![
                                "settings-sidebar",
                                "settings-detail-scroll",
                                "settings-detail-content",
                                "settings-detail-viewport",
                                "settings-pane-title",
                            ]
                            .contains(&control.id.as_str())
                    })
                    .collect();
                assert!(!controls.is_empty(), "{} has production content", pane.id());
                let required: &[&str] = match pane {
                    kagi_ui_settings::Pane::Appearance => &[
                        "settings-interface-group",
                        "settings-theme-folder-group",
                        "settings-theme-folder-open",
                        "settings-theme-reload",
                        "settings-theme-guide",
                        "zoom-value",
                        "compact-threshold-value",
                        "reduce-motion-toggle",
                    ],
                    kagi_ui_settings::Pane::Graph => &[
                        "settings-graph-group",
                        "settings-copy-group",
                        "compact-toggle",
                        "lane-compact-toggle",
                        "settings-copy-hash",
                        "settings-copy-branch",
                    ],
                    kagi_ui_settings::Pane::Editor => &[
                        "settings-editor-group",
                        "settings-markdown-preview",
                        "settings-markdown-source",
                    ],
                    kagi_ui_settings::Pane::Terminal => {
                        &["settings-terminal-group", "terminal-auto-lock-toggle"]
                    }
                    kagi_ui_settings::Pane::Network => {
                        &["settings-network-group", "auto-fetch-toggle"]
                    }
                    kagi_ui_settings::Pane::SmartCommit => &[
                        "settings-smart-group",
                        "sc-enabled-toggle",
                        "sc-provider-ollama",
                        "sc-provider-claude-code",
                        "sc-provider-codex",
                    ],
                    kagi_ui_settings::Pane::Analyze => &[
                        "settings-analyze-input",
                        "settings-analyze-status",
                        "settings-analyze-reset",
                        "settings-analyze-save",
                    ],
                    kagi_ui_settings::Pane::General => &[
                        "settings-language-group",
                        "settings-language-en",
                        "settings-language-ja",
                    ],
                };
                for id in required {
                    assert!(
                        controls.iter().any(|control| control.id == *id),
                        "{id} retained in {}",
                        pane.id()
                    );
                }
                let groups: Vec<_> = controls
                    .iter()
                    .filter(|control| control.id.ends_with("-group"))
                    .collect();
                for (index, group) in groups.iter().enumerate() {
                    for other in groups.iter().skip(index + 1) {
                        assert!(
                            !group.bounds.contains(&other.bounds.center())
                                && !other.bounds.contains(&group.bounds.center()),
                            "row groups must not be nested"
                        );
                    }
                }
                for control in controls {
                    let rect = bounds(cx, window, &control.id).unwrap();
                    assert!(
                        rect.left() >= viewport.left() - px(1.),
                        "{} left",
                        control.id
                    );
                    assert!(
                        rect.right() <= viewport.right() + px(1.),
                        "{} right",
                        control.id
                    );
                    scroll.set_offset(point(
                        px(0.),
                        scroll.offset().y - rect.top() + viewport.top(),
                    ));
                    draw(cx, window);
                    let reached = bounds(cx, window, &control.id).unwrap();
                    assert!(
                        reached.top() < viewport.bottom() && reached.bottom() > viewport.top(),
                        "{} reachable in {}",
                        control.id,
                        pane.id()
                    );
                    if control.id.ends_with("-group") {
                        let at = point(reached.left() + px(3.), reached.top() + px(16.));
                        assert!(viewport.contains(&at), "group fill sample is visible");
                        cx.update_window(window, |_, w, _| {
                            let image = w.render_to_image().expect("real group fill");
                            let pixel = image
                                .get_pixel(
                                    (f32::from(at.x) * w.scale_factor()) as u32,
                                    (f32::from(at.y) * w.scale_factor()) as u32,
                                )
                                .0;
                            let color = kagi::ui::theme::theme().bg_base;
                            for (got, expected) in pixel[..3].iter().zip([
                                (color >> 16) as u8,
                                (color >> 8) as u8,
                                color as u8,
                            ]) {
                                assert!(
                                    got.abs_diff(expected) <= 2,
                                    "group uses the production base surface"
                                );
                            }
                        })
                        .unwrap();
                    }
                    if reached.size.height > viewport.size.height {
                        scroll.set_offset(point(
                            px(0.),
                            scroll.offset().y - reached.bottom() + viewport.bottom(),
                        ));
                        draw(cx, window);
                        let tail = bounds(cx, window, &control.id).unwrap();
                        assert!(
                            tail.bottom() <= viewport.bottom() + px(1.)
                                && tail.bottom() > viewport.top(),
                            "{} oversized content tail reachable",
                            control.id
                        );
                    }
                    assert_eq!(
                        bounds(cx, window, "settings-sidebar").unwrap(),
                        sidebar,
                        "detail scrolling never moves navigation"
                    );
                    assert_eq!(
                        bounds(cx, window, "settings-detail-scroll").unwrap(),
                        viewport,
                        "content translation never moves the fixed viewport probe"
                    );
                    assert_eq!(scroll.bounds(), viewport);
                }
                if pane == kagi_ui_settings::Pane::Appearance {
                    let before = scroll.offset().y;
                    let delta = if before < px(-10.) { 100. } else { -100. };
                    cx.simulate_event(
                        window,
                        gpui::ScrollWheelEvent {
                            position: viewport.center(),
                            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
                            touch_phase: gpui::TouchPhase::Moved,
                            ..Default::default()
                        },
                    );
                    draw(cx, window);
                    assert!(
                        (scroll.offset().y - before).abs() > px(1.),
                        "wheel reaches the detail owner, not a nested group scroller"
                    );
                }
            }
        }
    }
    keys(cx, window, "escape");
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_groups EN/JA 100%/167% eight-pane reachability");
}
