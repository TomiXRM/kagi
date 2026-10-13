//! #1462: document tabs always occupy the first titlebar row, above the toolbar.
use crate::macos::{build_fixture, unmount};
use crate::recovery_layout::contained;
use gpui::{px, size, AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_ui_core::{i18n, settings, theme};
use std::{cell::RefCell, rc::Rc};

pub fn scenario_titlebar_tabs(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let first = build_fixture();
    let second = build_fixture();
    for slug in ["apple-light", "apple-dark"] {
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                for display in [
                    settings::ToolbarDisplay::IconOnly,
                    settings::ToolbarDisplay::IconAndText,
                ] {
                    settings::set_toolbar_display(display);
                    for (width, height) in [(940., 660.), (1392., 883.)] {
                        let state = e2e::app_state(first.path()).unwrap();
                        let captured = Rc::new(RefCell::new(None));
                        let output = captured.clone();
                        let win: AnyWindowHandle = cx
                            .update(|cx| {
                                let mut options = kagi::ui::window_restore::options(cx);
                                options.window_bounds =
                                    Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                                        origin: gpui::point(px(-10000.), px(-10000.)),
                                        size: size(px(width), px(height)),
                                    }));
                                options.focus = false;
                                options.show = false;
                                cx.open_window(options, move |window, cx| {
                                    e2e::mount_root(state, window, cx, &output)
                                })
                            })
                            .expect("open production titlebar offscreen")
                            .into();
                        let app = captured.borrow().clone().unwrap();
                        app.update(cx, |app, cx| {
                            app.set_theme(slug, cx);
                            cx.notify();
                        });
                        kagi::ui::tab_panel_a11y::clear_recorded_tab_panels();
                        settle(cx, win);
                        let toolbar = e2e::control_bounds(win.window_id(), "toolbar-row").unwrap();
                        assert!(toolbar.top() > px(0.), "toolbar must be below the always-visible titlebar tabs (#1462), got {toolbar:?}");
                        check_chrome(win, "toolbar-row", "toolbar-drag-region", zoom, true);
                        for live_zoom in [1., 1.67, 1.] {
                            theme::set_zoom(live_zoom);
                            app.update(cx, |_, cx| cx.notify());
                            settle(cx, win);
                            crate::macos::request_native_frame(cx, win);
                            check_native_lights(win);
                        }
                        theme::set_zoom(zoom);
                        app.update(cx, |_, cx| cx.notify());
                        settle(cx, win);
                        assert!(
                            kagi::ui::tab_panel_a11y::recorded_tab_panel("repo-tab-panel")
                                .is_some()
                        );
                        check_tab(win, "repo-tab-0");
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-t");
                        cx.run_until_parked();
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        settle(cx, win);
                        check_chrome(win, "home-frame", "home-drag-region", zoom, true);
                        check_tab(win, "home-tab");
                        assert!(
                            kagi::ui::tab_panel_a11y::recorded_tab_panel("home-tab-panel")
                                .is_some()
                        );
                        assert_eq!(
                            e2e::control_bounds(win.window_id(), "home-frame")
                                .unwrap()
                                .size
                                .height,
                            toolbar.size.height
                        );
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        cx.run_until_parked();
                        assert!(!app.read_with(cx, |app, _| app.home_in_front()));
                        app.update(cx, |app, cx| {
                            assert!(app.open_repository(second.path().to_path_buf(), cx));
                        });
                        settle(cx, win);
                        check_tab(win, "repo-tab-0");
                        check_tab(win, "repo-tab-1");
                        let add = e2e::control_bounds(win.window_id(), "tab-add").unwrap();
                        cx.simulate_click(win, add.center(), gpui::Modifiers::none());
                        cx.run_until_parked();
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        cx.run_until_parked();
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        settle(cx, win);
                        assert_eq!(app.read_with(cx, |app, _| app.tabs.len()), 1);
                        check_tab(win, "repo-tab-0");
                        focus_root(cx, win, &app);
                        cx.simulate_keystrokes(win, "cmd-w");
                        settle(cx, win);
                        assert!(app.read_with(cx, |app, _| app.home_in_front()));
                        check_tab(win, "home-tab");
                        assert!(
                            e2e::control_bounds(win.window_id(), "home-tab-close").is_none(),
                            "permanent Home has no inert close control"
                        );
                        check_chrome(win, "home-frame", "home-drag-region", zoom, false);
                        unmount(cx, app, win);
                    }
                }
            }
        }
    }
}

