//! #1199 / #1201: neutral Settings hierarchy and circular radio mark.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{rgb, FontWeight, VisualTestAppContext};
use kagi::ui::{e2e, theme};

fn luminance(color: u32) -> f64 {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8]
        .into_iter()
        .zip([0.2126, 0.7152, 0.0722])
        .map(|(channel, weight)| {
            let c = f64::from(channel) / 255.;
            weight
                * if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
        })
        .sum()
}

pub fn scenario_settings_quiet(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "lang"]);
    let old_lang = kagi::ui::i18n::lang();
    let old_theme = theme::theme().slug.to_string();
    let old_zoom = theme::zoom();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    // Secondary labels must remain readable against the Settings panel.
    for slug in ["apple-light", "apple-dark"] {
        for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
            kagi::ui::i18n::set_lang(lang);
            for accent in [theme::SystemAccent::Blue, theme::SystemAccent::Purple] {
                let _accent = theme::AccentInjection::new(accent, None);
                for zoom in [1.0, 1.67] {
                    app.update(cx, |app, cx| {
                        app.set_theme(slug, cx);
                        theme::set_zoom(zoom);
                        cx.notify();
                    });
                    let (label, panel) = (
                        luminance(theme::theme().text_sub),
                        luminance(theme::theme().panel),
                    );
                    assert!(
                        (label.max(panel) + 0.05) / (label.min(panel) + 0.05) >= 4.5,
                        "Settings heading contrast"
                    );
                    cx.update_window(window, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.handle_menu_command("app.settings", window, cx)
                        });
                        window.refresh();
                        window.draw(cx).clear();
                    })
                    .unwrap();
                    let heading =
                        e2e::control_paint(window.window_id(), "settings-appearance-heading")
                            .expect("heading painted");
                    assert_eq!(
                        heading.text_color,
                        rgb(theme::theme().text_sub).into(),
                        "neutral secondary-label role"
                    );
                    assert_eq!(heading.font_weight, FontWeight::SEMIBOLD);
                    assert_eq!(heading.font_size, theme::scaled_px(12.));
                    let bounds =
                        e2e::control_bounds(window.window_id(), "settings-appearance-heading")
                            .unwrap();
                    let title =
                        e2e::control_bounds(window.window_id(), "settings-titlebar").unwrap();
                    let frame = e2e::control_bounds(window.window_id(), "settings-frame").unwrap();
                    cx.update_window(window, |_, window, _| {
                        let image = window.render_to_image().expect("Settings native image");
                        let scale = window.scale_factor();
                        for b in [bounds, title] {
                            let p = image
                                .get_pixel(
                                    (f32::from(b.center().x) * scale) as u32,
                                    ((f32::from(b.bottom()) + 0.5) * scale) as u32,
                                )
                                .0;
                            let color = theme::theme().surface;
                            for (got, expected) in p[..3].iter().zip([
                                (color >> 16) as u8,
                                (color >> 8) as u8,
                                color as u8,
                            ]) {
                                assert!(
                                    got.abs_diff(expected) <= 2,
                                    "separator {p:?} expected {color:06x}"
                                );
                            }
                            // Adjacent interior/exterior pixels stay panel-coloured:
                            // a scaled 1.67px separator would cross this oracle.
                            for offset in [-0.5, 1.5] {
                                let p = image
                                    .get_pixel(
                                        (f32::from(b.center().x) * scale) as u32,
                                        ((f32::from(b.bottom()) + offset) * scale) as u32,
                                    )
                                    .0;
                                let color = theme::theme().panel;
                                for (got, expected) in p[..3].iter().zip([
                                    (color >> 16) as u8,
                                    (color >> 8) as u8,
                                    color as u8,
                                ]) {
                                    assert!(
                                        got.abs_diff(expected) <= 2,
                                        "separator is exactly one logical pixel: {p:?}"
                                    );
                                }
                            }
                        }
                        let p = image
                            .get_pixel(
                                ((f32::from(frame.left()) - 0.5) * scale) as u32,
                                (f32::from(frame.center().y) * scale) as u32,
                            )
                            .0;
                        let color = theme::theme().surface;
                        for (got, expected) in p[..3].iter().zip([
                            (color >> 16) as u8,
                            (color >> 8) as u8,
                            color as u8,
                        ]) {
                            assert!(
                                got.abs_diff(expected) <= 2,
                                "frame {p:?} expected {color:06x}"
                            );
                        }
                    })
                    .unwrap();
                    app.update(cx, |app, cx| {
                        app.menu_overlay = None;
                        cx.notify();
                    });
                }
            }
        }
    }
    app.update(cx, |app, cx| {
        app.set_theme(&old_theme, cx);
        theme::set_zoom(old_zoom);
        cx.notify();
    });
    kagi::ui::i18n::set_lang(old_lang);
    unmount(cx, app, window);
}

