//! #1067: the real toolbar count's text and pill scale together, without clipping.
use gpui::{AnyWindowHandle, Bounds, Pixels, VisualTestAppContext};
use kagi::ui::{
    commands::{ZoomIn, ZoomOut, ZoomReset},
    e2e,
};
use kagi_ui_core::theme;

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::recovery_layout::{contained, GlobalSettings};

fn frame(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn bounds(window: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    e2e::control_bounds(window.window_id(), id).unwrap_or_else(|| panic!("missing {id}"))
}

fn inspect(window: AnyWindowHandle, button: &str) -> (f32, f32, f32) {
    let badge = bounds(window, &format!("{button}-count"));
    let text = bounds(window, &format!("{button}-count-text"));
    let paint = e2e::control_paint(window.window_id(), &format!("{button}-count-text")).unwrap();
    contained(badge, text, "count text inside pill");
    contained(
        bounds(window, "toolbar-row"),
        badge,
        "count pill inside toolbar row",
    );
    contained(paint.mask, text, "count text inside paint mask");
    (
        f32::from(badge.size.height),
        f32::from(paint.font_size),
        f32::from(text.size.width),
    )
}

fn ratio(actual: f32, baseline: f32, zoom: f32, label: &str) {
    assert!(
        (actual / baseline - zoom).abs() < 0.06,
        "{label}: ratio {} must follow zoom {zoom}",
        actual / baseline
    );
}

fn shaped_width_scale(actual: f32, baseline: f32, zoom: f32) {
    // GPUI rounds each measured text box outward to whole logical pixels.
    // One digit cannot use a percentage tolerance (e.g. 6px -> 9px).
    // The two independently rounded boxes contribute at most 1 + zoom pixels.
    assert!(
        (actual - baseline * zoom).abs() <= 1.0 + zoom,
        "shaped digit width {actual} must scale from {baseline} at {zoom}"
    );
}

pub fn scenario_toolbar_count_scale(cx: &mut VisualTestAppContext) {
    let _restore = GlobalSettings::capture();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = repo_fingerprint(&repo);
    let (app, window) = mount(cx, &repo);
    frame(cx, window);
    cx.update_window(window, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
    for count in [1, 99, 100] {
        // Seed the session-owned accepted read, not an alternate badge component.
        app.update(cx, |app, cx| {
            app.view_mut().toolbar_state.ahead = count;
            app.view_mut().toolbar_state.behind = count;
            cx.notify();
        });
        cx.dispatch_action(window, ZoomReset);
        frame(cx, window);
        let baseline = [inspect(window, "tb-pull"), inspect(window, "tb-push")];
        assert!((theme::zoom() - 1.).abs() < 0.001);
        // The public zoom action caps at 1670 permille (displayed 167%),
        // approximately the migrated 1667-permille maximum used by the issue.
        for _ in 0..7 {
            cx.dispatch_action(window, ZoomIn);
            frame(cx, window);
        }
        assert!((theme::zoom() - 1.667).abs() < 0.004);
        for (index, button) in ["tb-pull", "tb-push"].into_iter().enumerate() {
            let measured = inspect(window, button);
            ratio(measured.0, baseline[index].0, 1.667, "badge height");
            ratio(measured.1, baseline[index].1, 1.667, "badge font");
            shaped_width_scale(measured.2, baseline[index].2, 1.667);
            assert!(
                baseline[index].1 >= 11.,
                "100% count font must be at least 11 logical points; got {}",
                baseline[index].1
            );
        }
        cx.dispatch_action(window, ZoomReset);
        frame(cx, window);
        for _ in 0..3 {
            cx.dispatch_action(window, ZoomOut);
            frame(cx, window);
        }
        assert!((theme::zoom() - 0.7).abs() < 0.001);
        for (index, button) in ["tb-pull", "tb-push"].into_iter().enumerate() {
            let measured = inspect(window, button);
            ratio(measured.0, baseline[index].0, 0.7, "small badge height");
            ratio(measured.1, baseline[index].1, 0.7, "small badge font");
            shaped_width_scale(measured.2, baseline[index].2, 0.7);
        }
    }
    assert_eq!(
        repo_fingerprint(&repo),
        before,
        "zoom must not write the repository"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS toolbar_count_scale: Pull/Push 1/99/99+ at 100%/167%/70%, actual font and shaped digit bounds, pill/row/mask containment");
}
