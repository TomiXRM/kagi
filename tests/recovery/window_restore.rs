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
    let (uuid, display) = main_display_geometry(cx);
    let expected = Bounds {
        origin: point(display.origin.x + px(40.), display.origin.y + px(50.)),
        size: window_restore::minimum_size(),
    };
    settings::write_setting(
        "window_frame",
        Some(&format!(
            "{},{},640,480,{uuid}",
            f32::from(expected.origin.x),
            f32::from(expected.origin.y)
        )),
    );
    assert_eq!(
        launch(cx),
        expected,
        "a reachable saved origin and size restore exactly on the matching display"
    );
    settings::write_setting(
        "window_frame",
        Some("100000,100000,640,480,disconnected-id"),
    );
    let centered = centered_in(display, 640., 480.);
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
    settings::write_setting("window_frame", Some(&format!("40,50,640,480,{uuid}")));
    check_persistence(cx, default);
}

fn check_persistence(cx: &mut VisualTestAppContext, default: Bounds<Pixels>) {
    let before = settings::read_setting("window_frame");
    let (entity, window) = mount_window(cx);
    cx.update_window(window, |_, window, app| {
        window_restore::observe(&entity, window, app);
        window.resize(size(px(1000.), px(720.)));
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
        window.resize(size(px(1100.), px(760.)))
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

fn main_display_geometry(cx: &mut VisualTestAppContext) -> (String, Bounds<Pixels>) {
    cx.update(|app| {
        let display = app.primary_display().expect("main display");
        (
            display.uuid().expect("display UUID").to_string(),
            display.visible_bounds(),
        )
    })
}

fn centered_in(visible: Bounds<Pixels>, width: f32, height: f32) -> Bounds<Pixels> {
    let minimum = window_restore::minimum_size();
    let width = px(width).max(minimum.width).min(visible.size.width);
    let height = px(height).max(minimum.height).min(visible.size.height);
    Bounds {
        origin: point(
            (visible.origin.x + (visible.size.width - width) / 2.).round(),
            (visible.origin.y + (visible.size.height - height) / 2.).round(),
        ),
        size: size(width, height),
    }
}

pub(crate) fn scenario_window_restore_display_identity(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_frame", "window_size"]);
    let (_, visible) = main_display_geometry(cx);
    settings::write_setting("window_frame", Some("40,50,640,480,disconnected-id"));
    let options = cx.update(|app| window_restore::options(app));
    let primary = cx.update(|app| app.primary_display().unwrap().id());
    assert_eq!(
        options.display_id,
        Some(primary),
        "missing display UUID selects the primary display"
    );
    assert_eq!(
        launch(cx),
        centered_in(visible, 640., 480.),
        "display-local coordinates from a disconnected display must not restore on another display"
    );
    settings::write_setting("window_frame", Some("40,50,640,480,"));
    assert_eq!(
        launch(cx),
        centered_in(visible, 640., 480.),
        "unavailable UUID also centres on primary"
    );
    // The secondary monitor may physically sit left/above primary: its saved
    // local frame remains positive and only its identity can select it.
    let secondary = settings::MainWindowFrame {
        x: 40.,
        y: 50.,
        width: 640.,
        height: 480.,
        display: "secondary".into(),
    };
    let secondary_visible = Bounds {
        origin: point(px(0.), px(33.)),
        size: size(px(1920.), px(1040.)),
    };
    assert_eq!(
        window_restore::restore_on_display(&secondary, "secondary", secondary_visible),
        Some(Bounds {
            origin: point(px(40.), px(50.)),
            size: window_restore::minimum_size()
        })
    );
    assert!(
        window_restore::restore_on_display(&secondary, "primary", visible).is_none(),
        "secondary local coordinates must not qualify against primary"
    );
}

pub(crate) fn scenario_window_restore_fit(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_frame", "window_size"]);
    let (uuid, visible) = main_display_geometry(cx);
    settings::write_setting("window_frame", Some(&format!("100,100,2560,1440,{uuid}")));
    assert_eq!(
        launch(cx),
        Bounds {
            origin: visible.origin,
            size: visible.size
        },
        "restored size and origin must fit the target display's usable area"
    );
}

pub(crate) fn scenario_window_restore_titlebar(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_frame", "window_size"]);
    let (uuid, visible) = main_display_geometry(cx);
    let x = f32::from(visible.origin.x) - 600.;
    let y = f32::from(visible.origin.y) + 30.;
    settings::write_setting("window_frame", Some(&format!("{x},{y},640,480,{uuid}")));
    assert_eq!(
        launch(cx),
        centered_in(visible, 640., 480.),
        "40px of titlebar intersection is not enough to retrieve the window"
    );
    let x = f32::from(visible.origin.x) - 500.;
    settings::write_setting("window_frame", Some(&format!("{x},{y},640,480,{uuid}")));
    assert_eq!(
        launch(cx),
        Bounds {
            origin: point(visible.origin.x, px(y)),
            size: window_restore::minimum_size()
        },
        "140px of reachable titlebar qualifies, then the complete window moves inward"
    );
    let x = f32::from(visible.origin.x) + 40.;
    let y = f32::from(visible.origin.y) - 1.;
    settings::write_setting("window_frame", Some(&format!("{x},{y},640,480,{uuid}")));
    assert_eq!(
        launch(cx),
        centered_in(visible, 640., 480.),
        "the complete 28px titlebar band must be reachable"
    );
}
