//! Activity period keyboard selection and named chart inspection (#1301/#1302).
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Entity, Role, VisualTestAppContext};
use kagi::ui::{e2e, i18n::Lang, theme, BottomTab, KagiApp};
use kagi_domain::activity::Granularity;

pub(crate) fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}
fn matrix(
    cx: &mut VisualTestAppContext,
    check: impl Fn(&mut VisualTestAppContext, &Entity<KagiApp>, AnyWindowHandle),
) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "ui_zoom", "theme"]);
    let original = (
        kagi_ui_core::i18n::lang(),
        theme::zoom(),
        theme::theme().slug.to_string(),
    );
    let fixture = build_fixture();
    for dimensions in [(940., 660.), (1392., 883.)] {
        for slug in ["apple-light", "apple-dark"] {
            for lang in [Lang::En, Lang::Ja] {
                for zoom in [1., 1.67] {
                    let (app, window) = mount(cx, fixture.path());
                    theme::set_zoom(zoom);
                    app.update(cx, |app, cx| {
                        app.set_lang(lang, cx);
                        app.set_theme(slug, cx);
                        app.bottom_panel_open = true;
                        app.bottom_tab = BottomTab::Activity;
                        app.bottom_panel_height = 180.;
                        cx.notify();
                    });
                    crate::pr_detail_adaptive::resize(cx, window, dimensions);
                    cx.run_until_parked();
                    draw(cx, window);
                    check(cx, &app, window);
                    unmount(cx, app, window);
                }
            }
        }
    }
    kagi_ui_core::i18n::set_lang(original.0);
    theme::set_zoom(original.1);
    theme::set_active(&original.2);
    cx.update(theme::sync_gpui_component_theme);
}
pub(crate) fn focus(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    let node = e2e::activity::node(id).expect("Activity renderer metadata");
    cx.update_window(window, |_, window, cx| {
        node.focus
            .expect("rendered control has a focus handle")
            .focus(window, cx);
    })
    .unwrap();
}
fn period_attributes() {
    let group = e2e::activity::node("activity-periods").expect("named period group");
    assert_eq!(group.role, Some(Role::TabList));
    assert_eq!(group.name, kagi_ui_core::i18n::Msg::ActivityPeriod.t());
    let nodes = e2e::activity::nodes("activity-gran-");
    assert_eq!(nodes.len(), Granularity::ALL.len());
    assert_eq!(nodes.iter().filter(|(_, n)| n.selected).count(), 1);
    assert_eq!(
        nodes.iter().filter(|(_, n)| n.tab_stop).count(),
        1,
        "period group is one Tab stop"
    );
    for (_, node) in nodes {
        assert_eq!(node.role, Some(Role::Tab), "period choice exposes Tab role");
        assert!(node.focus.is_some());
        assert!(!node.name.is_empty());
    }
}
fn bucket_attributes(app: &Entity<KagiApp>, cx: &VisualTestAppContext) {
    let data = cx.read(|cx| app.read(cx).view().activity.get(Granularity::Week).clone());
    let nodes = e2e::activity::nodes("activity-bucket-");
    assert_eq!(nodes.len(), data.buckets.len());
    assert_eq!(
        nodes.iter().filter(|(_, n)| n.tab_stop).count(),
        usize::from(!nodes.is_empty())
    );
    for (i, bucket) in data.buckets.iter().enumerate() {
        let node = e2e::activity::node(&format!("activity-bucket-{i}")).unwrap();
        assert_eq!(node.role, Some(Role::Status), "bucket exposes Status role");
        assert!(node.focus.is_some(), "bucket is keyboard focusable");
        assert_eq!(
            node.name,
            kagi_ui_core::i18n::activity::bucket(
                &data.bucket_date(i),
                bucket.commits,
                bucket.merges,
                bucket.authors
            )
        );
    }
}
fn assert_geometry(window: AnyWindowHandle) {
    let body = e2e::control_bounds(window.window_id(), "activity-body").unwrap();
    for id in [
        "activity-periods",
        "activity-readout",
        "activity-legend-commits",
        "activity-legend-merges",
        "activity-chart",
        "activity-x-axis",
        "activity-y-axis",
    ] {
        let bounds = e2e::control_bounds(window.window_id(), id).unwrap();
        assert!(
            bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.),
            "{id} has nonzero area"
        );
        crate::recovery_layout::contained(body, bounds, id);
    }
}
fn focused_bucket(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> Option<usize> {
    cx.update_window(window, |_, window, _| {
        e2e::activity::nodes("activity-bucket-")
            .iter()
            .find_map(|(id, node)| {
                node.focus
                    .as_ref()
                    .filter(|focus| focus.is_focused(window))
                    .map(|_| id.trim_start_matches("activity-bucket-").parse().unwrap())
            })
    })
    .unwrap()
}
fn inspect(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    count: usize,
) {
    focus(cx, window, "activity-bucket-0");
    for i in 0..count {
        draw(cx, window);
        assert_eq!(focused_bucket(cx, window), Some(i));
        bucket_attributes(app, cx);
        let keyboard = e2e::activity::readout();
        let bounds =
            e2e::control_bounds(window.window_id(), &format!("activity-bucket-{i}")).unwrap();
        cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
        draw(cx, window);
        assert_eq!(
            e2e::activity::readout(),
            keyboard,
            "hover and keyboard share the bucket readout"
        );
        if i + 1 < count {
            crate::keyboard_nav::keys(cx, window, "right");
            draw(cx, window);
            assert_eq!(focused_bucket(cx, window), Some(i + 1));
            let bucket = cx.read(|cx| app.read(cx).view().activity.week.buckets[i + 1].clone());
            assert!(
                e2e::activity::readout().ends_with(&format!(
                    "{} commits, {} merges",
                    bucket.commits, bucket.merges
                )),
                "keyboard inspection wins over a stationary pointer on the previous bucket"
            );
        }
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(0.), gpui::px(0.)),
            None,
            gpui::Modifiers::none(),
        );
    }
    for (key, index) in [
        ("right", count - 1),
        ("home", 0),
        ("left", 0),
        ("end", count - 1),
        ("enter space", count - 1),
        ("up", (count - 1).saturating_sub(1)),
    ] {
        crate::keyboard_nav::keys(cx, window, key);
        draw(cx, window);
        assert_eq!(focused_bucket(cx, window), Some(index));
    }
    for forward in [true, false] {
        focus(cx, window, "activity-bucket-0");
        draw(cx, window);
        cx.update_window(window, |_, window, cx| {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
        })
        .unwrap();
        draw(cx, window);
        assert_eq!(focused_bucket(cx, window), None, "one Tab leaves the chart");
    }
}
pub fn scenario_activity_granularity_keys(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, window| {
        period_attributes();
        focus(cx, window, "activity-gran-Week");
        for (keys, expected) in [
            ("right", Granularity::Month),
            ("right enter", Granularity::Year),
            ("end space", Granularity::All),
            ("home enter", Granularity::Day),
            ("right left", Granularity::Day),
        ] {
            crate::keyboard_nav::keys(cx, window, keys);
            draw(cx, window);
            assert_eq!(cx.read(|cx| app.read(cx).activity_granularity), expected);
            assert_eq!(cx.read(|cx| app.read(cx).activity_hover), None);
            period_attributes();
            let totals = cx.read(|cx| {
                let data = app.read(cx).view().activity.get(expected);
                format!(
                    "{}: {} commits, {} merges",
                    expected.window_label(),
                    data.total_commits,
                    data.total_merges
                )
            });
            assert_eq!(
                e2e::activity::readout(),
                totals,
                "selected period uses its aggregated window"
            );
        }
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            cx.simulate_mouse_move(
                window,
                gpui::point(gpui::px(0.), gpui::px(0.)),
                None,
                gpui::Modifiers::none(),
            );
            let sample = |cx: &mut VisualTestAppContext| {
                cx.update_window(window, |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                    let rect = e2e::control_bounds(
                        window.window_handle().window_id(),
                        "activity-gran-Day",
                    )
                    .unwrap();
                    let image = window
                        .render_to_image()
                        .expect("native selected-fill capture");
                    let scale = window.scale_factor();
                    (
                        rect,
                        image
                            .get_pixel(
                                ((f32::from(rect.left()) + theme::scaled(5.)) * scale) as u32,
                                ((f32::from(rect.top()) + theme::scaled(5.)) * scale) as u32,
                            )
                            .0,
                    )
                })
                .unwrap()
            };
            let rest = sample(cx);
            cx.simulate_mouse_move(window, rest.0.center(), None, gpui::Modifiers::none());
            assert_eq!(
                sample(cx),
                rest,
                "selected period retains fill and bounds on hover"
            );
        }
    });
    eprintln!("[gui-e2e] PASS activity_granularity_keys");
}
pub fn scenario_activity_bucket_a11y(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, window| {
        bucket_attributes(app, cx);
        assert_geometry(window);
        let before = cx.read(|cx| app.read(cx).view().activity.week.clone());
        for count in [24, 1, 0] {
            app.update(cx, |app, cx| {
                let data = &mut app.view_mut().activity.week;
                data.buckets = (0..count)
                    .map(|i| kagi_domain::activity::ActivityBucket {
                        commits: [0, 1, 4, 2][i % 4],
                        merges: [0, 1, 0, 2][i % 4],
                        authors: [0, 1, 2, 1][i % 4],
                    })
                    .collect();
                if count == 1 {
                    data.buckets[0] = kagi_domain::activity::ActivityBucket {
                        commits: 1,
                        merges: 1,
                        authors: 1,
                    };
                }
                data.total_commits = data.buckets.iter().map(|bucket| bucket.commits).sum();
                data.total_merges = data.buckets.iter().map(|bucket| bucket.merges).sum();
                app.activity_hover = None;
                cx.notify();
            });
            draw(cx, window);
            bucket_attributes(app, cx);
            if count > 0 {
                assert_geometry(window);
                for part in ["chart", "legend"] {
                    let commits =
                        e2e::activity::painted_stroke(&format!("{part}-commits")).unwrap();
                    let merges = e2e::activity::painted_stroke(&format!("{part}-merges")).unwrap();
                    assert!(
                        commits.solid_segments > 0 && merges.dash_segments > 0,
                        "series have solid/dashed encodings in chart and legend"
                    );
                    assert!(
                        commits.solid_segments + commits.dash_segments > 0
                            && merges.solid_segments + merges.dash_segments > 0,
                        "single and zero series paint visible strokes"
                    );
                }
                inspect(cx, app, window, count);
            }
        }
        app.update(cx, |app, cx| {
            app.view_mut().activity.week = before;
            cx.notify();
        });
    });
    eprintln!("[gui-e2e] PASS activity_bucket_a11y");
}

