//! #1274: native Metal pixels of the real floating surfaces, with fixed RGB oracles.
use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::toolbar_overflow::ToolbarRestore;
use gpui::{point, px, size, AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, BottomTab, KagiApp};
use kagi_ui_core::{settings, theme};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

// Intentionally independent of Theme: running against the old paints must fail.
const ELEVATED_DARK: u32 = 0x2a2a2c;

#[derive(Clone, Copy, Debug)]
enum Surface {
    Palette,
    Modal,
    Menu,
    Popover,
    Toast,
}

impl Surface {
    fn probe(self) -> &'static str {
        match self {
            Self::Palette => "palette-card",
            Self::Modal => "modal-card",
            Self::Menu => "branch-context-menu",
            Self::Popover => "tb-overflow-menu",
            Self::Toast => "toast-card",
        }
    }

    fn light_rgb(self) -> u32 {
        match self {
            Self::Palette | Self::Modal | Self::Toast => 0xf7f7f7,
            Self::Menu | Self::Popover => 0xffffff,
        }
    }
}

struct MotionRestore {
    before: bool,
    _saved: crate::gui_isolation::SavedKeys,
}

impl MotionRestore {
    fn capture() -> Self {
        let restore = Self {
            before: theme::reduce_motion(),
            _saved: crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]),
        };
        theme::set_reduce_motion(true);
        restore
    }
}

impl Drop for MotionRestore {
    fn drop(&mut self) {
        theme::set_reduce_motion(self.before);
    }
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn open_surface(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    surface: Surface,
    head: &str,
) {
    match surface {
        Surface::Palette => {
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| app.open_command_palette(window, cx));
            })
            .unwrap();
        }
        Surface::Modal => app.update(cx, |app, cx| {
            app.open_create_branch_modal(kagi_git::CommitId(head.into()), cx);
        }),
        Surface::Menu => app.update(cx, |app, cx| {
            app.open_local_branch_menu("main".into(), point(px(400.), px(100.)));
            cx.notify();
        }),
        Surface::Popover => {
            let trigger = e2e::control_bounds(window.window_id(), "tb-overflow")
                .expect("narrow toolbar draws the real » trigger");
            cx.simulate_mouse_move(window, trigger.center(), None, Modifiers::none());
            cx.simulate_click(window, trigger.center(), Modifiers::none());
            cx.simulate_mouse_move(window, point(px(1.), px(100.)), None, Modifiers::none());
        }
        Surface::Toast => app.update(cx, |app, cx| {
            kagi::ui::branch_menu::copy_branch_name(app, "main".into(), cx);
        }),
    }
    paint(cx, window);
}

fn assert_fill(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    probe: &str,
    inset: f32,
    expected: u32,
    label: &str,
) {
    let bounds = e2e::control_bounds(window.window_id(), probe)
        .unwrap_or_else(|| panic!("{label}: missing rendered {probe} bounds"));
    assert!(
        bounds.size.width > px(40.) && bounds.size.height > px(12.),
        "{label}: {bounds:?}"
    );
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native elevated-surface Metal capture");
        let scale = window.scale_factor();
        // Top-centre padding: no label, input, selection, border, corner or shadow.
        // PopupMenu's p_1 is only 3.6px at 100%; use its narrower 2px safe band.
        for dx in [-3., 0., 3.] {
            let x = ((f32::from(bounds.center().x) + dx) * scale).floor() as u32;
            for dy in [0., 0.5, 1.] {
                let y = ((f32::from(bounds.top()) + inset + dy) * scale).floor() as u32;
                let pixel = image.get_pixel(x, y).0;
                let actual = (u32::from(pixel[0]) << 16)
                    | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
                eprintln!("[gui-e2e] {label}: {probe} ({x},{y}) RGB=#{actual:06x} expected=#{expected:06x}");
                assert_eq!(actual, expected, "{label}: {probe} native interior ({x},{y}), bounds={bounds:?}, scale={scale}");
                assert_eq!(pixel[3], 255, "{label}: elevated interior must be opaque");
            }
        }
    })
    .unwrap();
}

