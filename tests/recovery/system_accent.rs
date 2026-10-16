//! #1181: native primary and custom focus-ring pixels from an in-memory accent.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Focusable, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, theme};

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, x: f32, y: f32) -> [u8; 4] {
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native accent capture");
        let scale = window.scale_factor();
        image.get_pixel((x * scale) as u32, (y * scale) as u32).0
    })
    .unwrap()
}

#[track_caller]
fn assert_rgb(actual: [u8; 4], expected: u32) {
    for (got, want) in actual[..3].iter().zip([
        (expected >> 16) as u8,
        (expected >> 8) as u8,
        expected as u8,
    ]) {
        assert!(
            got.abs_diff(want) <= 2,
            "native {actual:?}, expected #{expected:06x}"
        );
    }
}

/// Inspect rasterized glyph cores, not only the renderer's assigned colour.
/// Each region must contain real text; callers supply its actual painted fill.
pub(crate) fn assert_label_contrast(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
    background: [u8; 4],
) {
    let bounds = e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("missing native text region {id}"));
    let luminance = |pixel: [u8; 4]| {
        let linear = |value: u8| {
            let value = f64::from(value) / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
    };
    let background = luminance(background);
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native Nagi text capture");
        let scale = window.scale_factor();
        let left = (f32::from(bounds.left()) * scale).ceil() as u32;
        let right = (f32::from(bounds.right()) * scale).floor() as u32;
        let top = (f32::from(bounds.top()) * scale).ceil() as u32;
        let bottom = (f32::from(bounds.bottom()) * scale).floor() as u32;
        let mut strongest: f64 = 1.;
        for y in top..bottom {
            for x in left..right {
                let ink = luminance(image.get_pixel(x, y).0);
                strongest =
                    strongest.max((ink.max(background) + 0.05) / (ink.min(background) + 0.05));
            }
        }
        assert!(
            strongest >= 4.5,
            "{id}: native glyph contrast {strongest:.3}:1 < 4.5:1"
        );
    })
    .unwrap();
}

fn assert_graph_labels(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    background: [u8; 4],
) {
    for id in [
        "graph-commit-label",
        "graph-commit-secondary",
        "graph-commit-time",
    ] {
        assert_label_contrast(cx, window, id, background);
    }
}

