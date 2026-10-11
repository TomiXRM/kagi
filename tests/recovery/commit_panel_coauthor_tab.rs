//! Real Tab dismissal from populated and empty production co-author popups.
use super::*;
use kagi::ui::commit_panel::CommitPanelView;

pub(super) fn check_tab_dismiss(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<CommitPanelView>,
) {
    let opener = labels::focus_handle("cp-coauthor").unwrap();
    let message = cx.read(|cx| panel.read(cx).committable_message(cx));
    for activation in ["enter", "space"] {
        for departure in ["tab", "shift-tab"] {
            assert_focus(cx, window, "cp-coauthor");
            press(cx, window, activation);
            let count = cx.read(|cx| panel.read(cx).coauthor_menu.as_ref().unwrap().len());
            if count == 0 {
                assert_focus(cx, window, "cp-coauthor-empty");
            } else {
                assert_focus(cx, window, "cp-coauthor-row-0");
                if activation == "space" {
                    press(cx, window, "end");
                    assert_focus(cx, window, &format!("cp-coauthor-row-{}", count - 1));
                }
            }

            // Inspect before another frame: Tab must close before restoring
            // focus, not leave a live popup behind Root's focus traversal.
            let keystroke = gpui::Keystroke::parse(departure).unwrap();
            cx.dispatch_keystroke(window, keystroke.clone());
            assert!(
                cx.read(|cx| panel.read(cx).coauthor_menu.is_none()),
                "{departure} must synchronously dismiss the co-author popup"
            );
            assert!(
                cx.update_window(window, |_, window, _| opener.is_focused(window))
                    .unwrap(),
                "{departure} must restore the original production opener handle"
            );
            cx.simulate_event(window, gpui::KeyUpEvent { keystroke });
            paint(cx, window);
            assert!(cx.read(|cx| panel.read(cx).coauthor_menu.is_none()));
            assert_focus(cx, window, "cp-coauthor");
            assert!(
                cx.update_window(window, |_, window, _| opener.is_focused(window))
                    .unwrap(),
                "the opener handle must remain stable after repaint"
            );
            assert_eq!(
                cx.read(|cx| panel.read(cx).committable_message(cx)),
                message
            );
        }
    }
}
