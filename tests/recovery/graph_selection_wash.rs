//! #1485: Graph selection preserves stripes/lane tints and normal ink.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Hsla, Modifiers, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::theme;

pub fn scenario_graph_selection_wash(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "accent_source",
        "accent",
        "graph_lane_compact",
    ]);
    let fixture = build_fixture();
    std::fs::write(
        fixture.path().join("README.md"),
        "Graph wash dirty fixture\n",
    )
    .unwrap();
    let (app, window) = mount(cx, fixture.path());
    theme::set_graph_lane_compact(true);
    for slug in ["apple-light", "apple-dark"] {
        let accent = if slug == "apple-light" {
            theme::SystemAccent::Blue
        } else {
            theme::SystemAccent::Yellow
        };
        let _accent = theme::AccentInjection::new(accent, Some(true));
        for zoom in [1000, 1670] {
            theme::set_zoom(zoom as f32 / 1000.);
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                if let Some(selected) = app.ui().selected {
                    app.select_headless(selected);
                }
                app.set_window_active_for_e2e(true, cx);
            });
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(1.), gpui::px(1.)),
                None,
                Modifiers::none(),
            );
            for ix in [0, 1] {
                draw(cx, window);
                let row_id = format!("graph-row-{ix}");
                let lane_id = format!("graph-lane-band-{ix}");
                let baseline = pixel(cx, window, &row_id);
                let lane_baseline = pixel(cx, window, &lane_id);
                let palette = theme::theme();
                let stripe = if ix == 0 {
                    palette.bg_base
                } else {
                    palette.bg_row_alt
                };
                assert_rgb(baseline, stripe, "ordinary stripe");
                app.update(cx, |app, cx| {
                    app.select_headless(ix);
                    cx.notify();
                });
                draw(cx, window);
                // Selecting opens the existing Inspector; compare activation
                // geometry only after that legitimate workspace reflow.
                let row = e2e::control_bounds(window.window_id(), &row_id).unwrap();
                let actual = pixel(cx, window, &row_id);
                assert_rgb(
                    actual,
                    composite(palette.ui_accent(), 0.30, baseline),
                    "Graph accent wash over stripe",
                );
                assert_ne!(
                    actual,
                    palette.ui_accent(),
                    "Graph selection is not opaque accent"
                );
                assert_rgb(
                    pixel(cx, window, &lane_id),
                    composite(palette.ui_accent(), 0.30, lane_baseline),
                    "accent wash over preserved lane tint",
                );
                assert!(
                    contrast(palette.text_main, actual) >= 4.5,
                    "Graph primary contrast {slug}/{zoom}/{ix}"
                );
                if ix == 0 {
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-commit-label")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(palette.text_main))
                    );
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-commit-secondary")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(palette.text_sub))
                    );
                    let lane = cx.read(|cx| app.read(cx).view().rows[0].node_color);
                    let (_, _, ink) = theme::badge_style(theme::lane_color_u32(lane));
                    assert_eq!(
                        e2e::control_paint(window.window_id(), "graph-selected-badge-label")
                            .unwrap()
                            .text_color,
                        Hsla::from(gpui::rgb(ink))
                    );
                }
                app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
                draw(cx, window);
                assert_rgb(
                    pixel(cx, window, &row_id),
                    palette.inactive_selected,
                    "inactive Graph grey",
                );
                assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(ix));
                assert_eq!(
                    e2e::control_bounds(window.window_id(), &row_id).unwrap(),
                    row
                );
                app.update(cx, |app, cx| {
                    app.set_window_active_for_e2e(true, cx);
                    app.select_headless(ix);
                    cx.notify();
                });
            }
            draw(cx, window);
            let wip_base = pixel(cx, window, "graph-wip-row");
            let chip_ink = e2e::control_paint(window.window_id(), "graph-wip-chip")
                .unwrap()
                .text_color;
            let note_ink = e2e::control_paint(window.window_id(), "graph-wip-note")
                .unwrap()
                .text_color;
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.open_commit_panel(window, cx))
            })
            .unwrap();
            cx.run_until_parked();
            draw(cx, window);
            let palette = theme::theme();
            let selected_wip = pixel(cx, window, "graph-wip-row");
            assert_rgb(
                selected_wip,
                composite(palette.ui_accent(), 0.30, wip_base),
                "WIP wash over lane tint",
            );
            assert!(
                contrast(palette.text_main, selected_wip) >= 4.5,
                "WIP normal primary contrast {slug}/{zoom}"
            );
            assert_eq!(
                e2e::control_paint(window.window_id(), "graph-wip-chip")
                    .unwrap()
                    .text_color,
                chip_ink
            );
            assert_eq!(
                e2e::control_paint(window.window_id(), "graph-wip-note")
                    .unwrap()
                    .text_color,
                note_ink
            );
            app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
            draw(cx, window);
            assert_rgb(
                pixel(cx, window, "graph-wip-row"),
                palette.inactive_selected,
                "inactive WIP grey",
            );
            app.update(cx, |app, cx| {
                app.select_headless(0);
                app.set_window_active_for_e2e(true, cx);
                cx.notify();
            });
        }
    }
    unmount(cx, app, window);
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> u32 {
    let row = e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"));
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native Graph wash capture");
        let scale = window.scale_factor();
        let (x, y) = if id.starts_with("graph-lane-band-") {
            // Avoid graph nodes/connector ink, including compressed columns.
            (
                f32::from(row.left()) + 5.,
                f32::from(row.top()) + f32::from(row.size.height) * 0.25,
            )
        } else {
            (f32::from(row.right()) - 4., f32::from(row.center().y))
        };
        let p = image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
    })
    .unwrap()
}

fn assert_rgb(actual: u32, expected: u32, context: &str) {
    assert!(
        [0, 8, 16]
            .into_iter()
            .all(|shift| ((actual >> shift) & 255).abs_diff((expected >> shift) & 255) <= 1),
        "{context}: #{actual:06x} != #{expected:06x}"
    );
}

fn composite(fg: u32, alpha: f32, bg: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |rgb, shift| {
        let f = ((fg >> shift) & 255) as f32;
        let b = ((bg >> shift) & 255) as f32;
        rgb | (((f * alpha + b * (1. - alpha)).round() as u32) << shift)
    })
}

fn contrast(a: u32, b: u32) -> f64 {
    let luminance = |c: u32| {
        let channel = |shift| {
            let v = ((c >> shift) & 255u32) as f64 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
