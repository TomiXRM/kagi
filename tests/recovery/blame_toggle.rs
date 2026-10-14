//! #1524: the actual Blame chip is a named, keyboard-operated toggle button.
use crate::{native_ax, pr_viewed::paint};
use gpui::VisualTestAppContext;
use kagi_ui_core::i18n::Msg;

pub fn blame_toggle_a11y(cx: &mut VisualTestAppContext) {
    crate::blame_identity::matrix(cx, |cx, view, window, _| {
        native_ax::activate(cx);
        paint(cx, window);
        let node = native_ax::node(cx, window, "ews-blame-toggle".into());
        assert_eq!(
            node["aria"]["role"], "Button",
            "Blame must expose a button role"
        );
        assert_eq!(node["aria"]["label"], Msg::EditorWorkspaceBlame.t());
        assert_eq!(node["aria"]["selected"], true);
        let reached = cx
            .update_window(window, |_, window, cx| {
                for _ in 0..160 {
                    window.focus_next(cx);
                    window.draw(cx).clear();
                    let tree: serde_json::Value =
                        serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
                    if let Some(key) = tree["gpui_focus"].as_str() {
                        if tree["nodes"][key]["element_id"]
                            == format!("{:?}", gpui::ElementId::from("ews-blame-toggle"))
                        {
                            return true;
                        }
                    }
                }
                false
            })
            .unwrap();
        assert!(reached, "Tab traversal must reach the actual Blame chip");
        for (key, selected) in [
            ("space", false),
            ("enter", true),
            ("enter", false),
            ("space", true),
        ] {
            crate::keyboard_nav::keys(cx, window, key);
            paint(cx, window);
            cx.read(|cx| assert_eq!(view.read(cx).show_blame, selected));
            let node = native_ax::node(cx, window, "ews-blame-toggle".into());
            assert_eq!(node["aria"]["selected"], selected);
            assert_eq!(node["aria"]["label"], Msg::EditorWorkspaceBlame.t());
            let tree = native_ax::tree(cx, window);
            let focused = tree["gpui_focus"].as_str().expect("toggle retains focus");
            assert_eq!(
                tree["nodes"][focused]["element_id"],
                format!("{:?}", gpui::ElementId::from("ews-blame-toggle"))
            );
        }
        assert!(native_ax::press(cx, window, Msg::EditorWorkspaceBlame.t()));
        paint(cx, window);
        cx.read(|cx| {
            assert!(
                !view.read(cx).show_blame,
                "native Press shares the toggle path"
            )
        });
        assert_eq!(
            native_ax::node(cx, window, "ews-blame-toggle".into())["aria"]["selected"],
            false
        );
    });
}
