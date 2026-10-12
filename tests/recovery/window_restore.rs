//! #1367: production startup options applied to the real native repository root.
use gpui::{point, px, size, AnyWindowHandle, Bounds, Entity, Pixels, VisualTestAppContext};
use kagi::ui::{e2e, settings, window_restore, KagiApp};
use std::{cell::RefCell, rc::Rc, time::Duration};

struct NoRestoreGuard(Option<std::ffi::OsString>);
impl NoRestoreGuard {
    fn set() -> Self {
        let previous = std::env::var_os("KAGI_NO_RESTORE");
        std::env::set_var("KAGI_NO_RESTORE", "1");
        Self(previous)
    }
}
impl Drop for NoRestoreGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var("KAGI_NO_RESTORE", previous),
            None => std::env::remove_var("KAGI_NO_RESTORE"),
        }
    }
}

fn mount_window(cx: &mut VisualTestAppContext) -> (Entity<KagiApp>, AnyWindowHandle) {
    let out = Rc::new(RefCell::new(None));
    let window = cx.update(|app| {
        let mut options = window_restore::options(app);
        options.show = false;
        options.focus = false;
        app.open_window(options, |window, app| {
            e2e::mount_root(KagiApp::with_error("geometry probe"), window, app, &out)
        })
        .expect("open repository root")
    });
    let entity = out.borrow_mut().take().expect("repository entity");
    (entity, *window)
}

fn launch(cx: &mut VisualTestAppContext) -> Bounds<Pixels> {
    let (entity, window) = mount_window(cx);
    let actual = cx
        .update_window(window, |_, window, _| window.bounds())
        .unwrap();
    crate::macos::unmount(cx, entity, window);
    actual
}

pub(crate) fn scenario_window_restore_position(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_frame", "window_size"]);
    let display = cx.update(|app| app.primary_display().expect("main display").bounds());
    let expected = Bounds {
        origin: point(display.origin.x + px(40.), display.origin.y + px(50.)),
        size: size(px(640.), px(480.)),
    };
    settings::write_setting(
        "window_frame",
        Some(&format!(
            "{},{},640,480,disconnected-id",
            f32::from(expected.origin.x),
            f32::from(expected.origin.y)
        )),
    );
    assert_eq!(
        launch(cx),
        expected,
        "a visible saved origin and size must restore exactly, even after display identity changes"
    );
    settings::write_setting(
        "window_frame",
        Some("100000,100000,640,480,disconnected-id"),
    );
    let centered = cx.update(|app| Bounds::centered(None, size(px(640.), px(480.)), app));
    assert_eq!(
        launch(cx),
        centered,
        "off-screen geometry must fall back to the main display centre"
    );
    settings::write_setting("window_frame", None);
    settings::write_setting("window_size", Some("640x480"));
    assert_eq!(
        launch(cx),
        centered,
        "legacy size-only preferences still restore centred"
    );
    settings::write_setting("window_size", None);
    let default = launch(cx);
    settings::write_setting("window_frame", Some("NaN,50,640,480,id"));
    assert_eq!(
        launch(cx),
        default,
        "corrupt rectangles use the safe default"
    );
    settings::write_setting("window_frame", Some("-600,50,640,480,id"));
    let partial = cx.update(|app| window_restore::initial_bounds(app));
    assert_eq!(
        partial.origin.x,
        px(-600.),
        "a positive partial intersection remains restorable"
    );
    settings::write_setting("window_frame", Some("40,50,640,480,id"));
    check_persistence(cx, default);
}

fn check_persistence(cx: &mut VisualTestAppContext, default: Bounds<Pixels>) {
    let before = settings::read_setting("window_frame");
    let (entity, window) = mount_window(cx);
    cx.update_window(window, |_, window, app| {
        window_restore::observe(&entity, window, app);
        window.resize(size(px(700.), px(520.)));
    })
    .expect("observe production repository window");
    crate::macos::drain_native_events();
    cx.run_until_parked();
    assert_eq!(
        settings::read_setting("window_frame"),
        before,
        "bounds writes wait for drag/resize to settle"
    );
    cx.advance_clock(Duration::from_millis(201));
    cx.run_until_parked();
    let actual = cx
        .update_window(window, |_, window, _| window.window_bounds().get_bounds())
        .unwrap();
    let saved = settings::main_window_frame().expect("settled geometry persisted");
    assert_eq!(
        (saved.x, saved.y, saved.width, saved.height),
        (
            f32::from(actual.origin.x),
            f32::from(actual.origin.y),
            f32::from(actual.size.width),
            f32::from(actual.size.height),
        )
    );
    assert!(!saved.display.is_empty(), "save the connected display UUID");
    cx.update_window(window, |_, window, _| {
        window.resize(size(px(720.), px(540.)))
    })
    .unwrap();
    crate::macos::drain_native_events();
    cx.run_until_parked();
    let final_bounds = cx
        .update_window(window, |_, window, _| window.window_bounds().get_bounds())
        .unwrap();
    crate::macos::unmount(cx, entity, window);
    let final_saved = settings::main_window_frame().expect("root release saves pending geometry");
    assert_eq!(final_saved.width, f32::from(final_bounds.size.width));
    assert_eq!(final_saved.height, f32::from(final_bounds.size.height));
    assert_eq!(
        launch(cx),
        final_bounds,
        "the saved live native rectangle survives another launch"
    );
    let before_skip = settings::read_setting("window_frame");
    let _skip = NoRestoreGuard::set();
    assert_eq!(launch(cx), default, "NO_RESTORE ignores saved geometry");
    let (entity, window) = mount_window(cx);
    cx.update_window(window, |_, window, app| {
        window_restore::observe(&entity, window, app);
        window.resize(size(px(800.), px(600.)));
    })
    .unwrap();
    crate::macos::unmount(cx, entity, window);
    assert_eq!(
        settings::read_setting("window_frame"),
        before_skip,
        "NO_RESTORE does not save"
    );
}
