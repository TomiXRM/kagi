//! Settings' switches from the keyboard and to assistive technology (#970),
//! Tier A: each is a Tab stop, in drawing order; Space and Enter flip it and
//! save the setting; the checked state given to assistive technology
//! follows; the pointer still flips it after real wheel input reveals it.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::i18n::Msg;
use kagi::ui::{e2e, settings, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};

/// Each switch: its id, the setting it saves (None: flipping it starts work
/// Tier A must not do, so it is only reached), and its row's title.
const SWITCHES: [(&str, Option<&str>, Msg); 6] = [
    (
        "compact-toggle",
        Some("graph_compact"),
        Msg::SettingsCompact,
    ),
    (
        "lane-compact-toggle",
        Some("graph_lane_compact"),
        Msg::SettingsLaneCompact,
    ),
    (
        "auto-fetch-toggle",
        Some("auto_fetch"),
        Msg::SettingsAutoFetch,
    ),
    (
        "reduce-motion-toggle",
        Some("reduce_motion"),
        Msg::SettingsReduceMotion,
    ),
    (
        "terminal-auto-lock-toggle",
        Some("terminal_auto_lock"),
        Msg::SettingsTerminalAutoLock,
    ),
    // Turning Smart Commit on probes for local LLMs.
    ("smart-commit-enabled", None, Msg::SettingsSmartEnable),
];

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
}

/// The checked state switch `id` last gave assistive technology.
fn toggled(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> bool {
    draw(cx, window);
    e2e::recorded_switch(id)
        .unwrap_or_else(|| panic!("{id} is drawn"))
        .1
}

fn saved(key: &str) -> Option<String> {
    settings::read_setting(key)
}

fn on_off(on: bool) -> Option<String> {
    Some(if on { "true" } else { "false" }.to_string())
}

fn click_switch(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    draw(cx, window);
    let body = e2e::control_bounds(window.window_id(), "settings-scrollbar")
        .expect("the Settings scroll viewport");
    let measurement = format!("{id}-key");
    let mut bounds =
        e2e::control_bounds(window.window_id(), &measurement).expect("the switch is measured");
    // Keyboard focus may have revealed a later switch. Layout bounds alone
    // do not mean this pointer target is inside the scroll owner's clip.
    for _ in 0..20 {
        if bounds.top() >= body.top() && bounds.bottom() <= body.bottom() {
            break;
        }
        cx.simulate_event(
            window,
            gpui::ScrollWheelEvent {
                position: body.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(
                    gpui::px(0.),
                    gpui::px(if bounds.top() < body.top() {
                        300.
                    } else {
                        -300.
                    }),
                )),
                touch_phase: gpui::TouchPhase::Moved,
                ..Default::default()
            },
        );
        cx.run_until_parked();
        draw(cx, window);
        bounds = e2e::control_bounds(window.window_id(), &measurement).unwrap();
    }
    crate::recovery_layout::contained(body, bounds, "pointer switch is inside Settings");
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

pub fn scenario_settings_switches(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "graph_compact",
        "graph_lane_compact",
        "auto_fetch",
        "reduce_motion",
        "terminal_auto_lock",
    ]);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();
    draw(cx, window);

    // Named for assistive technology by their row's title.
    for (id, _, title) in SWITCHES {
        let (label, _) = e2e::recorded_switch(id).unwrap_or_else(|| panic!("{id} drawn"));
        assert_eq!(label, title.t(), "{id} is named by its row");
    }

    // Tab, from the window, reaches the six switches in drawing order, one
    // stop each (other controls in between are passed over here).
    let order = cx
        .update_window(window, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().expect("root focus");
            root.focus(window, cx);
            let mut seen: Vec<usize> = Vec::new();
            for _ in 0..80 {
                window.focus_next(cx);
                window.draw(cx).clear();
                if let Some(slot) = app.read(cx).settings_switch_focused_for_e2e(window) {
                    if seen.last() != Some(&slot) {
                        if seen.contains(&slot) {
                            break;
                        }
                        seen.push(slot);
                    }
                }
            }
            seen
        })
        .unwrap();
    assert_eq!(
        order,
        vec![0, 1, 2, 3, 4, 5],
        "Tab walks the switches in order"
    );

    // Space flips each switch, Enter flips it back: the setting is saved
    // each time, and the state given to assistive technology follows.
    for (slot, (id, key, _)) in SWITCHES.into_iter().enumerate() {
        let Some(key) = key else { continue };
        let start = toggled(cx, window, id);
        focus(cx, &app, window, slot);
        keys(cx, window, "space");
        assert_eq!(saved(key), on_off(!start), "Space saves {key}");
        assert_eq!(
            toggled(cx, window, id),
            !start,
            "{id} reports the new state"
        );
        keys(cx, window, "enter");
        assert_eq!(saved(key), on_off(start), "Enter saves {key} back");
        assert_eq!(toggled(cx, window, id), start, "{id} reports it again");
    }

    // The pointer still flips a switch (the Switch's own handling under the
    // keyboard wrapper), and a second click flips it back.
    let (id, key) = ("compact-toggle", "graph_compact");
    let start = toggled(cx, window, id);
    click_switch(cx, window, id);
    assert_eq!(saved(key), on_off(!start), "one click, one flip");
    assert_eq!(toggled(cx, window, id), !start);
    click_switch(cx, window, id);
    assert_eq!(saved(key), on_off(start));

    // The Switch's thumb springs to each new state; let those animations end
    // on the dispatcher's clock before the window goes, so nothing they hold
    // outlives it.
    for _ in 0..10 {
        cx.advance_clock(std::time::Duration::from_millis(100));
        draw(cx, window);
        cx.run_until_parked();
    }
    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_switches");
}

