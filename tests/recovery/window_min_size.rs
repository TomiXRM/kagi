//! #1550: production main-window minimum applies to restore and native resizing.
use gpui::{point, px, size, Bounds, VisualTestAppContext};
use kagi::ui::{e2e, settings, window_restore};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_window_min_size(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["window_frame", "window_size"]);
    let frame = settings::MainWindowFrame {
        x: 40.,
        y: 50.,
        width: 200.,
        height: 150.,
        display: "primary".into(),
    };
    let visible = Bounds {
        origin: point(px(0.), px(0.)),
        size: size(px(1920.), px(1080.)),
    };
    let restored = window_restore::restore_on_display(&frame, "primary", visible).unwrap();
    assert_eq!(
        restored.size,
        size(px(940.), px(660.)),
        "saved 200×150 frame must clamp to the supported minimum"
    );
    let fixture = crate::macos::build_fixture();
    for language in ["en", "ja"] {
        for zoom in [1., 1.67] {
            std::env::set_var("KAGI_LANG", language);
            kagi::ui::i18n::init_lang();
            kagi::ui::theme::set_zoom(zoom);
            let state = e2e::app_state(fixture.path()).unwrap();
            let out = Rc::new(RefCell::new(None));
            let window = cx.update(|app| {
                let mut options = window_restore::options(app);
                assert_eq!(
                    options.window_min_size,
                    Some(size(px(940.), px(660.))),
                    "native runtime minimum must match restore"
                );
                options.window_bounds = Some(gpui::WindowBounds::Windowed(restored));
                options.show = false;
                options.focus = false;
                app.open_window(options, |window, app| {
                    e2e::mount_root(state, window, app, &out)
                })
                .unwrap()
            });
            let window = *window;
            let entity = out.borrow_mut().take().unwrap();
            request_native_size(cx, window, 200., 150.);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                crate::macos::drain_native_events();
                crate::toolbar_overflow::frame(cx, window);
                if cx
                    .update_window(window, |_, window, _| window.viewport_size())
                    .unwrap()
                    == size(px(940.), px(660.))
                {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "native minimum resize timed out"
                );
            }
            let viewport = cx
                .update_window(window, |_, window, _| window.viewport_size())
                .unwrap();
            assert_eq!(
                viewport,
                size(px(940.), px(660.)),
                "runtime 200×150 resize clamps"
            );
            let viewport = Bounds::new(point(px(0.), px(0.)), viewport);
            for id in ["compact-rail", "compact-rail-local", "tb-graph-mode"] {
                let control = e2e::control_bounds(window.window_id(), id)
                    .unwrap_or_else(|| panic!("missing {id} at {language}/{zoom}"));
                crate::recovery_layout::contained(viewport, control, id);
            }
            let local = e2e::control_bounds(window.window_id(), "compact-rail-local").unwrap();
            cx.simulate_click(window, local.center(), gpui::Modifiers::none());
            for _ in 0..4 {
                crate::toolbar_overflow::frame(cx, window);
            }
            let search = e2e::control_bounds(window.window_id(), "sidebar-filter")
                .expect("compact navigator exposes its real search field");
            crate::recovery_layout::contained(viewport, search, "search at minimum");
            cx.simulate_keystrokes(window, "escape");
            for _ in 0..4 {
                crate::toolbar_overflow::frame(cx, window);
            }
            if zoom > 1. {
                let overflow = e2e::control_bounds(window.window_id(), "tb-overflow")
                    .expect("zoomed toolbar retains its overflow entry");
                crate::recovery_layout::contained(viewport, overflow, "overflow at minimum");
                cx.simulate_click(window, overflow.center(), gpui::Modifiers::none());
                for _ in 0..4 {
                    crate::toolbar_overflow::frame(cx, window);
                }
                assert!(e2e::control_bounds(window.window_id(), "tb-overflow-menu").is_some());
                cx.simulate_keystrokes(window, "escape");
            }
            for wanted in [size(px(1000.), px(720.)), size(px(1400.), px(900.))] {
                cx.update_window(window, |_, window, _| window.resize(wanted))
                    .unwrap();
                crate::macos::drain_native_events();
                cx.run_until_parked();
                assert_eq!(
                    cx.update_window(window, |_, window, _| window.viewport_size())
                        .unwrap(),
                    wanted
                );
            }
            crate::macos::unmount(cx, entity, window);
        }
    }
}

fn request_native_size(
    cx: &mut VisualTestAppContext,
    target: gpui::AnyWindowHandle,
    width: f64,
    height: f64,
) {
    #[repr(C)]
    struct NativeSize {
        width: f64,
        height: f64,
    }
    unsafe impl objc2::encode::Encode for NativeSize {
        const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
            "CGSize",
            &[
                objc2::encode::Encoding::Double,
                objc2::encode::Encoding::Double,
            ],
        );
    }
    let marker = format!("minimum-probe-{:?}", target.window_id());
    cx.update_window(target, |_, window, _| window.set_window_title(&marker))
        .unwrap();
    cx.update(|_| unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        let mut found = false;
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let title: *mut AnyObject = msg_send![window, title];
            let bytes: *const std::ffi::c_char = msg_send![title, UTF8String];
            if !bytes.is_null() && std::ffi::CStr::from_ptr(bytes).to_string_lossy() == marker {
                let value: *mut AnyObject =
                    msg_send![class!(NSValue), valueWithSize: NativeSize { width, height }];
                let attribute: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: c"AXSize".as_ptr()];
                let _: () =
                    msg_send![window, accessibilitySetValue: value, forAttribute: attribute];
                found = true;
                break;
            }
        }
        assert!(found, "native repository window found by its own identity");
    });
}
