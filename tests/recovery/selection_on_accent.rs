//! #1468: Finder-style selected rows and accent labels, measured at native paint.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Focusable, Hsla, Modifiers, VisualTestAppContext};
use kagi::ui::e2e;
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
        for y in
            (f32::from(bounds.top()) * scale) as u32..(f32::from(bounds.bottom()) * scale) as u32
        {
            for x in (f32::from(bounds.left()) * scale) as u32
                ..(f32::from(bounds.right()) * scale) as u32
            {
                let p = image.get_pixel(x, y).0;
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
        assert!(
            matching >= 3,
            "painted label must contain foreground #{foreground:06x}, got {matching} pixels"
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

pub fn scenario_selection_on_accent(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
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
        let fill = theme::SystemAccent::Blue.rgb(slug == "apple-dark");
        assert_eq!(
            rgb_pixel(
                cx,
                window,
                f32::from(row.right()) - 2.,
                f32::from(row.top()) + 1.
            ),
            fill,
            "active selection paints the exact system accent, not a wash"
        );
        let label = e2e::control_bounds(window.window_id(), "graph-commit-label")
            .expect("actual summary label probe");
        let paint = e2e::control_paint(window.window_id(), "graph-commit-label").unwrap();
        assert_eq!(
            paint.text_color,
            Hsla::from(gpui::rgb(0xffffff)),
            "active selected summary is white"
        );
        assert_label_pixels(cx, window, label, 0xffffff);
        let badge = e2e::control_paint(window.window_id(), "graph-selected-badge-label")
            .expect("selected transparent ref chip label painted");
        assert_eq!(badge.text_color, Hsla::from(gpui::rgb(0xffffff)));
        let badge_bounds =
            e2e::control_bounds(window.window_id(), "graph-selected-badge-label").unwrap();
        assert_label_pixels(cx, window, badge_bounds, 0xffffff);
        let secondary = e2e::control_paint(window.window_id(), "graph-commit-secondary").unwrap();
        assert_eq!(
            secondary.text_color,
            Hsla::from(gpui::rgb(0xffffff)).alpha(0.7)
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
    unmount(cx, app, window);
}