/// Migrated 150% must keep its physical size and appear as 167% in the real
/// Settings renderer. Legacy 167% steps to 170%; the upper bound is 200%.
pub fn scenario_settings_zoom_bound(cx: &mut VisualTestAppContext) {
    let restore = crate::recovery_layout::GlobalSettings::capture();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.settings", window, cx)
        })
    })
    .unwrap();

    let check = |cx: &mut VisualTestAppContext, zoom: f32, label: &str, saved: &str| {
        assert_eq!(kagi_ui_core::theme::set_zoom(zoom), zoom);
        draw(cx, window);
        assert_eq!(
            e2e::settings_zoom_label().as_deref(),
            Some(label),
            "Settings must show the active migrated zoom"
        );
        assert_eq!(settings::read_setting("ui_zoom").as_deref(), Some(saved));
    };
    check(cx, 1.556, "156%", "1556");
    check(cx, 1.667, "167%", "1667");
    let next = kagi_ui_core::theme::step_zoom(kagi_ui_core::theme::zoom(), true);
    check(cx, next, "170%", "1700");
    check(cx, 2., "200%", "2000");
    let capped = kagi_ui_core::theme::step_zoom(kagi_ui_core::theme::zoom(), true);
    check(cx, capped, "200%", "2000");
    let lower = kagi_ui_core::theme::step_zoom(kagi_ui_core::theme::zoom(), false);
    check(cx, lower, "190%", "1900");

    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
    unmount(cx, app, window);
    drop(restore);
    eprintln!(
        "[gui-e2e] PASS settings_zoom_bound legacy 167%, 170% preset, 200% upper bound and 190% lower preset"
    );
}

/// #1200: semantic containers remain horizontally contained and every group
/// can be brought into the single page viewport at both supported densities.
pub fn scenario_settings_groups(cx: &mut VisualTestAppContext) {
    use gpui::{point, px, size};
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, w, cx| {
        w.resize(size(px(1392.), px(883.)));
        app.update(cx, |app, cx| app.handle_menu_command("app.settings", w, cx));
        app.update(cx, |app, cx| app.set_theme("apple-light", cx));
    })
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        crate::macos::drain_native_events();
        cx.run_until_parked();
        if cx
            .update_window(window, |_, w, _| w.viewport_size())
            .unwrap()
            == size(px(1392.), px(883.))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "1392×883 native viewport"
        );
    }
    for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        for zoom in [1., 1.67] {
            kagi::ui::theme::set_zoom(zoom);
            let scroll = cx.read(|cx| app.read(cx).settings_scroll.clone());
            scroll.set_offset(point(px(0.), px(0.)));
            draw(cx, window);
            let viewport = e2e::control_bounds(window.window_id(), "settings-scrollbar")
                .expect("page viewport");
            for name in [
                "settings-appearance-group",
                "settings-graph-group",
                "settings-network-group",
                "settings-behaviour-group",
                "settings-language-group",
                "settings-smart-group",
                "settings-analyze-group",
            ] {
                draw(cx, window);
                let group = e2e::control_bounds(window.window_id(), name)
                    .unwrap_or_else(|| panic!("semantic group {name} must exist"));
                assert!(group.left() >= viewport.left() - px(1.), "{name} left");
                assert!(group.right() <= viewport.right() + px(1.), "{name} right");
                let delta = group.top() - viewport.top();
                scroll.set_offset(point(px(0.), scroll.offset().y - delta));
                draw(cx, window);
                let reached = e2e::control_bounds(window.window_id(), name).unwrap();
                assert!(reached.top() < viewport.bottom(), "{name} scroll reachable");
                assert!(reached.bottom() > viewport.top(), "{name} not hidden");
                check_group_surface(cx, window, name, &scroll);
            }
            assert!(
                scroll.offset().y < px(-20.),
                "one page owner reaches the tail"
            );
            check_grouped_rows(cx, &app, window, zoom);
            check_extra_settings_controls(cx, &app, window, zoom);
            check_analyze_spacing(window);
            if zoom == 1. {
                check_settings_help(cx, &app, window);
            }
        }
    }
    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS settings_groups EN/JA 100%/167% grouped page reachability");
}