pub fn scenario_system_accent_chrome(cx: &mut VisualTestAppContext) {
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "accent_source", "accent", "lang"]);
    let before = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        if app.ui().selected != Some(0) {
            app.select_headless(0);
        }
        app.set_window_active_for_e2e(true, cx);
    });
    for slug in ["apple-light", "apple-dark"] {
        for accent in [
            theme::SystemAccent::Blue,
            theme::SystemAccent::Purple,
            theme::SystemAccent::Graphite,
            theme::SystemAccent::Red,
            theme::SystemAccent::Yellow,
        ] {
            let injected = theme::AccentInjection::new(accent, None);
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(1.), gpui::px(1.)),
                None,
                Modifiers::none(),
            );
            draw(cx, window);
            let bounds = e2e::control_bounds(window.window_id(), "sidebar-mode-graph")
                .expect("drawn mode cell");
            let label =
                e2e::control_bounds(window.window_id(), "sidebar-mode-graph-label").unwrap();
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx));
            })
            .unwrap();
            cx.simulate_keystrokes(window, "f19");
            draw(cx, window);
            cx.update_window(window, |_, window, cx| {
                assert_eq!(app.read(cx).mode_nav_focused_for_e2e(window), Some(0));
                assert!(window.last_input_was_keyboard());
            })
            .unwrap();
            assert_eq!(
                e2e::control_bounds(window.window_id(), "sidebar-mode-graph").unwrap(),
                bounds
            );
            assert_eq!(
                e2e::control_bounds(window.window_id(), "sidebar-mode-graph-label").unwrap(),
                label
            );
            let expected = accent.rgb(slug == "apple-dark");
            let default_expected = if slug == "apple-dark" {
                0xffd600
            } else {
                expected
            };
            assert_eq!(theme::follows_system_accent(), slug == "apple-light");
            assert_eq!(theme::theme().ui_accent(), default_expected);
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(bounds.center().x),
                    // measure_inside's absolute canvas begins inside the border.
                    // Sample the fixed 2pt ring immediately outside its top edge.
                    f32::from(bounds.top()) - 1.,
                ),
                default_expected,
            );

            let row = e2e::control_bounds(window.window_id(), "graph-commit-row")
                .expect("selected first graph row drawn");
            let bar = e2e::control_bounds(window.window_id(), "graph-selection-bar")
                .expect("selected graph accent bar drawn");
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(bar.center().x),
                    f32::from(bar.center().y),
                ),
                default_expected,
            );
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(row.right()) - 2.,
                    f32::from(row.top()) + 1.,
                ),
                graph_wash(default_expected, theme::theme().bg_base),
            );
            let selected = cx.read(|cx| app.read(cx).ui().selected);
            app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
            draw(cx, window);
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(row.right()) - 2.,
                    f32::from(row.top()) + 1.,
                ),
                theme::theme().inactive_selected,
            );
            assert_eq!(cx.read(|cx| app.read(cx).ui().selected), selected);
            app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
            draw(cx, window);
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(row.right()) - 2.,
                    f32::from(row.top()) + 1.,
                ),
                graph_wash(default_expected, theme::theme().bg_base),
            );

            let output = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(fixture.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            let head =
                kagi_git::CommitId(String::from_utf8(output.stdout).unwrap().trim().to_string());
            app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
            draw(cx, window);
            let input = cx
                .read(|cx| {
                    app.read(cx)
                        .create_branch_modal()
                        .unwrap()
                        .input_state
                        .clone()
                })
                .unwrap();
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |state, cx| state.set_value("accent-probe", window, cx))
            })
            .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                cx.advance_clock(std::time::Duration::from_millis(300));
                cx.run_until_parked();
                draw(cx, window);
                if cx.read(|cx| {
                    app.read(cx)
                        .create_branch_modal()
                        .unwrap()
                        .plan
                        .plan()
                        .is_some_and(|p| p.blockers.is_empty())
                }) {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "primary plan ready");
            }
            cx.update_window(window, |_, window, cx| {
                window.focus(&input.read(cx).focus_handle(cx), cx);
            })
            .unwrap();
            draw(cx, window);
            let input_bounds =
                e2e::control_bounds(window.window_id(), "input-modal-field").unwrap();
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(input_bounds.left()) + 0.25,
                    f32::from(input_bounds.center().y),
                ),
                default_expected,
            );
            let primary = e2e::confirm_bounds(window.window_id()).unwrap();
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(primary.left()) + 6.,
                    f32::from(primary.center().y),
                ),
                default_expected,
            );
            app.update(cx, |_, cx| {
                theme::set_follows_system_accent(true);
                theme::sync_gpui_component_theme(cx);
                cx.notify();
                assert_eq!(theme::theme().ui_accent(), expected);
                assert_eq!(
                    kagi_ui_core::settings::read_setting("accent_source").as_deref(),
                    Some("system")
                );
            });
            draw(cx, window);
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(input_bounds.left()) + 0.25,
                    f32::from(input_bounds.center().y),
                ),
                expected,
            );
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(primary.left()) + 6.,
                    f32::from(primary.center().y),
                ),
                expected,
            );
            app.update(cx, |_, cx| {
                theme::set_follows_system_accent(false);
                theme::sync_gpui_component_theme(cx);
                cx.notify();
                assert_eq!(theme::theme().ui_accent(), theme::theme().color_branch);
                assert_eq!(
                    kagi_ui_core::settings::read_setting("accent_source").as_deref(),
                    Some("theme")
                );
            });
            draw(cx, window);
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(primary.left()) + 6.,
                    f32::from(primary.center().y),
                ),
                theme::theme().color_branch,
            );
            app.update(cx, |app, cx| {
                app.cancel_create_branch_modal();
                cx.notify();
            });
            drop(input);
            app.update(cx, |_, cx| {
                theme::set_follows_system_accent(true);
                theme::sync_gpui_component_theme(cx);
                cx.notify();
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx));
            })
            .unwrap();
            cx.simulate_keystrokes(window, "f19");
            draw(cx, window);
            assert_eq!(
                e2e::control_bounds(window.window_id(), "sidebar-mode-graph").unwrap(),
                bounds
            );
            assert_eq!(
                e2e::control_bounds(window.window_id(), "sidebar-mode-graph-label").unwrap(),
                label
            );
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(bounds.center().x),
                    f32::from(bounds.top()) - 1.,
                ),
                expected,
            );
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(bar.center().x),
                    f32::from(bar.center().y),
                ),
                expected,
            );
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(row.right()) - 2.,
                    f32::from(row.top()) + 1.,
                ),
                graph_wash(expected, theme::theme().bg_base),
            );
            drop(injected);
        }
    }
    app.update(cx, |app, cx| app.set_theme(&before, cx));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS system_accent_chrome: Apple Dark default yellow, Light default system, explicit System/Theme accent primary/ring/graph pixels for five injected colours; fixed geometry and retained inactive selection");
}