fn scenario_surface(cx: &mut VisualTestAppContext, surface: Surface) {
    let _presentation = ToolbarRestore::capture(cx);
    let _motion = MotionRestore::capture();
    let fixture = build_fixture();
    let before = repo_fingerprint(fixture.path());
    settings::set_toolbar_display(settings::ToolbarDisplay::IconAndText);
    for slug in ["apple-light", "apple-dark"] {
        theme::set_zoom(1.);
        let (app, window) = mount(cx, fixture.path());
        let result = catch_unwind(AssertUnwindSafe(|| {
            app.update(cx, |app, cx| app.set_theme(slug, cx));
            if matches!(surface, Surface::Popover) {
                // Same constrained size/zoom as the existing toolbar oracle.
                theme::set_zoom(1.67);
                cx.update_window(window, |_, window, _| {
                    window.resize(size(px(940.), px(660.)))
                })
                .unwrap();
                crate::macos::drain_native_events();
            }
            for _ in 0..5 {
                paint(cx, window);
            }
            open_surface(cx, &app, window, surface, &before.0);
            let probe = surface.probe();
            let expected = if slug == "apple-dark" {
                ELEVATED_DARK
            } else {
                surface.light_rgb()
            };
            let inset = if matches!(surface, Surface::Palette | Surface::Popover) {
                2.
            } else {
                4.
            };
            assert_fill(
                cx,
                window,
                probe,
                inset,
                expected,
                &format!("{surface:?}/{slug}"),
            );
            cx.simulate_keystrokes(window, "escape");
            paint(cx, window);
        }));
        // The runner's panic hook owns failure evidence and closes failed windows.
        if let Err(panic) = result {
            resume_unwind(panic);
        }
        unmount(cx, app, window);
        assert_eq!(
            repo_fingerprint(fixture.path()),
            before,
            "surface capture never writes Git"
        );
    }
}

pub fn scenario_dark_elevation_palette(cx: &mut VisualTestAppContext) {
    scenario_surface(cx, Surface::Palette);
}

pub fn scenario_dark_elevation_modal(cx: &mut VisualTestAppContext) {
    scenario_surface(cx, Surface::Modal);
}

pub fn scenario_dark_elevation_menu(cx: &mut VisualTestAppContext) {
    scenario_surface(cx, Surface::Menu);
}

pub fn scenario_dark_elevation_popover(cx: &mut VisualTestAppContext) {
    scenario_surface(cx, Surface::Popover);
}

pub fn scenario_dark_elevation_toast(cx: &mut VisualTestAppContext) {
    scenario_surface(cx, Surface::Toast);
}

fn bounds(window: AnyWindowHandle, probe: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), probe)
        .unwrap_or_else(|| panic!("missing native chrome probe: {probe}"))
}

fn assert_points(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    samples: &[(gpui::Point<gpui::Pixels>, u32)],
    label: &str,
) {
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native chrome Metal capture");
        let scale = window.scale_factor();
        for &(point, expected) in samples {
            let x = (f32::from(point.x) * scale).floor() as u32;
            let y = (f32::from(point.y) * scale).floor() as u32;
            let pixel = image.get_pixel(x, y).0;
            let actual =
                (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
            eprintln!("[gui-e2e] {label}: ({x},{y}) RGB=#{actual:06x} expected=#{expected:06x}");
            assert_eq!(
                actual, expected,
                "{label}: native RGB at {point:?}, scale={scale}"
            );
            assert_eq!(pixel[3], 255, "{label}: opaque native fill");
        }
    })
    .unwrap();
}

fn assert_chrome_edge(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    probe: &str,
    x: gpui::Pixels,
    top: bool,
    outside: u32,
) {
    let rect = bounds(window, probe);
    let scale = cx
        .update_window(window, |_, window, _| window.scale_factor())
        .unwrap();
    let half_pixel = px(0.5 / scale);
    // Absolute size_full probes span the padding box, excluding the border.
    let start = if top {
        rect.top() - px(1.)
    } else {
        rect.bottom()
    };
    let (above, below) = if top {
        (outside, 0x1c1c1e)
    } else {
        (0x1c1c1e, outside)
    };
    // First/last physical pixels inside a literal 1 logical-px line, then
    // immediate neighbours: a thicker border or absent line cannot pass.
    let samples = [
        (point(x, start - half_pixel), above),
        (point(x, start + half_pixel), 0x2c2c2e),
        (point(x, start + px(1.) - half_pixel), 0x2c2c2e),
        (point(x, start + px(1.) + half_pixel), below),
    ];
    assert_points(cx, window, &samples, &format!("{probe}/1px edge"));
}

