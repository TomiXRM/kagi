//! #1199 / #1201: neutral Settings hierarchy and circular radio mark.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{rgb, FontWeight, VisualTestAppContext};
use kagi::ui::{e2e, theme};

pub(crate) fn luminance(color: u32) -> f64 {
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
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "ui_zoom",
        "lang",
        "settings_pane",
        "accent_source",
    ]);
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _host = crate::settings_window::SettingsHostGuard::portable(cx);
    kagi::ui::settings::write_setting("settings_pane", Some("general"));
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    for slug in ["apple-light", "apple-dark"] {
        for lang in [kagi::ui::i18n::Lang::En, kagi::ui::i18n::Lang::Ja] {
            for accent in [theme::SystemAccent::Blue, theme::SystemAccent::Purple] {
                let _accent = theme::AccentInjection::new(accent, None);
                for zoom in [1.0, 1.67] {
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                        theme::set_follows_system_accent(true);
                        theme::set_zoom(zoom);
                        theme::sync_gpui_component_theme(cx);
                        app.seed_settings(cx);
                        cx.notify();
                    });
                    cx.update_window(window, |_, window, cx| {
                        app.update(cx, |app, cx| {
                            app.handle_menu_command("app.settings", window, cx)
                        });
                    })
                    .unwrap();
                    crate::settings_controls::draw(cx, window);
                    crate::settings_controls::click(cx, window, "settings-pane-general");
                    let palette = theme::theme();
                    // Independent PR1814 role oracle, not production kit helpers.
                    let window_fill = if palette.dark {
                        palette.bg_base
                    } else {
                        palette.panel
                    };
                    let card_fill = if palette.dark {
                        fidelity_mix(palette.bg_base, palette.surface, 0.42)
                    } else {
                        palette.bg_base
                    };
                    let sidebar_fill = fidelity_mix(
                        window_fill,
                        palette.surface,
                        if palette.dark { 0.22 } else { 0.35 },
                    );
                    let heading =
                        crate::settings_controls::probe(cx, window, "settings-pane-title")
                            .expect("actual shared pane title is painted");
                    assert_eq!(heading.role, gpui::Role::Heading);
                    assert_eq!(heading.name, kagi_ui_settings::Pane::General.title());
                    let paint = heading
                        .paint
                        .expect("title records its resolved text style");
                    assert_eq!(
                        paint.text_color,
                        rgb(palette.text_main).into(),
                        "neutral primary title"
                    );
                    assert_eq!(paint.font_weight, FontWeight::BOLD);
                    assert_eq!(
                        paint.font_size,
                        theme::scaled_px(15. * 16. / theme::BASE_REM_PX)
                    );
                    let (title, panel) = (luminance(palette.text_main), luminance(window_fill));
                    assert!((title.max(panel) + 0.05) / (title.min(panel) + 0.05) >= 4.5);
                    // Descriptions sit on the group fill; section content sits
                    // directly on the pane, without the removed nested cards.
                    for background in [card_fill, window_fill] {
                        let (label, fill) = (luminance(palette.text_sub), luminance(background));
                        assert!(
                            (label.max(fill) + 0.05) / (label.min(fill) + 0.05) >= 4.5,
                            "secondary Settings text remains readable on {background:06x}"
                        );
                    }
                    let sidebar =
                        crate::settings_controls::bounds(cx, window, "settings-sidebar").unwrap();
                    let detail =
                        crate::settings_controls::bounds(cx, window, "settings-detail-scroll")
                            .unwrap();
                    let group =
                        crate::settings_controls::bounds(cx, window, "settings-language-group")
                            .unwrap();
                    crate::recovery_layout::contained(
                        detail,
                        group,
                        "language row group fits detail pane",
                    );
                    assert!(
                        sidebar.right() <= detail.left(),
                        "navigation and detail have distinct columns"
                    );
                    for pane in kagi_ui_settings::Pane::ALL {
                        let probe = crate::settings_controls::probe(
                            cx,
                            window,
                            &format!("settings-pane-{}", pane.id()),
                        )
                        .unwrap();
                        assert_eq!(probe.role, gpui::Role::Tab);
                        assert_eq!(probe.name, pane.title());
                        assert_eq!(probe.selected, pane == kagi_ui_settings::Pane::General);
                        let tile = crate::settings_controls::bounds(
                            cx,
                            window,
                            &format!("settings-pane-tile-{}", pane.id()),
                        )
                        .expect("actual category tile");
                        let expected = gpui::px(20. * zoom);
                        assert!((tile.size.width - expected).abs() <= gpui::px(0.5));
                        assert!((tile.size.height - expected).abs() <= gpui::px(0.5));
                    }
                    // Sample interiors away from text, rounded corners and
                    // focus rings. The new hierarchy has neutral surfaces, not
                    // the old heading/titlebar hairlines or floating-card frame.
                    let samples = [
                        (
                            gpui::point(
                                sidebar.left() + gpui::px(2.),
                                sidebar.top() + gpui::px(2.),
                            ),
                            sidebar_fill,
                        ),
                        (
                            gpui::point(detail.left() + gpui::px(2.), detail.center().y),
                            window_fill,
                        ),
                        (
                            gpui::point(group.left() + gpui::px(2.), group.center().y),
                            card_fill,
                        ),
                    ];
                    cx.update_window(window, |_, window, _| {
                        let image = window
                            .render_to_image()
                            .expect("shared Settings native image");
                        let scale = window.scale_factor();
                        for (point, color) in samples {
                            let pixel = image
                                .get_pixel(
                                    (f32::from(point.x) * scale) as u32,
                                    (f32::from(point.y) * scale) as u32,
                                )
                                .0;
                            for (got, expected) in pixel[..3].iter().zip([
                                (color >> 16) as u8,
                                (color >> 8) as u8,
                                color as u8,
                            ]) {
                                assert!(
                                    got.abs_diff(expected) <= 2,
                                    "neutral surface {pixel:?}, expected {color:06x} at {point:?}"
                                );
                            }
                        }
                    })
                    .unwrap();
                    // Keep the pointer over the selected control. A neutral
                    // hover fill must not erase selection on same-height panes.
                    for pane in kagi_ui_settings::Pane::ALL {
                        let id = format!("settings-pane-{}", pane.id());
                        crate::settings_controls::click(cx, window, &id);
                        assert!(
                            crate::settings_controls::probe(cx, window, &id)
                                .unwrap()
                                .selected
                        );
                        assert_hover_fill(cx, window, &id, palette.selected, zoom);
                    }
                    let language = if lang == kagi::ui::i18n::Lang::En {
                        "settings-language-en"
                    } else {
                        "settings-language-ja"
                    };
                    // Independent approved raised-segment colors, not the
                    // production helper that formerly returned inactive gray.
                    assert_hover_fill(
                        cx,
                        window,
                        language,
                        if slug == "apple-light" {
                            0xffffff
                        } else {
                            0x66666c
                        },
                        zoom,
                    );
                    let hit = crate::settings_controls::bounds(cx, window, language).unwrap();
                    let selected_label =
                        crate::settings_controls::probe(cx, window, &format!("{language}-label"))
                            .expect("selected segment label is the actual production text");
                    assert_eq!(
                        selected_label
                            .paint
                            .expect("resolved selected text")
                            .font_weight,
                        FontWeight::MEDIUM,
                    );
                    let other_language = if lang == kagi::ui::i18n::Lang::En {
                        "ja"
                    } else {
                        "en"
                    };
                    let unselected_label = crate::settings_controls::probe(
                        cx,
                        window,
                        &format!("settings-language-{other_language}-label"),
                    )
                    .expect("unselected segment label");
                    assert_eq!(
                        unselected_label
                            .paint
                            .expect("resolved unselected text")
                            .font_weight,
                        FontWeight::NORMAL,
                    );
                    let track =
                        crate::settings_controls::bounds(cx, window, "settings-language-visual")
                            .unwrap();
                    let pill =
                        crate::settings_controls::bounds(cx, window, &format!("{language}-visual"))
                            .unwrap();
                    assert!(
                        hit.size.height >= gpui::px(28. * zoom - 0.5),
                        "segment hit target remains at least 28 points: {hit:?}"
                    );
                    for (bounds, height, inset, name) in
                        [(track, 24., 2., "track"), (pill, 20., 4., "pill")]
                    {
                        if zoom == 1. {
                            assert!(
                                (bounds.size.height - gpui::px(height)).abs() <= gpui::px(0.5),
                                "normal one-line segment {name} is {height} points: {bounds:?}"
                            );
                        }
                        // GPUI rounds absolute near/far edges independently.
                        for gap in [bounds.top() - hit.top(), hit.bottom() - bounds.bottom()] {
                            assert!(
                                (gap - gpui::px(inset * zoom)).abs() <= gpui::px(0.5),
                                "segment {name} inset is {inset} scaled points: {bounds:?}, hit={hit:?}"
                            );
                        }
                        assert!(
                            bounds.top() >= hit.top() && bounds.bottom() <= hit.bottom(),
                            "segment {name} stays within the larger hit target"
                        );
                    }
                    app.update(cx, |app, cx| {
                        app.menu_overlay = None;
                        cx.notify();
                    });
                }
            }
        }
    }
    unmount(cx, app, window);
}

