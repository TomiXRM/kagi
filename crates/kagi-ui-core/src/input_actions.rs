//! Public Input action observation shared by the repository and Settings roots.
use gpui::{App, EntityInputHandler, Focusable, Window};
use gpui_component::WindowExt;

/// Observe composition at action capture, before the child can unmark it.
///
/// `focused_input` can briefly retain the previous editor until its next paint.
/// Check its actual focus handle so a button never edits that hidden editor.
/// The caller overwrites its window-owned flag on every capture and propagates;
/// only the later bubble consumes an Escape that began with marked text. Leave
/// menus, completions and IME cancellation to the child's normal priority order.
pub fn focused_input_has_marked_text(window: &mut Window, cx: &mut App) -> bool {
    let Some(input) = window.focused_input(cx) else {
        return false;
    };
    if !input.read(cx).focus_handle(cx).is_focused(window) {
        return false;
    }
    input.update(cx, |input, cx| {
        input.marked_text_range(window, cx).is_some()
    })
}
