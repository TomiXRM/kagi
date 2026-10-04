//! Settings' switches from the keyboard and to assistive technology (#970),
//! Tier A: each is a Tab stop, in drawing order; Space and Enter flip it and
//! save the setting; the checked state given to assistive technology
//! follows; the pointer still flips it.
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
    let at = e2e::control_bounds(window.window_id(), &format!("{id}-key"))
        .expect("the switch is measured")
        .center();
    cx.simulate_click(window, at, gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(saved(key), on_off(!start), "one click, one flip");
    assert_eq!(toggled(cx, window, id), !start);
    cx.simulate_click(window, at, gpui::Modifiers::none());
    cx.run_until_parked();
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
/// Settings renderer. At the upper bound, + clamps; − reaches the 160% preset.
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
    let capped = kagi_ui_core::theme::step_zoom(kagi_ui_core::theme::zoom(), true);
    check(cx, capped, "167%", "1670");
    let lower = kagi_ui_core::theme::step_zoom(kagi_ui_core::theme::zoom(), false);
    check(cx, lower, "160%", "1600");

    app.update(cx, |app, cx| {
        app.menu_overlay = None;
        cx.notify();
    });
    unmount(cx, app, window);
    drop(restore);
    eprintln!(
        "[gui-e2e] PASS settings_zoom_bound 150% legacy -> 167%, upper step and 160% lower preset"
    );
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
