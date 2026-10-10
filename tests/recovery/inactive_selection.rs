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
        assert_ne!(
            active, inactive,
            "{name}: non-key selection must lose its blue tint"
        );
        assert_eq!(
            &inactive[..3],
            &[220, 220, 220],
            "{name}: actual inactive fill must be #dcdcdc"
        );
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