fn check_settings_help(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    use gpui::{point, px};
    use kagi::ui::settings_view::take_settings_help_for_e2e;
    let scroll = cx.read(|cx| app.read(cx).settings_scroll.clone());
    let viewport = e2e::control_bounds(window.window_id(), "settings-scrollbar").unwrap();
    for (id, measurement, detail) in [
        (
            "settings-avatar-help",
            "settings-avatar-help",
            Msg::SettingsLaneCompactDesc,
        ),
        (
            "settings-custom-themes-help",
            "settings-custom-description",
            Msg::SettingsCustomThemesDesc,
        ),
        (
            "settings-analyze-help",
            "settings-analyze-description",
            Msg::SettingsAnalyzeIgnoreDesc,
        ),
    ] {
        cx.simulate_mouse_move(window, point(px(0.), px(0.)), None, gpui::Modifiers::none());
        let bounds = e2e::control_bounds(window.window_id(), measurement).unwrap();
        scroll.set_offset(point(
            px(0.),
            scroll.offset().y - bounds.top() + viewport.top() + px(8.),
        ));
        draw(cx, window);
        let bounds = e2e::control_bounds(window.window_id(), measurement).unwrap();
        take_settings_help_for_e2e(id);
        cx.simulate_mouse_move(
            window,
            point(bounds.left() + px(10.), bounds.center().y),
            None,
            gpui::Modifiers::none(),
        );
        for _ in 0..3 {
            cx.advance_clock(std::time::Duration::from_millis(400));
            cx.run_until_parked();
            draw(cx, window);
        }
        assert_eq!(
            take_settings_help_for_e2e(id).as_deref(),
            Some(detail.t()),
            "{id} hover builds the full localized explanation"
        );
    }
    cx.simulate_mouse_move(window, point(px(0.), px(0.)), None, gpui::Modifiers::none());
    draw(cx, window);
}

fn check_analyze_spacing(window: AnyWindowHandle) {
    let bounds = |name| e2e::control_bounds(window.window_id(), name).unwrap();
    let group = bounds("settings-analyze-group");
    let title = bounds("settings-analyze-label");
    let description = bounds("settings-analyze-description");
    let editor = bounds("settings-analyze-editor");
    let reset = bounds("settings-analyze-reset");
    let save = bounds("settings-analyze-save");
    let spacing = kagi::ui::theme::scaled_px(8.) - gpui::px(1.);
    assert!(title.top() - group.top() >= spacing, "Analyze top padding");
    assert!(
        description.top() - title.bottom() >= spacing,
        "Analyze label gap"
    );
    assert!(
        editor.top() - description.bottom() >= spacing,
        "Analyze editor gap"
    );
    assert!(
        reset.top() - editor.bottom() >= spacing,
        "Analyze action gap"
    );
    assert!(
        group.bottom() - save.bottom() >= spacing,
        "Analyze bottom padding"
    );
}

fn check_group_surface(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    name: &str,
    scroll: &gpui::ScrollHandle,
) {
    use gpui::{point, px};
    let bounds = e2e::control_bounds(window.window_id(), name).unwrap();
    let at = point(bounds.left() + px(3.), bounds.top() + px(16.));
    cx.update_window(window, |_, w, _| {
        let image = w.render_to_image().expect("group native fill");
        let scale = w.scale_factor();
        let pixel = image
            .get_pixel(
                (f32::from(at.x) * scale) as u32,
                (f32::from(at.y) * scale) as u32,
            )
            .0;
        let color = kagi::ui::theme::theme().bg_base;
        for (got, expected) in
            pixel[..3]
                .iter()
                .zip([(color >> 16) as u8, (color >> 8) as u8, color as u8])
        {
            assert!(got.abs_diff(expected) <= 2, "{name} shared base surface");
        }
    })
    .unwrap();
    let before = scroll.offset().y;
    let delta = if before < px(-10.) { 10. } else { -10. };
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    draw(cx, window);
    assert!(
        (scroll.offset().y - before).abs() > px(1.),
        "{name} wheel must reach the page owner, not a nested group scroller"
    );
}