struct RadioProbe;

impl gpui::Render for RadioProbe {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement as _, Styled as _};
        gpui::div()
            .p(gpui::px(20.))
            .bg(rgb(theme::theme().panel))
            .child(e2e::measure_control(
                "radio-selected",
                gpui_component::radio::Radio::new("selected").checked(true),
            ))
            .child(e2e::measure_control(
                "radio-empty",
                gpui_component::radio::Radio::new("empty"),
            ))
    }
}

pub fn scenario_settings_radio_dot(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom"]);
    let old_theme = theme::theme().slug.to_string();
    let old_zoom = theme::zoom();
    let fixture = build_fixture();
    let (app, app_window) = mount(cx, fixture.path());
    let probe = cx.update(|cx| {
        use gpui::AppContext as _;
        cx.new(|_| RadioProbe)
    });
    let root = probe.clone();
    let window =
        crate::macos::open_offscreen(cx, gpui::size(gpui::px(160.), gpui::px(100.)), |_, _| root);
    let window: gpui::AnyWindowHandle = window.into();
    for slug in ["apple-light", "apple-dark"] {
        for zoom in [1.0, 1.67] {
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                theme::set_zoom(zoom);
                cx.notify();
            });
            cx.update_window(window, |_, window, cx| {
                window.set_rem_size(theme::scaled_px(16.));
                window.refresh();
                window.draw(cx).clear();
            })
            .unwrap();
            let checked = e2e::control_bounds(window.window_id(), "radio-selected").unwrap();
            let empty = e2e::control_bounds(window.window_id(), "radio-empty").unwrap();
            cx.update_window(window, |_, window, cx| {
                let image = window.render_to_image().expect("native radio glyph image");
                let scale = window.scale_factor();
                let radius = f32::from(theme::scaled_px(8.));
                let sample = |b: gpui::Bounds<gpui::Pixels>, dx: f32, dy: f32| {
                    image
                        .get_pixel(
                            ((f32::from(b.left()) + radius + dx * zoom) * scale) as u32,
                            ((f32::from(b.top()) + radius + dy * zoom) * scale) as u32,
                        )
                        .0
                };
                // A round mark is white symmetrically above/below/left/right;
                // the old check SVG does not satisfy this oracle.
                for (x, y) in [(0., 0.), (-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
                    let p = sample(checked, x, y);
                    assert!(
                        p[..3].iter().all(|c| *c >= 250),
                        "radio dot ({x},{y}): {p:?}"
                    );
                }
                for (x, y) in [(-4., 0.), (4., 0.), (0., -4.), (0., 4.)] {
                    let p = sample(checked, x, y);
                    let color = theme::theme().ui_accent();
                    for (got, want) in
                        p[..3]
                            .iter()
                            .zip([(color >> 16) as u8, (color >> 8) as u8, color as u8])
                    {
                        assert!(
                            got.abs_diff(want) <= 2,
                            "outside dot must be accent, not check path: {p:?}"
                        );
                    }
                }
                use gpui_component::ActiveTheme as _;
                let input: gpui::Rgba = cx.theme().input_background().into();
                let panel = rgb(theme::theme().panel);
                let expected = [
                    input.r * input.a + panel.r * (1. - input.a),
                    input.g * input.a + panel.g * (1. - input.a),
                    input.b * input.a + panel.b * (1. - input.a),
                ];
                for (x, y) in [(0., 0.), (-3., 0.), (3., 0.), (0., -3.), (0., 3.)] {
                    let p = sample(empty, x, y);
                    assert!(
                        p[..3]
                            .iter()
                            .zip(expected)
                            .all(|(got, want)| got.abs_diff((want * 255.).round() as u8) <= 2),
                        "unchecked radio retains input background, no mark: {p:?}"
                    );
                }
            })
            .unwrap();
        }
    }
    unmount(cx, probe, window.into());
    app.update(cx, |app, cx| {
        app.set_theme(&old_theme, cx);
        theme::set_zoom(old_zoom);
        cx.notify();
    });
    unmount(cx, app, app_window);
}