pub fn scenario_nagi_fixed_accent(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "reduce_motion"]);
    let before = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.select_headless(0);
        app.set_window_active_for_e2e(true, cx);
    });
    for slug in ["nagi-light", "nagi-dark"] {
        for accent in [theme::SystemAccent::Purple, theme::SystemAccent::Red] {
            let _injected = theme::AccentInjection::new(accent, Some(true));
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.focus_mode_nav_for_e2e(0, window, cx));
            })
            .unwrap();
            cx.simulate_keystrokes(window, "f19");
            draw(cx, window);
            let bounds = e2e::control_bounds(window.window_id(), "sidebar-mode-graph")
                .expect("drawn mode cell");
            assert_rgb(
                pixel(
                    cx,
                    window,
                    f32::from(bounds.center().x),
                    f32::from(bounds.top()) - 1.,
                ),
                theme::theme().color_branch,
            );
            let selected = cx.read(|cx| app.read(cx).ui().selected);
            cx.update_window(window, |_, window, cx| {
                assert_eq!(app.read(cx).mode_nav_focused_for_e2e(window), Some(0));
                assert!(window.last_input_was_keyboard());
            })
            .unwrap();
            let mut selection_pixels = [0; 2];
            for (state, active) in [true, false].into_iter().enumerate() {
                app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
                draw(cx, window);
                let row = e2e::control_bounds(window.window_id(), "graph-commit-row")
                    .expect("selected commit row remains drawn");
                let bar = e2e::control_bounds(window.window_id(), "graph-selection-bar")
                    .expect("selection has a geometric indicator");
                let palette = theme::theme();
                let fill = pixel(
                    cx,
                    window,
                    f32::from(row.right()) - 2.,
                    f32::from(row.top()) + 1.,
                );
                selection_pixels[state] =
                    (u32::from(fill[0]) << 16) | (u32::from(fill[1]) << 8) | u32::from(fill[2]);
                assert_rgb(
                    fill,
                    if active {
                        palette.selected
                    } else {
                        palette.inactive_selected
                    },
                );
                assert_graph_labels(cx, window, fill);
                let badge = e2e::control_bounds(window.window_id(), "graph-selected-badge-label")
                    .expect("selected branch label painted");
                let badge_fill = pixel(
                    cx,
                    window,
                    f32::from(badge.center().x),
                    f32::from(badge.top()) + 0.25,
                );
                assert_label_contrast(cx, window, "graph-selected-badge-label", badge_fill);
                if active {
                    assert_rgb(
                        pixel(
                            cx,
                            window,
                            f32::from(bar.center().x),
                            f32::from(bar.center().y),
                        ),
                        palette.color_branch,
                    );
                }
                assert_eq!(cx.read(|cx| app.read(cx).ui().selected), selected);
            }
            // Compare rendered fills, not just unequal tokens: tiny gray shifts
            // preserve identity/contrast but do not establish activation state.
            let separation =
                kagi_ui_core::color_vision::delta_e(selection_pixels[0], selection_pixels[1], None);
            assert!(
                separation >= 10.0,
                "{slug}: rendered active/inactive selection ΔE {separation:.2} < 10"
            );
            app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
        }
    }
    // Exercise both sides of a real retained selection transition without
    // changing the fixture or replacing production graph state with a mock.
    let reduced = theme::reduce_motion();
    theme::set_reduce_motion(false);
    app.update(cx, |app, cx| {
        app.set_theme("nagi-dark", cx);
        app.set_window_active_for_e2e(true, cx);
    });
    let mut at = std::time::Instant::now();
    for target in [1, 0] {
        kagi_ui_core::motion::set_clock_for_e2e(Some(at));
        app.update(cx, |app, cx| {
            app.select_headless(target);
            cx.notify();
        });
        draw(cx, window);
        let mut first_fill = None;
        for elapsed in [0, 30, 60, 90, 120, 400] {
            kagi_ui_core::motion::set_clock_for_e2e(Some(
                at + std::time::Duration::from_millis(elapsed),
            ));
            draw(cx, window);
            let row = e2e::control_bounds(window.window_id(), "graph-commit-row").unwrap();
            let fill = pixel(
                cx,
                window,
                f32::from(row.right()) - 2.,
                f32::from(row.top()) + 1.,
            );
            assert_graph_labels(cx, window, fill);
            if elapsed == 0 {
                first_fill = Some(fill);
            } else if elapsed == 90 {
                assert_ne!(
                    Some(fill),
                    first_fill,
                    "the selected fill must actually fade"
                );
            }
        }
        assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(target));
        at += std::time::Duration::from_secs(1);
    }
    kagi_ui_core::motion::set_clock_for_e2e(None);
    theme::set_reduce_motion(reduced);
    app.update(cx, |app, cx| app.set_theme(&before, cx));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS nagi_fixed_accent: fixed keyboard focus/selection despite OS accent, distinct active/inactive fills, native primary/secondary/age/badge contrast and incoming/outgoing fades, retained selected identity");
}

fn graph_wash(accent: u32, background: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |rgb, shift| {
        let fg = ((accent >> shift) & 255) as f32;
        let bg = ((background >> shift) & 255) as f32;
        rgb | (((fg * 0.30 + bg * 0.70).round() as u32) << shift)
    })
}
