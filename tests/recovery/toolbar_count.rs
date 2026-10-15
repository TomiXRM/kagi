//! #1067: the real toolbar count's text and pill scale together, without clipping.
use gpui::{AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{
    commands::{ZoomIn, ZoomOut, ZoomReset},
    e2e,
};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::recovery_layout::contained;
const ICON_BUTTONS: [&str; 12] = [
    "tb-pull",
    "tb-push",
    "tb-branch",
    "tb-stash",
    "tb-pop",
    "tb-undo",
    "tb-redo",
    "tb-terminal",
    "tb-graph-mode",
    "tb-pr-mode",
    "tb-editor-ws",
    "tb-ecosystem",
];

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
}

fn inspect(window: AnyWindowHandle, button: &str) -> (f32, f32, f32) {
    let badge = bounds(window, &format!("{button}-count"));
    let text = bounds(window, &format!("{button}-count-text"));
    let paint = e2e::control_paint(window.window_id(), &format!("{button}-count-text")).unwrap();
    contained(badge, text, "count text inside pill");
    contained(
        bounds(window, "toolbar-row"),
        badge,
        "count pill inside toolbar row",
    );
    contained(paint.mask, text, "count text inside paint mask");
    let icon = bounds(window, &format!("{button}-icon"));
    let overlap_width =
        f32::from(badge.right().min(icon.right()) - badge.left().max(icon.left())).max(0.);
    let overlap_height =
        f32::from(badge.bottom().min(icon.bottom()) - badge.top().max(icon.top())).max(0.);
    // The old 14-unit pill overhung the icon cell by 2 units on each axis.
    // Neither the larger pill nor a wider 99+ label may cover more icon area.
    let old_coverage = theme::scaled(14.0 - 2.0).powi(2);
    assert!(
        overlap_width * overlap_height <= old_coverage + 0.5,
        "{button}: badge covers {} icon pixels, above old 14-unit coverage {old_coverage}",
        overlap_width * overlap_height
    );
    (
        f32::from(badge.size.height),
        f32::from(paint.font_size),
        f32::from(text.size.width),
    )
}

fn ratio(actual: f32, baseline: f32, zoom: f32, label: &str) {
    assert!(
        (actual / baseline - zoom).abs() < 0.06,
        "{label}: ratio {} must follow zoom {zoom}",
        actual / baseline
    );
}

fn shaped_width_scale(actual: f32, baseline: f32, zoom: f32) {
    // GPUI rounds each measured text box outward to whole logical pixels.
    // One digit cannot use a percentage tolerance (e.g. 6px -> 9px).
    // The two independently rounded boxes contribute at most 1 + zoom pixels.
    assert!(
        (actual - baseline * zoom).abs() <= 1.0 + zoom,
        "shaped digit width {actual} must scale from {baseline} at {zoom}"
    );
}