fn install_encoding_case(
    app: &Entity<KagiApp>,
    cx: &mut VisualTestAppContext,
    count: usize,
    value: u32,
) {
    app.update(cx, |app, cx| {
        let data = &mut app.view_mut().activity.week;
        data.buckets = (0..count)
            .map(|i| {
                let commits = if count > 1 && i >= 2 { 1 } else { value };
                kagi_domain::activity::ActivityBucket {
                    commits,
                    merges: commits,
                    authors: u32::from(commits > 0),
                }
            })
            .collect();
        data.total_commits = data.buckets.iter().map(|bucket| bucket.commits).sum();
        data.total_merges = data.total_commits;
        app.activity_hover = None;
        cx.notify();
    });
}

fn gap_pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str, zero: bool) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let bounds = e2e::control_bounds(window.window_handle().window_id(), id).unwrap();
        let (x, y) = if id == "activity-chart" {
            (bounds.left() + theme::scaled_px(17.),
                if zero { bounds.bottom() - theme::scaled_px(10.) } else { bounds.top() + theme::scaled_px(10.) })
        } else {
            (bounds.left() + theme::scaled_px(7.), bounds.center().y)
        };
        let image = window.render_to_image().expect("native dash-gap capture");
        let scale = window.scale_factor();
        let pixel = image.get_pixel((f32::from(x) * scale) as u32, (f32::from(y) * scale) as u32).0;
        // Compare nearby unpainted pixels, not the painter's chosen theme token.
        let background_y = if id == "activity-chart" && zero {
            y - theme::scaled_px(4.)
        } else {
            y + theme::scaled_px(4.)
        };
        let expected = image.get_pixel(
            (f32::from(x) * scale) as u32,
            (f32::from(background_y) * scale) as u32,
        ).0;
        for channel in 0..3 {
            assert!(pixel[channel].abs_diff(expected[channel]) <= 2,
                "dash gap matches nearby unpainted background: {id} {pixel:?}, expected {expected:?}");
        }
    }).unwrap();
}

