//! #1174: native painted separators retain scaled resize targets.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{
    point, px, AnyWindowHandle, Modifiers, MouseButton, Pixels, Point, VisualTestAppContext,
};
use kagi::ui::{e2e, theme};

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn assert_pixel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    sample: Point<Pixels>,
    color: u32,
    state: &str,
) {
    cx.update_window(window, |_, window, _| {
        let image = window.render_to_image().expect("native divider pixels");
        let scale = window.scale_factor();
        let pixel = image
            .get_pixel(
                (f32::from(sample.x) * scale) as u32,
                (f32::from(sample.y) * scale) as u32,
            )
            .0;
        assert_eq!(
            &pixel[..3],
            &[(color >> 16) as u8, (color >> 8) as u8, color as u8],
            "{state}: painted color"
        );
    })
    .unwrap();
}

pub fn scenario_hairline_dividers(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&[
        "theme",
        "ui_zoom",
        "sidebar_panes",
        "sidebar_width",
        "panel_width",
    ]);
    let previous_theme = theme::theme().slug.to_string();
    let previous_zoom = theme::zoom();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, fixture.path().to_path_buf(), cx);
        cx.notify();
    });
    cx.run_until_parked();
    paint(cx, window);
    cx.advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    paint(cx, window);
    let scale = cx
        .update_window(window, |_, window, _| window.scale_factor())
        .unwrap();
    if scale != 2. {
        eprintln!("[gui-e2e] SKIP hairline_dividers: Retina geometry oracle requires scale 2, actual {scale}");
        unmount(cx, app, window);
        return;
    }
    for slug in ["apple-light", "apple-dark"] {
        app.update(cx, |app, cx| app.set_theme(slug, cx));
        for zoom in [1., 1.67] {
            theme::set_zoom(zoom);
            cx.simulate_mouse_move(window, point(px(1.), px(1.)), None, Modifiers::none());
            app.update(cx, |_, cx| cx.notify());
            paint(cx, window);
            for (id, horizontal) in [
                ("sidebar-local-divider", true),
                ("sidebar-remote-divider", true),
                ("sidebar-worktrees-divider", true),
                ("sidebar-tags-divider", true),
                ("divider-sidebar", false),
                ("divider-panel", false),
                ("divider-bottom-panel", true),
            ] {
                let hit = e2e::control_bounds(window.window_id(), id).expect(id);
                let thickness = if horizontal {
                    hit.size.height
                } else {
                    hit.size.width
                };
                let expected_hit = px((4. * theme::scaled(1.) * scale).round() / scale);
                assert_eq!(
                    thickness, expected_hit,
                    "{slug}/{zoom}/{id}: unchanged Retina-snapped resize target {hit:?}"
                );
                let line = e2e::control_bounds(window.window_id(), &format!("{id}-line"))
                    .expect("painted hairline");
                assert_eq!(
                    if horizontal {
                        line.size.height
                    } else {
                        line.size.width
                    },
                    px(1.),
                    "{slug}/{zoom}/{id}: literal logical hairline"
                );
                // An odd physical hit width has no exact 1px center; require
                // the nearest physical-pixel center, not a loose tolerance.
                let center_error = if horizontal {
                    f32::from(line.center().y - hit.center().y).abs()
                } else {
                    f32::from(line.center().x - hit.center().x).abs()
                };
                let expected_error = if (f32::from(thickness) * scale) as u32 % 2 == 0 {
                    0.
                } else {
                    0.25
                };
                assert_eq!(
                    center_error, expected_error,
                    "{slug}/{zoom}/{id}: nearest physical-pixel center {hit:?}/{line:?}"
                );
                assert_pixel(
                    cx,
                    window,
                    line.center(),
                    theme::theme().surface,
                    "resting hairline",
                );
                let edge = if horizontal {
                    point(hit.center().x, hit.top() + px(0.5))
                } else {
                    point(hit.left() + px(0.5), hit.center().y)
                };
                let fill = if id.starts_with("sidebar-") || id == "divider-sidebar" {
                    theme::theme().sidebar
                } else {
                    theme::theme().panel
                };
                assert_pixel(
                    cx,
                    window,
                    edge,
                    fill,
                    "resting hit region outside hairline",
                );
                cx.simulate_mouse_move(window, hit.center(), None, Modifiers::none());
                paint(cx, window);
                assert_pixel(cx, window, edge, theme::theme().ui_accent(), "hover accent");
                assert_pixel(
                    cx,
                    window,
                    line.center(),
                    theme::theme().ui_accent(),
                    "hover line accent",
                );
                cx.simulate_mouse_down(window, hit.center(), MouseButton::Left, Modifiers::none());
                // Move along the divider, starting a real drag without resizing
                // either adjacent pane or moving away from the hover target.
                let along = if horizontal {
                    hit.center() + point(px(8.), px(0.))
                } else {
                    hit.center() + point(px(0.), px(8.))
                };
                cx.simulate_mouse_move(window, along, Some(MouseButton::Left), Modifiers::none());
                paint(cx, window);
                assert_pixel(cx, window, edge, theme::theme().ui_accent(), "drag accent");
                assert_pixel(
                    cx,
                    window,
                    line.center(),
                    theme::theme().ui_accent(),
                    "drag line accent",
                );
                cx.simulate_mouse_up(window, along, MouseButton::Left, Modifiers::none());
                cx.simulate_mouse_move(window, point(px(1.), px(1.)), None, Modifiers::none());
                paint(cx, window);
            }
        }
    }
    theme::set_zoom(previous_zoom);
    app.update(cx, |app, cx| app.set_theme(&previous_theme, cx));
    unmount(cx, app, window);
}