pub fn scenario_toolbar_count_scale(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    kagi_ui_core::settings::write_setting("toolbar_display", None);
    kagi_ui_core::settings::init_toolbar_display();
    for slug in ["apple-light", "apple-dark"] {
        assert_eq!(
            kagi_ui_core::settings::toolbar_display(slug),
            kagi_ui_core::settings::ToolbarDisplay::IconOnly
        );
    }
    assert_eq!(
        kagi_ui_core::settings::toolbar_display("catppuccin"),
        kagi_ui_core::settings::ToolbarDisplay::IconAndText
    );
    assert!(theme::set_active("apple-light"));
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    frame(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
    for display in [
        kagi_ui_core::settings::ToolbarDisplay::IconOnly,
        kagi_ui_core::settings::ToolbarDisplay::IconAndText,
    ] {
        cx.dispatch_action(
            window,
            kagi::ui::commands::SetToolbarDisplay { mode: display },
        );
        app.update(cx, |app, cx| {
            app.view_mut().toolbar_state.ahead = 100;
            app.view_mut().toolbar_state.behind = 100;
            cx.notify();
        });
        if display == kagi_ui_core::settings::ToolbarDisplay::IconAndText {
            std::env::set_var("KAGI_LANG", "ja");
            kagi_ui_core::i18n::init_lang();
            e2e::clear_toolbar_unavailable();
            frame(cx, window);
            for button in [
                "tb-pull",
                "tb-push",
                "tb-branch",
                "tb-stash",
                "tb-pop",
                "tb-undo",
                "tb-redo",
                "tb-terminal",
                "tb-graph-mode",
                "tb-editor-ws",
                "tb-ecosystem",
            ] {
                let name = e2e::toolbar_label(button).expect("JA AX name");
                let (caption, tooltip) = e2e::toolbar_caption_and_tooltip(button);
                assert_eq!(
                    caption,
                    Some(name),
                    "{button}: JA caption must match AX name"
                );
                assert_eq!(
                    tooltip,
                    Some(name),
                    "{button}: JA tooltip must match AX name"
                );
            }
        }
        frame(cx, window);
        assert_eq!(
            kagi_ui_core::settings::Settings::load().toolbar_display(),
            Some(display)
        );
        let expected = if display == kagi_ui_core::settings::ToolbarDisplay::IconOnly {
            52.
        } else {
            60.
        };
        assert!(
            (f32::from(bounds(window, "toolbar-row").size.height) - theme::scaled(expected)).abs()
                < 1.
        );
        if display == kagi_ui_core::settings::ToolbarDisplay::IconOnly {
            for locale in ["en", "ja"] {
                std::env::set_var("KAGI_LANG", locale);
                kagi_ui_core::i18n::init_lang();
                for zoom in [1., 1.667] {
                    theme::set_zoom(zoom);
                    for width in [940., 1392.] {
                        let wanted = gpui::size(gpui::px(width), gpui::px(883.));
                        cx.update_window(window, |_, window, _| window.resize(wanted))
                            .unwrap();
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_secs(2);
                        loop {
                            crate::macos::drain_native_events();
                            cx.run_until_parked();
                            if cx
                                .update_window(window, |_, window, _| window.viewport_size())
                                .unwrap()
                                == wanted
                            {
                                break;
                            }
                            assert!(
                                std::time::Instant::now() < deadline,
                                "native resize timed out"
                            );
                        }
                        crate::recovery_layout::draw(cx, window, (width, 883.));
                        e2e::clear_control_bounds(window.window_id(), "tb-overflow");
                        for index in 0..14 {
                            e2e::clear_control_bounds(
                                window.window_id(),
                                &format!("tb-overflow-item-{index}"),
                            );
                        }
                        for button in ICON_BUTTONS {
                            e2e::clear_control_bounds(
                                window.window_id(),
                                &format!("{button}-icon"),
                            );
                            e2e::clear_control_bounds(
                                window.window_id(),
                                &format!("{button}-glyph"),
                            );
                        }
                        for _ in 0..3 {
                            frame(cx, window);
                        }
                        let overflow = e2e::control_bounds(window.window_id(), "tb-overflow");
                        if let Some(overflow) = overflow {
                            cx.simulate_click(window, overflow.center(), gpui::Modifiers::none());
                            cx.run_until_parked();
                            frame(cx, window);
                        }
                        let row = bounds(window, "toolbar-row");
                        assert!((f32::from(row.size.height) - theme::scaled(expected)).abs() < 1.);
                        inspect(window, "tb-pull");
                        inspect(window, "tb-push");
                        let mut previous = None;
                        for button in ICON_BUTTONS.into_iter().filter(|id| *id != "tb-pr-mode") {
                            assert!(!e2e::toolbar_label(button)
                                .expect("AX/tooltip label")
                                .is_empty());
                            let icon =
                                e2e::control_bounds(window.window_id(), &format!("{button}-icon"))
                                    .unwrap_or_default();
                            if icon.size.width <= gpui::px(0.) {
                                let index =
                                    ICON_BUTTONS.iter().position(|id| *id == button).unwrap();
                                assert!(
                                    overflow.is_some(),
                                    "{button}: hidden glyph requires overflow"
                                );
                                assert!(
                                    e2e::control_bounds(
                                        window.window_id(),
                                        &format!("tb-overflow-item-{index}")
                                    )
                                    .is_some(),
                                    "{button}: hidden command must be in the actual menu"
                                );
                                continue;
                            }
                            contained(row, icon, button);
                            let glyph = bounds(window, &format!("{button}-glyph"));
                            assert!((f32::from(glyph.size.height) - theme::scaled(20.)).abs() < 1.);
                            assert!((f32::from(icon.size.height) - theme::scaled(22.)).abs() < 1.);
                            if let Some(right) = previous {
                                assert!(
                                    icon.left() >= right,
                                    "{locale}/{width}/{zoom}: {button} overlaps preceding icon"
                                );
                            }
                            previous = Some(icon.right());
                        }
                        if overflow.is_some() {
                            cx.simulate_keystrokes(window, "escape");
                            cx.run_until_parked();
                        }
                    }
                }
            }
        }
        for count in [1, 99, 100] {
            // Seed the session-owned accepted read, not an alternate badge component.
            app.update(cx, |app, cx| {
                app.view_mut().toolbar_state.ahead = count;
                app.view_mut().toolbar_state.behind = count;
                cx.notify();
            });
            cx.dispatch_action(window, ZoomReset);
            frame(cx, window);
            let baseline = [inspect(window, "tb-pull"), inspect(window, "tb-push")];
            assert!((theme::zoom() - 1.).abs() < 0.001);
            // Seven public 10% steps reach 170%; legacy 1667-permille
            // settings remain covered by the direct zoom matrix above.
            for _ in 0..7 {
                cx.dispatch_action(window, ZoomIn);
                frame(cx, window);
            }
            assert!((theme::zoom() - 1.7).abs() < 0.001);
            for (index, button) in ["tb-pull", "tb-push"].into_iter().enumerate() {
                let measured = inspect(window, button);
                ratio(measured.0, baseline[index].0, 1.7, "badge height");
                ratio(measured.1, baseline[index].1, 1.7, "badge font");
                shaped_width_scale(measured.2, baseline[index].2, 1.7);
                assert!(
                    baseline[index].1 >= 11.,
                    "100% count font must be at least 11 logical points; got {}",
                    baseline[index].1
                );
            }
            cx.dispatch_action(window, ZoomReset);
            frame(cx, window);
            for _ in 0..3 {
                cx.dispatch_action(window, ZoomOut);
                frame(cx, window);
            }
            assert!((theme::zoom() - 0.7).abs() < 0.001);
            for (index, button) in ["tb-pull", "tb-push"].into_iter().enumerate() {
                let measured = inspect(window, button);
                ratio(measured.0, baseline[index].0, 0.7, "small badge height");
                ratio(measured.1, baseline[index].1, 0.7, "small badge font");
                shaped_width_scale(measured.2, baseline[index].2, 0.7);
            }
        }
    }
    for mode in [
        kagi_ui_core::settings::ToolbarDisplay::IconOnly,
        kagi_ui_core::settings::ToolbarDisplay::IconAndText,
    ] {
        app.update(cx, |app, cx| {
            assert!(app.open_platform_menu_for_e2e("View", cx))
        });
        frame(cx, window);
        let row = bounds(window, &format!("platform-menu-toolbar-{}", mode.as_str()));
        contained(
            bounds(window, "platform-menu-panel"),
            row,
            "toolbar menu row",
        );
        cx.simulate_click(window, row.center(), gpui::Modifiers::none());
        frame(cx, window);
        assert_eq!(
            kagi_ui_core::settings::Settings::load().toolbar_display(),
            Some(mode)
        );
        let entries = kagi::ui::commands::toolbar_display_menu_entries();
        assert!(entries
            .iter()
            .find(|(entry, _)| *entry == mode)
            .unwrap()
            .1
            .starts_with("✓ "));
    }
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "zoom must not write the repository"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS toolbar_count_scale: Pull/Push 1/99/99+ at 100%/167%/170%/70%, actual font and shaped digit bounds, pill/row/mask containment");
}
