//! Native AccessKit inspection and AppKit activation for hidden Tier A windows.
use gpui::{AnyWindowHandle, ElementId, VisualTestAppContext};
use serde_json::Value;

pub fn activate(cx: &mut VisualTestAppContext) {
    cx.update(|_| unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let view: *mut AnyObject = msg_send![window, contentView];
            if !view.is_null() {
                let _: *mut AnyObject = msg_send![view, accessibilityChildren];
            }
        }
    });
}

pub fn tree(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> Value {
    let (active, json) = cx
        .update_window(win, |_, w, _| {
            (w.is_a11y_active(), w.debug_a11y_tree_json())
        })
        .unwrap();
    let json = json.unwrap_or_else(|| {
        panic!(
            "native accessibility adapter must produce a real tree: window={:?}, active={active}",
            win.window_id()
        )
    });
    serde_json::from_str(&json).unwrap()
}

pub fn node(cx: &mut VisualTestAppContext, win: AnyWindowHandle, element: ElementId) -> Value {
    let tree = tree(cx, win);
    let identity = format!("{element:?}");
    tree["nodes"]
        .as_object()
        .unwrap()
        .values()
        .find(|node| node["element_id"].as_str() == Some(identity.as_str()))
        .cloned()
        .unwrap_or_else(|| panic!("missing actual AX node {identity}: {tree}"))
}

/// The focused semantic node, never an enclosing named Group.
pub fn focused(cx: &mut VisualTestAppContext, win: AnyWindowHandle) -> Value {
    let tree = tree(cx, win);
    let id = tree["gpui_focus"]
        .as_str()
        .unwrap_or_else(|| panic!("missing native focus: {tree}"));
    tree["nodes"][id].clone()
}

/// Traverse the real focus tree to a rendered control (including editor fields).
pub fn focus_named(cx: &mut VisualTestAppContext, win: AnyWindowHandle, role: &str, label: &str) {
    for _ in 0..200 {
        cx.update_window(win, |_, window, cx| {
            window.focus_next(cx);
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
        let tree = tree(cx, win);
        let Some(id) = tree["gpui_focus"].as_str() else {
            continue; // The window root and nonsemantic focus stops have no AX node.
        };
        let node = &tree["nodes"][id];
        if node["aria"]["role"] == role && node["aria"]["label"] == label {
            return;
        }
    }
    panic!(
        "focus traversal did not reach {role} {label}: {}",
        tree(cx, win)
    );
}

/// Deliver the standard native Press action, not a renderer callback or key.
pub fn press(cx: &mut VisualTestAppContext, win: AnyWindowHandle, label: &str) -> bool {
    press_scoped(cx, win, label, None)
}

/// Press a named control only inside its named native accessibility ancestor.
pub fn press_in(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    scope: &str,
    label: &str,
) -> bool {
    press_scoped(cx, win, label, Some(scope))
}

fn press_scoped(
    cx: &mut VisualTestAppContext,
    win: AnyWindowHandle,
    label: &str,
    scope: Option<&str>,
) -> bool {
    // NSApplication can retain native windows after GPUI unmounts their roots.
    // Mark only this live window, so a stale adapter cannot consume the request.
    let marker = format!("kagi-native-ax-{:?}", win.window_id());
    let previous = cx
        .update_window(win, |_, w, _| {
            let previous = w.window_title();
            w.set_window_title(&marker);
            previous
        })
        .unwrap();
    let delivered = cx.update(|_| unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        unsafe fn find(
            object: *mut AnyObject,
            label: &str,
            scope: Option<&str>,
            mut inside: bool,
            depth: usize,
        ) -> bool {
            if object.is_null() || depth > 64 {
                return false;
            }
            let name: *mut AnyObject = unsafe { msg_send![object, accessibilityTitle] };
            if !name.is_null() {
                let bytes: *const std::ffi::c_char = unsafe { msg_send![name, UTF8String] };
                if !bytes.is_null() {
                    let name = unsafe { std::ffi::CStr::from_ptr(bytes) }.to_string_lossy();
                    inside |= scope == Some(name.as_ref());
                    if inside && name == label {
                        return unsafe { msg_send![object, accessibilityPerformPress] };
                    }
                }
            }
            let children: *mut AnyObject = unsafe { msg_send![object, accessibilityChildren] };
            if children.is_null() {
                return false;
            }
            let count: usize = unsafe { msg_send![children, count] };
            for index in 0..count {
                let child: *mut AnyObject = unsafe { msg_send![children, objectAtIndex: index] };
                if unsafe { find(child, label, scope, inside, depth + 1) } {
                    return true;
                }
            }
            false
        }
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let title: *mut AnyObject = msg_send![window, title];
            let bytes: *const std::ffi::c_char = msg_send![title, UTF8String];
            if bytes.is_null() || std::ffi::CStr::from_ptr(bytes).to_string_lossy() != marker {
                continue;
            }
            let view: *mut AnyObject = msg_send![window, contentView];
            if find(view, label, scope, scope.is_none(), 0) {
                return true;
            }
        }
        false
    });
    cx.update_window(win, |_, w, _| w.set_window_title(&previous))
        .unwrap();
    delivered
}