/// #1339/#1462: resizing must not make content width its own overflow budget.
pub fn scenario_titlebar_tabs_resize(cx: &mut VisualTestAppContext) {
    let _restore = crate::toolbar_overflow::ToolbarRestore::capture(cx);
    let first = build_fixture();
    let second = build_fixture();
    for slug in ["apple-light", "apple-dark"] {
        for language in ["en", "ja"] {
            std::env::set_var("KAGI_LANG", language);
            i18n::init_lang();
            for zoom in [1., 1.67] {
                theme::set_zoom(zoom);
                let state = e2e::app_state(first.path()).unwrap();
                let captured = Rc::new(RefCell::new(None));
                let output = captured.clone();
                let win: AnyWindowHandle = crate::macos::open_offscreen(
                    cx,
                    size(px(1392.), px(660.)),
                    move |window, cx| e2e::mount_root(state, window, cx, &output),
                )
                .into();
                let app = captured.borrow().clone().unwrap();
                app.update(cx, |app, cx| {
                    app.set_theme(slug, cx);
                    assert!(app.open_repository(second.path().to_path_buf(), cx));
                    for (index, tab) in app.tabs.iter_mut().enumerate() {
                        tab.name = format!("repository-titlebar-resize-{index}-with-long-title");
                    }
                    cx.notify();
                });
                settle(cx, win);
                let natural = e2e::control_bounds(win.window_id(), "repo-tab-0").unwrap();
                crate::toolbar_overflow::resize_overflow_window(cx, win, 300. * zoom);
                settle(cx, win);
                assert!(
                    e2e::control_bounds(win.window_id(), "tab-overflow").is_some(),
                    "narrow titlebar must overflow"
                );
                let narrow = e2e::control_bounds(win.window_id(), "repo-tab-0").unwrap();
                crate::toolbar_overflow::resize_overflow_window(cx, win, 1392.);
                e2e::clear_control_bounds(win.window_id(), "tab-overflow");
                settle(cx, win);
                assert!(
                    e2e::control_bounds(win.window_id(), "tab-overflow").is_none(),
                    "widening the titlebar must retire » without changing the tab signature"
                );
                let wide = e2e::control_bounds(win.window_id(), "repo-tab-0").unwrap();
                assert!(
                    wide.size.width > narrow.size.width,
                    "restore natural tab width"
                );
                assert_eq!(wide.size.width, natural.size.width);
                check_exact_fit(cx, win);
                unmount(cx, app, win);
            }
        }
    }
}

fn check_exact_fit(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    let scroll = e2e::tab_strip::scroll().unwrap();
    let first = scroll.bounds_for_item(0).unwrap();
    let last = scroll.bounds_for_item(1).unwrap();
    let content = last.right() - first.left();
    let add = e2e::control_bounds(win.window_id(), "tab-add").unwrap();
    let scale = cx
        .update_window(win, |_, window, _| window.scale_factor())
        .unwrap();
    let spacer = px((f32::from(theme::scaled_px(8.)) * scale).round() / scale);
    // AppKit's whole-logical-pixel frame rounds an exact device-grid fit up
    // by at most half a logical pixel on this Retina display.
    let width = f32::from(content + add.size.width + spacer + px(80.)).ceil();
    let budget = px(width) - add.size.width - spacer - px(80.);
    assert!(budget >= content && budget - content < px(1.));
    crate::toolbar_overflow::resize_overflow_window(cx, win, width);
    e2e::clear_control_bounds(win.window_id(), "tab-overflow");
    settle(cx, win);
    assert!(
        e2e::control_bounds(win.window_id(), "tab-overflow").is_none(),
        "natural titlebar contents fit the device-grid budget: content={content:?}, budget={budget:?}"
    );
    let first_after = e2e::control_bounds(win.window_id(), "repo-tab-0").unwrap();
    let last_after = e2e::control_bounds(win.window_id(), "repo-tab-1").unwrap();
    contained(scroll.bounds(), first_after, "exact-fit first document");
    contained(scroll.bounds(), last_after, "exact-fit last document");
}

fn settle(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    e2e::clear_control_bounds(win.window_id(), "home-tab-close");
    for id in [
        "repo-tab-0",
        "repo-tab-1",
        "home-tab",
        "tab-add",
        "toolbar-identity",
        "tb-refresh",
        "tb-pull",
        "tb-push",
        "tb-branch",
        "tb-stash",
        "tb-pop",
        "tb-undo",
        "tb-redo",
        "tb-terminal",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
        "tb-overflow",
    ] {
        e2e::clear_control_bounds(win.window_id(), id);
    }
    for _ in 0..8 {
        crate::toolbar_overflow::frame(cx, win);
    }
}

fn focus_root(cx: &mut VisualTestAppContext, win: AnyWindowHandle, app: &gpui::Entity<KagiApp>) {
    cx.update_window(win, |_, window, cx| {
        app.read(cx).root_focus.clone().unwrap().focus(window, cx);
    })
    .unwrap();
}

fn check_tab(win: AnyWindowHandle, id: &str) {
    let strip = e2e::control_bounds(win.window_id(), "repo-tab-strip").unwrap();
    let tab = e2e::control_bounds(win.window_id(), id).unwrap();
    contained(strip, tab, id);
    assert!(
        tab.left() >= px(80.),
        "native traffic-light exclusion: {id}: {tab:?}"
    );
}

