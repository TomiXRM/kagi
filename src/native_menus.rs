//! Preserve AppKit's Window-menu role independently of translated display titles.
use gpui::{App, Menu};

pub fn publish(cx: &mut App, menus: Vec<Menu>, window_section: usize) {
    cx.set_menus(menus);
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::{class, msg_send, runtime::AnyObject};
        // SAFETY: publication runs on GPUI's main thread. The application owns
        // the newly installed menu/submenu; no native pointer escapes this call.
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let main: *mut AnyObject = msg_send![app, mainMenu];
        if main.is_null() {
            return;
        }
        let count: usize = msg_send![main, numberOfItems];
        if window_section >= count {
            return;
        }
        let item: *mut AnyObject = msg_send![main, itemAtIndex: window_section];
        let submenu: *mut AnyObject = msg_send![item, submenu];
        let _: () = msg_send![app, setWindowsMenu: submenu];
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window_section;
}
