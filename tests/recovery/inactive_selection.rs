//! #1176: native selected rows/tabs paint neutral chrome without losing selection.
use crate::app_conflict::content_fixture;
use crate::macos::{mount, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::theme;

fn paint(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    name: &str,
) -> ([u8; 4], gpui::Bounds<gpui::Pixels>) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let bounds = e2e::control_bounds(window.window_handle().window_id(), name)
            .expect("selected control drawn");
        let image = window
            .render_to_image()
            .expect("native selection paint capture");
        let scale = window.scale_factor();
        // Below the accent bar and above the label; inside the square tab fill.
        let x = ((f32::from(bounds.left()) + 10.) * scale) as u32;
        let y = ((f32::from(bounds.top()) + 5.) * scale) as u32;
        (image.get_pixel(x, y).0, bounds)
    })
    .unwrap()
}

pub fn scenario_inactive_selection(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let old_theme = theme::theme().slug.to_string();
    let fixture = content_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(std::path::Path::new("file.txt"));
        cx.notify();
    });
    app.update(cx, |app, cx| {
        app.set_theme("apple-light", cx);
        app.set_window_active_for_e2e(true, cx);
    });
    cx.run_until_parked();
    let selected = cx.read(|cx| app.read(cx).active_session());
    let selected_file = cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().selected_file);
    for name in ["repo-tab-0", "conflict-row-0"] {
        app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
        cx.run_until_parked();
        let (active, bounds) = paint(cx, window, name);
        app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
        cx.run_until_parked();
        let (inactive, inactive_bounds) = paint(cx, window, name);
        assert_eq!(
            inactive_bounds, bounds,
            "activation must not change geometry"
        );
        assert_eq!(cx.read(|cx| app.read(cx).active_session()), selected);
        assert_eq!(
            cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().selected_file),
            selected_file
        );
        if name == "repo-tab-0" {
            assert_eq!(active, inactive, "document tab selection stays neutral");
            assert_eq!(
                &inactive[..3],
                &[255, 255, 255],
                "selected tab content surface"
            );
        } else {
            assert_ne!(
                active, inactive,
                "{name}: non-key selection must lose its blue tint"
            );
            assert_eq!(
                &inactive[..3],
                &[211, 211, 211],
                "{name}: actual inactive fill must be #d3d3d3"
            );
        }
        app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
        cx.run_until_parked();
        assert_eq!(
            paint(cx, window, name).0,
            active,
            "reactivation restores the same accent fill"
        );
    }
    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
    drop(conflict);
    unmount(cx, app, window);
}

/// #1180: document tabs stay neutral in both activation states, without a rail.
pub fn scenario_repo_tabs_neutral(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "ui_zoom"]);
    let old_theme = theme::theme().slug.to_string();
    let old_zoom = theme::zoom();
    let fixtures: Vec<_> = (0..6).map(|_| crate::macos::build_fixture()).collect();
    let linked = fixtures[5].path().join("linked");
    crate::macos::git(
        fixtures[0].path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "native-tab-worktree",
            linked.to_str().unwrap(),
        ],
    );
    let (app, window) = mount(cx, fixtures[0].path());
    for count in [1, 2, 6] {
        while app.read_with(cx, |app, _| app.tabs.len()) < count {
            let index = app.read_with(cx, |app, _| app.tabs.len());
            app.update(cx, |app, cx| {
                let path = if index == 5 {
                    &linked
                } else {
                    fixtures[index].path()
                };
                assert!(app.open_repository(path.to_path_buf(), cx));
            });
            cx.run_until_parked();
        }
        if count == 6 {
            assert!(
                app.read_with(cx, |app, _| app.tabs[5].is_worktree),
                "real linked-worktree marker retained"
            );
        }
        app.update(cx, |app, cx| {
            for tab in &mut app.tabs {
                tab.name = "日本語の非常に長いリポジトリ名と作業ツリーの名前".into();
            }
            app.switch_repo(0, cx);
        });
        cx.run_until_parked();
        for slug in ["apple-light", "apple-dark"] {
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for active in [true, false] {
                    app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
                    cx.run_until_parked();
                    for index in 0..count {
                        let name = format!("repo-tab-{index}");
                        let expected = if index == 0 {
                            if slug == "apple-dark" {
                                theme::theme().surface
                            } else {
                                theme::theme().bg_base
                            }
                        } else {
                            if slug == "apple-light" {
                                theme::theme().surface
                            } else {
                                theme::theme().panel
                            }
                        };
                        let (pixel, bounds) = paint(cx, window, &name);
                        let luminance = |color: u32| {
                            let channel = |shift: u32| {
                                let value = ((color >> shift) & 255u32) as f64 / 255.;
                                if value <= 0.04045 {
                                    value / 12.92
                                } else {
                                    ((value + 0.055) / 1.055).powf(2.4)
                                }
                            };
                            0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
                        };
                        let text = luminance(theme::theme().text_main);
                        let background = luminance(expected);
                        assert!(
                            (text.max(background) + 0.05) / (text.min(background) + 0.05) >= 4.5
                        );
                        assert_eq!(
                            &pixel[..3],
                            &[
                                (expected >> 16) as u8,
                                (expected >> 8) as u8,
                                expected as u8
                            ],
                            "{slug}/{zoom}/{active}/{count}/{index}: neutral tab fill"
                        );
                        cx.update_window(window, |_, window, _cx| {
                            let image = window.render_to_image().unwrap();
                            let scale = window.scale_factor();
                            let x = ((f32::from(bounds.left()) + 10.) * scale) as u32;
                            // The old absolute accent overlay occupies the first two px.
                            let y = ((f32::from(bounds.top()) + 0.5) * scale) as u32;
                            assert_eq!(
                                &image.get_pixel(x, y).0[..3],
                                &pixel[..3],
                                "no accent rail"
                            );
                            let paint =
                                e2e::control_paint(window.window_handle().window_id(), &name)
                                    .unwrap();
                            assert_eq!(
                                paint.text_color,
                                gpui::Hsla::from(gpui::rgb(theme::theme().text_main))
                            );
                            assert!(
                                f32::from(bounds.size.width)
                                    <= f32::from(theme::scaled_px(200.)) + 1.
                            );
                        })
                        .unwrap();
                    }
                }
            }
        }
    }
    theme::set_zoom(old_zoom);
    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
    unmount(cx, app, window);
}
