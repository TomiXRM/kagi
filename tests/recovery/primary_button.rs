//! #1076: real mouse input and native Metal background pixels, not token equality.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{AnyWindowHandle, Focusable, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::e2e;
use kagi_ui_core::theme;

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn fill(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    bounds: gpui::Bounds<gpui::Pixels>,
) -> [u8; 4] {
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native primary-button paint capture");
        let scale = window.scale_factor();
        // Inside the fill, away from label, rounded corners, border and focus ring.
        let x = ((f32::from(bounds.left()) + 6.) * scale) as u32;
        let y = (f32::from(bounds.center().y) * scale) as u32;
        image.get_pixel(x, y).0
    })
    .unwrap()
}

pub fn scenario_primary_button_states(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let before_theme = theme::theme().slug.to_string();
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let head = kagi_git::CommitId(String::from_utf8(output.stdout).unwrap().trim().to_string());
    app.update(cx, |app, cx| app.open_create_branch_modal(head, cx));
    paint(cx, window);
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
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "f e a t");
    cx.run_until_parked();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        cx.advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        if cx.read(|cx| {
            app.read(cx)
                .create_branch_modal()
                .unwrap()
                .plan
                .plan()
                .is_some_and(|plan| plan.blockers.is_empty())
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "branch plan must enable the primary"
        );
    }
    for palette in theme::THEMES {
        app.update(cx, |app, cx| app.set_theme(&palette.slug, cx));
        cx.run_until_parked();
        cx.simulate_mouse_move(
            window,
            gpui::point(gpui::px(1.), gpui::px(1.)),
            None,
            Modifiers::none(),
        );
        paint(cx, window);
        let bounds = e2e::confirm_bounds(window.window_id()).expect("enabled modal primary");
        let rest = fill(cx, window, bounds);
        cx.simulate_mouse_move(window, bounds.center(), None, Modifiers::none());
        paint(cx, window);
        let hover = fill(cx, window, bounds);
        cx.simulate_mouse_down(
            window,
            bounds.center(),
            MouseButton::Left,
            Modifiers::none(),
        );
        paint(cx, window);
        let pressed = fill(cx, window, bounds);
        // Release outside: never authorize a Git write in this visual regression.
        let outside = gpui::point(gpui::px(1.), gpui::px(1.));
        cx.simulate_mouse_move(window, outside, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(window, outside, MouseButton::Left, Modifiers::none());
        assert_ne!(rest, hover, "{} rest/hover painted fills", palette.slug);
        assert_ne!(rest, pressed, "{} rest/pressed painted fills", palette.slug);
        assert_ne!(
            hover, pressed,
            "{} hover/pressed painted fills",
            palette.slug
        );
    }
    app.update(cx, |app, cx| app.set_theme(&before_theme, cx));
    drop(input);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS primary_button_states: every built-in theme, native rest/hover/pressed pixels");
}
