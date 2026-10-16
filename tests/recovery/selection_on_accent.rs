//! #1468: Finder-style selected rows and accent labels, measured at native paint.
#[path = "selection_on_accent_surfaces.rs"]
mod surfaces;

use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, Focusable, Hsla, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::theme;

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn rgb_pixel(cx: &mut VisualTestAppContext, window: AnyWindowHandle, x: f32, y: f32) -> u32 {
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native on-accent capture");
        let scale = window.scale_factor();
        let p = image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
        ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32
    })
    .unwrap()
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

fn assert_label_pixels(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    bounds: gpui::Bounds<gpui::Pixels>,
    foreground: u32,
) {
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native label capture");
        let scale = window.scale_factor();
        let mut matching = 0;
        let mut nearest = (u16::MAX, [0u8; 3]);
        for y in
            (f32::from(bounds.top()) * scale) as u32..(f32::from(bounds.bottom()) * scale) as u32
        {
            for x in (f32::from(bounds.left()) * scale) as u32
                ..(f32::from(bounds.right()) * scale) as u32
            {
                let p = image.get_pixel(x, y).0;
                let distance: u16 = p[..3].iter().zip([
                    (foreground >> 16) as u8, (foreground >> 8) as u8, foreground as u8
                ]).map(|(a, b)| u16::from(a.abs_diff(b))).sum();
                if distance < nearest.0 {
                    nearest = (distance, [p[0], p[1], p[2]]);
                }
                if p[..3]
                    .iter()
                    .zip([
                        (foreground >> 16) as u8,
                        (foreground >> 8) as u8,
                        foreground as u8,
                    ])
                    .all(|(a, b)| a.abs_diff(b) <= 2)
                {
                    matching += 1;
                }
            }
        }
        if matching < 3 {
            let path = crate::gui_evidence::evidence_dir("selection_on_accent")
                .with_extension("label.png");
            std::fs::create_dir_all(path.parent().unwrap()).expect("create native evidence directory");
            image.save(path).expect("save native label failure evidence");
        }
        assert!(
            matching >= 3,
            "painted label must contain foreground #{foreground:06x}, got {matching} pixels; nearest={nearest:?}, bounds={bounds:?}"
        );
    })
    .unwrap();
}

fn assert_neutral_segment(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
    label_id: &str,
) {
    let bounds = e2e::control_bounds(window.window_id(), id).expect("selected segment");
    let fill = rgb_pixel(
        cx,
        window,
        f32::from(bounds.left()) + 3.,
        f32::from(bounds.center().y),
    );
    assert_ne!(
        fill,
        theme::theme().ui_accent(),
        "segments are not row accent"
    );
    assert_eq!(
        fill,
        theme::theme().inactive_selected,
        "neutral Apple segment"
    );
    assert_eq!(
        e2e::control_paint(window.window_id(), label_id)
            .unwrap()
            .text_color,
        Hsla::from(gpui::rgb(theme::theme().text_main)),
        "selected segments retain ordinary foreground"
    );
    let label = e2e::control_bounds(window.window_id(), label_id).unwrap();
    assert_label_pixels(cx, window, label, theme::theme().text_main);
}

fn bounds(window: AnyWindowHandle, id: &str) -> gpui::Bounds<gpui::Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing actual {id}"))
}

fn assert_paint(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    id: &str,
    foreground: Hsla,
    native_ink: u32,
) {
    assert_eq!(
        e2e::control_paint(window.window_id(), id)
            .unwrap()
            .text_color,
        foreground,
        "{id}: actual inherited foreground"
    );
    assert_label_pixels(cx, window, bounds(window, id), native_ink);
}

fn composite(foreground: u32, alpha: f32, background: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |rgb, shift| {
        let fg = ((foreground >> shift) & 255) as f32;
        let bg = ((background >> shift) & 255) as f32;
        rgb | (((fg * alpha + bg * (1. - alpha)).round() as u32) << shift)
    })
}