fn check_extra_settings_controls(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    zoom: f32,
) {
    use gpui::{point, px};
    let scroll = cx.read(|cx| app.read(cx).settings_scroll.clone());
    let viewport = e2e::control_bounds(window.window_id(), "settings-scrollbar").unwrap();
    for name in [
        "settings-custom-themes-path",
        "settings-custom-themes-open",
        "settings-custom-themes-reload",
        "settings-analyze-editor",
        "settings-custom-themes-guide-control",
        "settings-analyze-reset",
        "settings-analyze-save",
    ] {
        let bounds = e2e::control_bounds(window.window_id(), name).expect("preserved control");
        let delta = bounds.top() - viewport.top() - px(8.);
        scroll.set_offset(point(px(0.), scroll.offset().y - delta));
        draw(cx, window);
        let bounds = e2e::control_bounds(window.window_id(), name).unwrap();
        assert!(bounds.left() >= viewport.left() - px(1.), "{name} left");
        assert!(bounds.right() <= viewport.right() + px(1.), "{name} right");
        assert!(bounds.top() >= viewport.top() - px(1.), "{name} top");
        if name == "settings-analyze-editor" && bounds.size.height > viewport.size.height {
            // The existing 8–22-line editor is intentionally unchanged. An
            // oversized editor's two edges must both be page-scroll reachable.
            let delta = bounds.bottom() - viewport.bottom() + px(8.);
            scroll.set_offset(point(px(0.), scroll.offset().y - delta));
            draw(cx, window);
            let bottom = e2e::control_bounds(window.window_id(), name)
                .unwrap()
                .bottom();
            assert!(
                bottom <= viewport.bottom() + px(1.) && bottom > viewport.top(),
                "{name} tail reachable"
            );
        } else {
            assert!(
                bounds.bottom() <= viewport.bottom() + px(1.),
                "{name} bottom"
            );
        }
    }
    if zoom == 1. {
        for name in [
            "settings-custom-description",
            "settings-analyze-description",
        ] {
            let description = e2e::control_bounds(window.window_id(), name).unwrap();
            assert!(description.size.height <= px(36.), "{name} <= two lines");
        }
    }
}

fn check_grouped_rows(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    zoom: f32,
) {
    use gpui::{point, px};
    let scroll = cx.read(|cx| app.read(cx).settings_scroll.clone());
    let viewport = e2e::control_bounds(window.window_id(), "settings-scrollbar").unwrap();
    for title in [
        Msg::SettingsTheme,
        Msg::SettingsAccent,
        Msg::SettingsZoom,
        Msg::SettingsToolbarDisplay,
        Msg::SettingsCompact,
        Msg::SettingsLaneCompact,
        Msg::SettingsAutoFetch,
        Msg::SettingsGraphCopy,
        Msg::SettingsReduceMotion,
        Msg::SettingsTerminalAutoLock,
        Msg::SettingsInterfaceLang,
        Msg::SettingsSmartEnable,
        Msg::SettingsSmartProvider,
        Msg::SettingsSmartModel,
    ] {
        let name = format!("settings-control-{}", title.t());
        let bounds = e2e::control_bounds(window.window_id(), &name).expect("preserved control");
        let delta = bounds.top() - viewport.top() - px(8.);
        scroll.set_offset(point(px(0.), scroll.offset().y - delta));
        draw(cx, window);
        let bounds = e2e::control_bounds(window.window_id(), &name).unwrap();
        assert!(bounds.left() >= viewport.left() - px(1.), "{name} left");
        assert!(bounds.right() <= viewport.right() + px(1.), "{name} right");
        assert!(bounds.top() >= viewport.top() - px(1.), "{name} top");
        assert!(
            bounds.bottom() <= viewport.bottom() + px(1.),
            "{name} bottom: control={bounds:?}, viewport={viewport:?}, zoom={zoom}"
        );
        if zoom == 1. {
            let description = e2e::control_bounds(
                window.window_id(),
                &format!("settings-description-{}", title.t()),
            )
            .expect("description bounds");
            assert!(
                description.size.height <= px(36.),
                "{name} description <= two lines"
            );
        }
    }
}

fn focus(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    slot: usize,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.focus_settings_switch_for_e2e(slot, window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
}
