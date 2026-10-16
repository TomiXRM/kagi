//! #1638: contrast from actual count-chip Metal pixels, including stacked fills.
use std::collections::HashMap;

use gpui::{point, px, AnyWindowHandle, Modifiers, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::theme;

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for id in [
        "modal-target-count",
        "discard-warnings-count",
        "discard-warnings-body",
    ] {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn luminance(rgb: [u8; 3]) -> f64 {
    let linear = rgb.map(|channel| {
        let c = f64::from(channel) / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
}

fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn assert_count_contrast(cx: &mut VisualTestAppContext, window: AnyWindowHandle, label: &str) {
    let window_id = window.window_id();
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native modal count capture");
        let scale = window.scale_factor();
        for id in ["modal-target-count", "discard-warnings-count"] {
            let bounds = e2e::control_bounds(window_id, id).expect("visible count chip");
            // Stay inside the border. This region contains only the number and
            // its padded pill fill, not adjacent labels, icons or focus rings.
            let inset = 2. * scale;
            let left = (f32::from(bounds.left()) * scale + inset).ceil() as u32;
            let right = (f32::from(bounds.right()) * scale - inset).floor() as u32;
            let top = (f32::from(bounds.top()) * scale + inset).ceil() as u32;
            let bottom = (f32::from(bounds.bottom()) * scale - inset).floor() as u32;
            assert!(
                left < right && top < bottom && right <= image.width() && bottom <= image.height()
            );
            let mut colors = HashMap::<[u8; 3], usize>::new();
            for y in top..bottom {
                for x in left..right {
                    let [r, g, b, _] = image.get_pixel(x, y).0;
                    *colors.entry([r, g, b]).or_default() += 1;
                }
            }
            let background = *colors.iter().max_by_key(|(_, count)| *count).unwrap().0;
            // The highest-contrast glyph core is conservative: antialiasing
            // can lower its contrast, never raise it above the source ink.
            // No palette values or copied alpha-composition formula are used.
            let (ink, ratio) = colors
                .keys()
                .map(|&ink| (ink, contrast(ink, background)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            assert!(
                ratio >= 4.5,
                "{label} {id}: ink {ink:?} on {background:?} = {ratio:.3}:1"
            );
            eprintln!("[gui-e2e] {label} {id}: ink {ink:?} on {background:?} = {ratio:.3}:1");
        }
    })
    .unwrap();
}

pub fn scenario_modal_count_contrast(cx: &mut VisualTestAppContext) {
    let _settings = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let before_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let files = [
        ("README.md", "# fixture\nmodified without approval\n"),
        ("notes.txt", "untracked note stays\n"),
        ("scratch.txt", "untracked scratch stays\n"),
    ];
    for (name, contents) in files {
        std::fs::write(fixture.path().join(name), contents).unwrap();
    }
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, fixture.path().to_path_buf(), cx)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        app.open_discard_all_modal(app.active_session().unwrap(), cx)
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).discard_modal().expect("real discard plan");
        assert_eq!(modal.paths.len(), 3);
        assert_eq!(modal.plan.warnings.len(), 1);
        assert!(modal.plan.blockers.is_empty());
    });
    for slug in ["nagi-dark", "nagi-light", "apple-dark", "apple-light"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for zoom in [0.7, 1., 1.67] {
            theme::set_zoom(zoom);
            cx.simulate_mouse_move(window, point(px(1.), px(1.)), None, Modifiers::none());
            paint(cx, window);
            let target_size = e2e::control_bounds(window.window_id(), "modal-target-count")
                .unwrap()
                .size;
            let warning_size = e2e::control_bounds(window.window_id(), "discard-warnings-count")
                .unwrap()
                .size;
            assert_count_contrast(cx, window, &format!("{slug}/{zoom} rest"));
            let header = e2e::control_bounds(window.window_id(), "discard-warnings").unwrap();
            cx.simulate_mouse_move(window, header.center(), None, Modifiers::none());
            paint(cx, window);
            assert_count_contrast(cx, window, &format!("{slug}/{zoom} hover"));
            for expanded in [false, true] {
                let header = e2e::control_bounds(window.window_id(), "discard-warnings").unwrap();
                cx.simulate_click(window, header.center(), Modifiers::none());
                cx.run_until_parked();
                paint(cx, window);
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "discard-warnings-body").is_some(),
                    expanded
                );
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "modal-target-count")
                        .unwrap()
                        .size,
                    target_size
                );
                assert_eq!(
                    e2e::control_bounds(window.window_id(), "discard-warnings-count")
                        .unwrap()
                        .size,
                    warning_size
                );
                assert_count_contrast(cx, window, &format!("{slug}/{zoom} expanded={expanded}"));
            }
        }
    }
    app.update(cx, |app, _| app.cancel_discard_modal());
    app.update(cx, |app, cx| app.set_theme(&before_theme, cx));
    unmount(cx, app, window);
    assert_eq!(repo_fingerprint(fixture.path()), before);
    for (name, contents) in files {
        assert_eq!(
            std::fs::read_to_string(fixture.path().join(name)).unwrap(),
            contents
        );
    }
    eprintln!("[gui-e2e] PASS modal_count_contrast: four themes, three zooms, hover/disclosure, native composed pixels, no Git write");
}