fn check_chrome(win: AnyWindowHandle, row_id: &str, drag_id: &str, zoom: f32, has_add: bool) {
    let bounds = |id| e2e::control_bounds(win.window_id(), id).unwrap();
    let strip = bounds("repo-tab-strip");
    let row = bounds(row_id);
    assert_eq!(strip.top(), px(0.), "document strip is first");
    assert!(
        (f32::from(strip.size.height + px(1.)) - 36.).abs() <= 0.5,
        "fixed 36 logical px titlebar at {zoom}: {strip:?}"
    );
    assert_eq!(
        row.top(),
        strip.bottom() + px(1.),
        "toolbar directly follows titlebar's literal 1px separator"
    );
    if zoom == 1. {
        assert!(row.size.height <= px(54.), "toolbar heights unchanged");
    }
    let add = e2e::control_bounds(win.window_id(), "tab-add");
    assert_eq!(add.is_some(), has_add, "bare Home has no inert +");
    let title_drag = bounds("titlebar-drag-region");
    let toolbar_drag = bounds(drag_id);
    contained(strip, title_drag, "blank titlebar drag target");
    contained(row, toolbar_drag, "blank toolbar drag target");
    assert!(title_drag.size.width > px(0.));
    assert!(toolbar_drag.size.width > px(0.));
    if let Some(add) = add {
        contained(strip, add, "+ belongs to the document strip");
        assert!(
            title_drag.left() >= add.right(),
            "+ precedes the blank titlebar spacer"
        );
    }
    for id in [
        "repo-tab-0",
        "repo-tab-1",
        "home-tab",
        "tab-add",
        "toolbar-identity",
        "tb-refresh",
        "tb-pull",
        "tb-push",
        "tb-branch",
        "tb-stash",
        "tb-pop",
        "tb-undo",
        "tb-redo",
        "tb-terminal",
        "tb-graph-mode",
        "tb-pr-mode",
        "tb-editor-ws",
        "tb-ecosystem",
        "tb-settings",
        "tb-overflow",
    ] {
        if let Some(control) = e2e::control_bounds(win.window_id(), id) {
            if control.size.width <= px(0.) {
                continue;
            }
            for drag in [title_drag, toolbar_drag] {
                assert!(
                    control.right() <= drag.left()
                        || control.left() >= drag.right()
                        || control.bottom() <= drag.top()
                        || control.top() >= drag.bottom(),
                    "{id} intersects blank drag target: {control:?}, {drag:?}"
                );
            }
        }
    }
}

/// Read actual AppKit button bounds, not an app-owned geometry proxy.
fn check_native_lights(win: AnyWindowHandle) {
    use std::ffi::{c_char, c_void, CStr};
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    let title = format!("Kagi native-frame test {:?}", win.window_id());
    let strip = e2e::control_bounds(win.window_id(), "repo-tab-strip").unwrap();
    unsafe {
        type Object = *mut c_void;
        let send: unsafe extern "C" fn(Object, Object) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let indexed: unsafe extern "C" fn(Object, Object, usize) -> Object =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let count: unsafe extern "C" fn(Object, Object) -> usize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let rect: unsafe extern "C" fn(Object, Object) -> Rect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let convert: unsafe extern "C" fn(Object, Object, Rect, Object) -> Rect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let app = send(
            objc_getClass(c"NSApplication".as_ptr()),
            sel_registerName(c"sharedApplication".as_ptr()),
        );
        let windows = send(app, sel_registerName(c"windows".as_ptr()));
        let mut found = false;
        for index in 0..count(windows, sel_registerName(c"count".as_ptr())) {
            let window = indexed(windows, sel_registerName(c"objectAtIndex:".as_ptr()), index);
            let name = send(
                send(window, sel_registerName(c"title".as_ptr())),
                sel_registerName(c"UTF8String".as_ptr()),
            );
            if name.is_null() || CStr::from_ptr(name.cast()).to_bytes() != title.as_bytes() {
                continue;
            }
            assert!(!found, "unique native test window");
            found = true;
            let content = send(window, sel_registerName(c"contentView".as_ptr()));
            let content_bounds = rect(content, sel_registerName(c"bounds".as_ptr()));
            for kind in 0..3 {
                let button = indexed(
                    window,
                    sel_registerName(c"standardWindowButton:".as_ptr()),
                    kind,
                );
                assert!(!button.is_null(), "native traffic light {kind} exists");
                let local = rect(button, sel_registerName(c"bounds".as_ptr()));
                let frame = convert(
                    button,
                    sel_registerName(c"convertRect:toView:".as_ptr()),
                    local,
                    content,
                );
                let top = content_bounds.size.height - frame.origin.y - frame.size.height;
                let center = top + frame.size.height / 2.;
                let row_center =
                    f64::from(f32::from(strip.top() + (strip.size.height + px(1.)) / 2.));
                assert!((center - row_center).abs() <= 2., "native light {kind} center={center}, titlebar center={row_center}, strip={strip:?}");
                assert!(
                    frame.origin.x + frame.size.width <= 80.,
                    "native light exclusion"
                );
                assert!(
                    frame.size.width > 0.
                        && frame.size.width <= 20.
                        && frame.size.height > 0.
                        && frame.size.height <= 20.,
                    "native lights stay unscaled"
                );
            }
        }
        assert!(found, "actual AppKit window found");
    }
}