fn fidelity_mix(a: u32, b: u32, amount: f32) -> u32 {
    let channel = |shift| {
        let a = ((a >> shift) & 255) as f32;
        let b = ((b >> shift) & 255) as f32;
        (a + (b - a) * amount).round() as u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn assert_hover_fill(
    cx: &mut VisualTestAppContext,
    window: gpui::AnyWindowHandle,
    id: &str,
    expected: u32,
    zoom: f32,
) {
    let bounds = crate::settings_controls::bounds(cx, window, id).unwrap();
    cx.simulate_event(
        window,
        gpui::MouseMoveEvent {
            position: bounds.center(),
            ..Default::default()
        },
    );
    crate::settings_controls::draw(cx, window);
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("selected hovered control");
        let scale = window.scale_factor();
        // Inside the leading padding, away from text, glyphs and focus rings.
        let pixel = image
            .get_pixel(
                ((f32::from(bounds.left()) + 3. * zoom) * scale) as u32,
                (f32::from(bounds.center().y) * scale) as u32,
            )
            .0;
        for (got, want) in pixel[..3].iter().zip([
            (expected >> 16) as u8,
            (expected >> 8) as u8,
            expected as u8,
        ]) {
            assert!(
                got.abs_diff(want) <= 2,
                "{id} selected hover fill {pixel:?}, expected {expected:06x}"
            );
        }
    })
    .unwrap();
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
    let _saved =
        crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom", "accent_source", "accent"]);
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
        for source in [
            None,
            Some(theme::SystemAccent::Blue),
            Some(theme::SystemAccent::Purple),
        ] {
            let _accent =
                theme::AccentInjection::new(source.unwrap_or(theme::SystemAccent::Blue), None);
            for zoom in [1.0, 1.67] {
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    theme::set_zoom(zoom);
                    theme::set_follows_system_accent(source.is_some());
                    theme::sync_gpui_component_theme(cx);
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
                    use gpui_component::ActiveTheme as _;
                    let foreground: gpui::Rgba = cx.theme().primary_foreground.into();
                    let dot = [foreground.r, foreground.g, foreground.b]
                        .map(|c| (c * 255.).round() as u8);
                    let dot_rgb =
                        (u32::from(dot[0]) << 16) | (u32::from(dot[1]) << 8) | u32::from(dot[2]);
                    let (dot_luma, fill_luma) =
                        (luminance(dot_rgb), luminance(theme::theme().ui_accent()));
                    // The selected dot is a non-text state indicator (#1468).
                    assert!(
                        (dot_luma.max(fill_luma) + 0.05) / (dot_luma.min(fill_luma) + 0.05) >= 3.
                    );
                    if slug == "apple-dark" && source.is_none() {
                        assert!(
                            dot.iter().all(|c| *c < 128),
                            "yellow theme accent requires a dark dot"
                        );
                    }
                    // A round mark is symmetrical in the contrasting primary foreground;
                    // the old checkbox SVG and hard-coded white dot both fail this oracle.
                    for (x, y) in [(0., 0.), (-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
                        let p = sample(checked, x, y);
                        assert!(
                            p[..3]
                                .iter()
                                .zip(dot)
                                .all(|(got, want)| got.abs_diff(want) <= 2),
                            "radio dot ({x},{y}): {p:?}, expected {dot:?}"
                        );
                    }
                    for (x, y) in [(-4., 0.), (4., 0.), (0., -4.), (0., 4.)] {
                        let p = sample(checked, x, y);
                        let color = theme::theme().ui_accent();
                        for (got, want) in p[..3].iter().zip([
                            (color >> 16) as u8,
                            (color >> 8) as u8,
                            color as u8,
                        ]) {
                            assert!(
                                got.abs_diff(want) <= 2,
                                "outside dot must be accent, not check path: {p:?}"
                            );
                        }
                    }
                    // Unchecked controls retain their independent input-background role.
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
    }
    unmount(cx, probe, window.into());
    app.update(cx, |app, cx| {
        app.set_theme(&old_theme, cx);
        theme::set_zoom(old_zoom);
        cx.notify();
    });
    unmount(cx, app, app_window);
}
