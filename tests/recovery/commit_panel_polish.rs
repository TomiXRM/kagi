//! Native paint regressions for #1228 and #1232.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_layout::draw;
use gpui::{point, px, AnyWindowHandle, Focusable, Modifiers, MouseButton, VisualTestAppContext};
use gpui_component::ActiveTheme as _;
use kagi::ui::e2e;

fn pixel(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    name: &str,
    border: bool,
) -> [u8; 4] {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let bounds = e2e::control_bounds(window.window_handle().window_id(), name).unwrap();
        let image = window.render_to_image().expect("native composer paint");
        let scale = window.scale_factor();
        let x = (f32::from(bounds.right()) + if border { 0.5 } else { -8. }) * scale;
        let y = f32::from(bounds.center().y) * scale;
        image.get_pixel(x as u32, y as u32).0
    })
    .unwrap()
}

pub fn scenario_commit_panel_selection_hover(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    std::fs::write(fixture.path().join("polish.txt"), "staged\n").unwrap();
    git(fixture.path(), &["add", "polish.txt"]);
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window, (1440., 900.));
    let bounds = e2e::control_bounds(window.window_id(), "cp-selected-staged-row").unwrap();
    let position = point(bounds.left() + px(80.), bounds.center().y);
    cx.simulate_mouse_down(window, position, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(window, position, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_move(window, point(px(0.), px(0.)), None, Modifiers::none());
    let rest = pixel(cx, window, "cp-selected-staged-row", false);
    cx.simulate_mouse_move(window, position, None, Modifiers::none());
    let hover = pixel(cx, window, "cp-selected-staged-row", false);
    assert_eq!(
        hover, rest,
        "selected file hover must preserve selection fill"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_panel_selection_hover");
}

pub fn scenario_commit_panel_body_focus(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    draw(cx, window, (1440., 900.));
    let panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .unwrap();
    let body = cx.read(|cx| panel.read(cx).body_input.clone()).unwrap();
    let rest = pixel(cx, window, "cp-body", true);
    let rest_bounds = e2e::control_bounds(window.window_id(), "cp-body").unwrap();
    cx.update_window(window, |_, window, cx| {
        body.read(cx).focus_handle(cx).focus(window, cx)
    })
    .unwrap();
    cx.run_until_parked();
    let focused = pixel(cx, window, "cp-body", true);
    assert_ne!(
        focused, rest,
        "body focus must replace rest border with focus token"
    );
    let ring = cx.read(|cx| cx.theme().ring.to_rgb());
    for (actual, expected) in focused[..3].iter().zip([ring.r, ring.g, ring.b]) {
        assert!(
            actual.abs_diff((expected * 255.).round() as u8) <= 2,
            "body frame must paint theme focus token: {focused:?}, {ring:?}"
        );
    }
    assert_eq!(
        e2e::control_bounds(window.window_id(), "cp-body").unwrap(),
        rest_bounds,
        "focus must not change composer bounds"
    );
    drop(body);
    drop(panel);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_panel_body_focus");
}