fn fill(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> u32 {
    let row = bounds(window, id);
    rgb_pixel(
        cx,
        window,
        f32::from(row.right()) - 4.,
        f32::from(row.center().y),
    )
}

fn move_to(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: Option<&str>) {
    let at = id.map_or(gpui::point(gpui::px(1.), gpui::px(1.)), |id| {
        bounds(window, id).center()
    });
    cx.simulate_mouse_move(window, at, None, Modifiers::none());
    draw(cx, window);
}

fn non_apple_slug() -> String {
    theme::THEMES
        .iter()
        .find(|preset| !matches!(&*preset.slug, "apple-light" | "apple-dark"))
        .unwrap()
        .slug
        .to_string()
}

fn menu_legs(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    use crate::keyboard_nav::keys;
    const ENABLED: &str = "commit-menu-item-0-0";
    const DISABLED: &str = "commit-menu-item-5-0";
    const COMMAND: &str = "platform-menu-cmd-view.toggleSidebar";
    let other = non_apple_slug();
    for (slug, system) in [
        ("apple-light", true),
        ("apple-dark", true),
        ("apple-dark", false),
        (other.as_str(), true),
    ] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(system));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            move_to(cx, window, None);
            cx.update_window(window, |_, window, cx| {
                app.read(cx).root_focus.clone().unwrap().focus(window, cx);
            })
            .unwrap();
            keys(cx, window, "shift-f10");
            assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
            // The production MenuKeys focus-visible path, not a painted stand-in.
            crate::context_menu_keys::focus_menu_control(cx, app, window, ENABLED);
            draw(cx, window);
            let palette = theme::theme();
            let on_accent = active && matches!(slug, "apple-light" | "apple-dark");
            let ink = if on_accent {
                if system {
                    0xffffff
                } else {
                    0x000000
                }
            } else {
                palette.text_main
            };
            let expected_fill = if active {
                if on_accent {
                    palette.ui_accent()
                } else {
                    palette.selected
                }
            } else {
                palette.inactive_selected
            };
            assert_eq!(
                fill(cx, window, ENABLED),
                expected_fill,
                "keyboard menu fill"
            );
            assert_paint(
                cx,
                window,
                &format!("{ENABLED}-primary"),
                Hsla::from(gpui::rgb(ink)),
                ink,
            );
            keys(cx, window, "end");
            draw(cx, window);
            let disabled_paint =
                e2e::control_paint(window.window_id(), &format!("{DISABLED}-primary"))
                    .unwrap()
                    .text_color;
            assert_eq!(
                disabled_paint,
                Hsla::from(gpui::rgb(palette.color_blocker_muted))
            );
            assert_eq!(
                fill(cx, window, DISABLED),
                palette.selected,
                "disabled focus retains its old neutral discoverability highlight"
            );
            assert_label_pixels(
                cx,
                window,
                bounds(window, &format!("{DISABLED}-primary")),
                palette.color_blocker_muted,
            );
            keys(cx, window, "enter");
            assert!(cx.read(|cx| app.read(cx).commit_menu.is_some()));
            move_to(cx, window, Some(ENABLED));
            assert_eq!(fill(cx, window, ENABLED), expected_fill, "hover menu fill");
            assert_paint(
                cx,
                window,
                &format!("{ENABLED}-primary"),
                Hsla::from(gpui::rgb(ink)),
                ink,
            );
            keys(cx, window, "home");
            move_to(cx, window, Some(DISABLED));
            assert_eq!(
                fill(cx, window, DISABLED),
                palette.surface,
                "disabled hover unchanged"
            );
            assert_eq!(
                e2e::control_paint(window.window_id(), &format!("{DISABLED}-primary"))
                    .unwrap()
                    .text_color,
                disabled_paint
            );
            assert_label_pixels(
                cx,
                window,
                bounds(window, &format!("{DISABLED}-primary")),
                palette.color_blocker_muted,
            );
            keys(cx, window, "escape");
            assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
            move_to(cx, window, None);
            app.update(cx, |app, cx| {
                assert!(app.open_platform_menu_for_e2e("View", cx));
            });
            draw(cx, window);
            move_to(cx, window, Some(COMMAND));
            assert_eq!(
                fill(cx, window, COMMAND),
                expected_fill,
                "real command hover fill"
            );
            assert_paint(
                cx,
                window,
                &format!("{COMMAND}-primary"),
                Hsla::from(gpui::rgb(ink)),
                ink,
            );
            let secondary = if on_accent {
                Hsla::from(gpui::rgb(ink)).alpha(0.7)
            } else {
                Hsla::from(gpui::rgb(palette.text_muted))
            };
            assert_paint(
                cx,
                window,
                &format!("{COMMAND}-shortcut"),
                secondary,
                if on_accent {
                    composite(ink, 0.7, expected_fill)
                } else {
                    palette.text_muted
                },
            );
            keys(cx, window, "escape");
        }
    }
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    move_to(cx, window, None);
}