fn chrome_workspace(cx: &mut VisualTestAppContext, window: AnyWindowHandle, slug: &str) {
    let chrome = if slug == "apple-dark" {
        0x1c1c1e
    } else {
        0xf7f7f7
    };
    let base = if slug == "apple-dark" {
        0x101012
    } else {
        0xffffff
    };
    let panel = if slug == "apple-dark" {
        0x161618
    } else {
        0xf7f7f7
    };
    let sidebar = if slug == "apple-dark" {
        0x101012
    } else {
        0xf7f7f7
    };
    let drag = bounds(window, "toolbar-drag-region");
    let status = bounds(window, "pr-menu-footer");
    let tabs = bounds(window, "bottom-panel-tabs");
    let body = bounds(window, "bottom-panel");
    let list = bounds(window, "commit-list-viewport");
    let side = bounds(window, "sidebar-clip");
    let mut samples = vec![
        (drag.center(), chrome),
        (point(status.right() - px(3.), status.center().y), chrome),
        (point(tabs.right() - px(8.), tabs.center().y), chrome),
        (
            point(body.right() - px(12.), body.bottom() - px(12.)),
            panel,
        ),
        (point(list.right() - px(20.), list.bottom() - px(8.)), base),
        (
            point(side.left() + px(3.), side.bottom() - px(20.)),
            sidebar,
        ),
    ];
    let strip = bounds(window, "repo-tab-strip");
    samples.push((point(strip.right() - px(8.), strip.center().y), chrome));
    assert_points(cx, window, &samples, &format!("workspace/{slug}"));
    if slug != "apple-dark" {
        return; // Light fills are pinned; its existing border geometry is unchanged.
    }
    // The toolbar's left edge now meets the sidebar, not a tab strip.
    assert_chrome_edge(cx, window, "toolbar-row", px(3.), false, sidebar);
    assert_chrome_edge(cx, window, "pr-menu-footer", px(3.), true, sidebar);
    assert_chrome_edge(
        cx,
        window,
        "bottom-panel-tabs",
        tabs.right() - px(8.),
        false,
        panel,
    );
    assert_chrome_edge(
        cx,
        window,
        "repo-tab-strip",
        strip.right() - px(8.),
        false,
        chrome,
    );
}

fn chrome_home(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    slug: &str,
) {
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    })
    .unwrap();
    paint(cx, window);
    let drag = bounds(window, "home-drag-region");
    let expected = if slug == "apple-dark" {
        0x1c1c1e
    } else {
        0xf7f7f7
    };
    assert_points(
        cx,
        window,
        &[(drag.center(), expected)],
        &format!("Home/{slug}"),
    );
    if slug == "apple-dark" {
        // Home's document titlebar is above the frame; below it is base content.
        let frame = bounds(window, "home-frame");
        assert_chrome_edge(
            cx,
            window,
            "home-frame",
            frame.right() - px(8.),
            false,
            0x101012,
        );
    }
}

pub fn scenario_dark_elevation_chrome(cx: &mut VisualTestAppContext) {
    let _presentation = ToolbarRestore::capture(cx);
    let _motion = MotionRestore::capture();
    let first = build_fixture();
    let second = build_fixture();
    let before = [
        repo_fingerprint(first.path()),
        repo_fingerprint(second.path()),
    ];
    settings::set_toolbar_display(settings::ToolbarDisplay::IconOnly);
    theme::set_zoom(1.);
    for slug in ["apple-light", "apple-dark"] {
        let (app, window) = mount(cx, first.path());
        let result = catch_unwind(AssertUnwindSafe(|| {
            app.update(cx, |app, cx| {
                app.set_theme(slug, cx);
                app.bottom_panel_open = true;
                app.bottom_tab = BottomTab::OperationLog;
                cx.notify();
            });
            for _ in 0..5 {
                paint(cx, window);
            }
            chrome_workspace(cx, window, slug);
            app.update(cx, |app, cx| {
                assert!(app.open_repository(second.path().to_path_buf(), cx));
            });
            for _ in 0..5 {
                paint(cx, window);
            }
            chrome_workspace(cx, window, slug);
            chrome_home(cx, &app, window, slug);
        }));
        if let Err(panic) = result {
            resume_unwind(panic);
        }
        unmount(cx, app, window);
        assert_eq!(
            [
                repo_fingerprint(first.path()),
                repo_fingerprint(second.path())
            ],
            before,
            "chrome capture never writes Git"
        );
    }
}
