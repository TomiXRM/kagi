//! #985 — the context-menu key and the keyboard inside a context menu.
//!
//! Shift+F10 on the window opens the selected commit's menu at its row's
//! bottom-left, with the focus on the first enabled item; ↑/↓/Home/End move
//! among the enabled items only (the menu's last item, Reset, is always
//! disabled); Enter presses the focused item; Escape closes the menu, and
//! either way the focus goes back to the window it came from.
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, mount, unmount};

/// The open menu's focused item and enabled items (slots), as of a fresh
/// frame.
fn menu(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> (Option<usize>, Vec<usize>) {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx).menu_keys_for_e2e(window)
    })
    .unwrap()
}

fn root_focused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx)
            .root_focus
            .as_ref()
            .is_some_and(|focus| focus.is_focused(window))
    })
    .unwrap()
}

/// Shift+F10 from the window: the selected commit's menu opens below its
/// row, its first enabled item focused.
fn open_from_key(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    let anchor = cx
        .update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            app.read(cx).context_anchor_for_e2e()
        })
        .unwrap()
        .expect("the selected row records where it is drawn");
    keys(cx, window, "shift-f10");
    let state = cx.read(|cx| app.read(cx).commit_menu.clone());
    let state = state.expect("Shift+F10 opens the selected commit's menu");
    assert_eq!(state.row_index, 0);
    assert_eq!(
        state.position, anchor,
        "the menu opens at the row's bottom-left"
    );
    let (focused, enabled) = menu(cx, app, window);
    assert_eq!(
        focused,
        enabled.first().copied(),
        "the first enabled item has the focus"
    );
}

pub fn scenario_context_menu_keys(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
        app.update(cx, |app, cx| {
            app.select(0);
            cx.notify();
        });
        let root = app.read(cx).root_focus.clone().expect("root focus");
        root.focus(window, cx);
    })
    .unwrap();

    open_from_key(cx, &app, window);
    let window_id = window.window_id();
    // Reset (the last group's only item) is drawn and disabled.
    assert!(e2e::control_bounds(window_id, "commit-menu-item-5-0").is_some());
    let (_, enabled) = menu(cx, &app, window);
    let last = *enabled.last().expect("enabled items");
    assert!(
        !enabled.contains(&(last + 1)),
        "a disabled item follows the last enabled one"
    );
    keys(cx, window, "end");
    assert_eq!(menu(cx, &app, window).0, Some(last));
    // ↓ passes the disabled Reset by and wraps to the first enabled item.
    keys(cx, window, "down");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[0]));
    keys(cx, window, "up");
    assert_eq!(menu(cx, &app, window).0, Some(last));
    keys(cx, window, "home");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[0]));

    // Enter presses the focused item: ↓ to "Copy SHA", a read-only item.
    keys(cx, window, "down");
    assert_eq!(menu(cx, &app, window).0, Some(enabled[1]));
    keys(cx, window, "enter");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(fixture.path())
        .output()
        .expect("git rev-parse");
    let head = String::from_utf8(head.stdout).unwrap();
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(head.trim().to_string()),
        "Enter on Copy SHA copied the commit's SHA"
    );
    assert!(
        root_focused(cx, &app, window),
        "the focus is back on the window"
    );

    // Escape closes the menu; the focus goes back where it came from.
    open_from_key(cx, &app, window);
    keys(cx, window, "escape");
    assert!(cx.read(|cx| app.read(cx).commit_menu.is_none()));
    assert!(
        root_focused(cx, &app, window),
        "Escape gives the focus back"
    );
    unmount(cx, app, window);
}