pub fn scenario_selection_on_accent(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _gh = crate::pr_fields_focus::OfflineGh::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme", "accent_source", "accent"]);
    let before = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.select_headless(0);
        app.set_window_active_for_e2e(true, cx);
    });
    for slug in ["apple-light", "apple-dark"] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(1.), gpui::px(1.)),
            None,
            Modifiers::none(),
        );
        draw(cx, window);
        let row = e2e::control_bounds(window.window_id(), "graph-commit-row")
            .expect("selected graph row");
        let fill = composite(
            theme::SystemAccent::Blue.rgb(slug == "apple-dark"),
            0.30,
            theme::theme().bg_base,
        );
        assert_eq!(
            rgb_pixel(
                cx,
                window,
                f32::from(row.right()) - 2.,
                f32::from(row.top()) + 1.
            ),
            fill,
            "Graph selection paints accent over its normal stripe"
        );
        let label = e2e::control_bounds(window.window_id(), "graph-commit-label")
            .expect("actual summary label probe");
        let paint = e2e::control_paint(window.window_id(), "graph-commit-label").unwrap();
        assert_eq!(
            paint.text_color,
            Hsla::from(gpui::rgb(theme::theme().text_main)),
            "Graph selected summary retains normal ink"
        );
        assert_label_pixels(cx, window, label, theme::theme().text_main);
        assert!(contrast(theme::theme().text_main, fill) >= 4.5);
        let badge = e2e::control_paint(window.window_id(), "graph-selected-badge-label")
            .expect("selected transparent ref chip label painted");
        let lane = cx.read(|cx| app.read(cx).view().rows[0].node_color);
        let (_, _, badge_ink) = theme::badge_style(theme::lane_color_u32(lane));
        assert_eq!(badge.text_color, Hsla::from(gpui::rgb(badge_ink)));
        let badge_bounds =
            e2e::control_bounds(window.window_id(), "graph-selected-badge-label").unwrap();
        assert_label_pixels(cx, window, badge_bounds, badge_ink);
        let secondary = e2e::control_paint(window.window_id(), "graph-commit-secondary").unwrap();
        assert_eq!(
            secondary.text_color,
            Hsla::from(gpui::rgb(theme::theme().text_sub))
        );
        assert_neutral_segment(cx, window, "sidebar-mode-graph", "sidebar-mode-graph-label");
        let selected = cx.read(|cx| app.read(cx).ui().selected);
        app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
        draw(cx, window);
        assert_eq!(
            rgb_pixel(
                cx,
                window,
                f32::from(row.right()) - 2.,
                f32::from(row.top()) + 1.
            ),
            theme::theme().inactive_selected
        );
        assert_eq!(
            e2e::control_paint(window.window_id(), "graph-commit-label")
                .unwrap()
                .text_color,
            Hsla::from(gpui::rgb(theme::theme().text_main))
        );
        assert_eq!(
            e2e::control_bounds(window.window_id(), "graph-commit-row").unwrap(),
            row
        );
        assert_eq!(cx.read(|cx| app.read(cx).ui().selected), selected);
        assert_neutral_segment(cx, window, "sidebar-mode-graph", "sidebar-mode-graph-label");
        app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    }
    menu_legs(cx, &app, window);
    // Give the real Commit Panel one modified file with additions and deletions.
    std::fs::write(fixture.path().join("README.md"), "changed\nsecond line\n").unwrap();
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    let panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .unwrap();
    for slug in ["apple-light", "apple-dark"] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| {
            app.set_theme(slug, cx);
            app.set_window_active_for_e2e(true, cx);
        });
        draw(cx, window);
        let (id, label_id) = if cx.read(|cx| panel.read(cx).state.tree_view) {
            ("cp-view-tree", "cp-view-tree-label")
        } else {
            ("cp-view-list", "cp-view-list-label")
        };
        assert_neutral_segment(cx, window, id, label_id);
        app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
        draw(cx, window);
        assert_neutral_segment(cx, window, id, label_id);
    }
    app.update(cx, |app, cx| {
        app.open_main_diff_wip(
            kagi::ui::commit_panel::CommitPanelFileRef::Unstaged { index: 0 },
            cx,
        )
    });
    cx.run_until_parked();
    let other = non_apple_slug();
    for slug in ["apple-light", "apple-dark", other.as_str()] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            draw(cx, window);
            let palette = theme::theme();
            if active && matches!(slug, "apple-light" | "apple-dark") {
                assert_paint(
                    cx,
                    window,
                    "cp-selected-unstaged-label",
                    Hsla::from(gpui::rgb(0xffffff)),
                    0xffffff,
                );
            } else {
                assert_paint(
                    cx,
                    window,
                    "cp-selected-unstaged-label",
                    Hsla::from(gpui::rgb(palette.text_main)),
                    palette.text_main,
                );
                let row = bounds(window, "cp-selected-unstaged-row");
                // Isolate text from the rightmost mini-bar, whose semantic fills never change.
                let mut labels = row;
                labels.size.width =
                    bounds(window, "cp-row-stage").left() - row.left() - theme::scaled_px(36.);
                assert_label_pixels(cx, window, labels, palette.change_modified);
                assert_label_pixels(cx, window, labels, palette.change_added);
                assert_label_pixels(cx, window, labels, palette.change_deleted);
            }
        }
    }
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    drop(panel);
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let head = kagi_git::CommitId(String::from_utf8(output.stdout).unwrap().trim().to_string());
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
        window.focus(&input.read(cx).focus_handle(cx), cx);
        input.update(cx, |input, cx| {
            input.set_value("on-accent-probe", window, cx)
        });
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
    for preset in theme::THEMES {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| app.set_theme(&preset.slug, cx));
        draw(cx, window);
        let palette = theme::theme();
        let fg = theme::primary_button_foreground(&palette);
        assert!(
            contrast(palette.ui_accent(), fg) >= 3.,
            "{} on-accent contrast",
            palette.slug
        );
        let primary = e2e::confirm_bounds(window.window_id()).expect("enabled primary");
        assert_eq!(
            rgb_pixel(
                cx,
                window,
                f32::from(primary.left()) + 6.,
                f32::from(primary.center().y)
            ),
            palette.ui_accent()
        );
        assert_label_pixels(cx, window, primary, fg);
        if matches!(&*palette.slug, "apple-light" | "apple-dark") {
            assert_eq!(fg, 0xffffff);
        }
    }
    app.update(cx, |app, cx| {
        app.cancel_create_branch_modal();
        app.set_theme(&before, cx);
    });
    drop(input);
    draw(cx, window);
    crate::app_conflict::click_control(cx, window, "tab-add");
    cx.run_until_parked();
    for slug in ["apple-light", "apple-dark"] {
        let _accent = theme::AccentInjection::new(theme::SystemAccent::Blue, Some(true));
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for active in [true, false] {
            app.update(cx, |app, cx| app.set_window_active_for_e2e(active, cx));
            for (id, label) in [
                ("home-pane-repos", "home-pane-repos-label"),
                ("home-pane-prs", "home-pane-prs-label"),
                ("home-pane-issues", "home-pane-issues-label"),
            ] {
                draw(cx, window);
                crate::app_conflict::click_control(cx, window, id);
                cx.run_until_parked();
                move_to(cx, window, None);
                assert_neutral_segment(cx, window, id, label);
            }
        }
    }
    unmount(cx, app, window);
    surfaces::pr_card_legs(cx);
    surfaces::conflict_segment_legs(cx);
}