pub fn scenario_activity_equal_series_encoding(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, window| {
        for value in [0, 1] {
            install_encoding_case(app, cx, 4, value);
            draw(cx, window);
            for part in ["chart", "legend"] {
                let painted = e2e::activity::painted_stroke(&format!("{part}-merges")).unwrap();
                assert!(
                    painted.knockout_segments > 0,
                    "coincident series paint actual knockout gaps"
                );
                assert_eq!(painted.knockout_segments, painted.gap_segments);
            }
            gap_pixel(cx, window, "activity-chart", value == 0);
            gap_pixel(cx, window, "activity-legend-merges", false);
        }
    });
    eprintln!("[gui-e2e] PASS activity_equal_series_encoding");
}

pub fn scenario_activity_single_bucket_encoding(cx: &mut VisualTestAppContext) {
    matrix(cx, |cx, app, window| {
        install_encoding_case(app, cx, 1, 1);
        draw(cx, window);
        let painted = e2e::activity::painted_stroke("chart-merges").unwrap();
        assert!(painted.single_point);
        assert!(
            painted.dash_segments >= 2,
            "single merge mark paints two actual dashes"
        );
        assert_eq!(
            painted.solid_segments, 0,
            "merge fallback is never a solid mark"
        );
        assert_eq!(painted.gap_segments, 1);
        assert_eq!(painted.knockout_segments, 1);
    });
    eprintln!("[gui-e2e] PASS activity_single_bucket_encoding");
}
