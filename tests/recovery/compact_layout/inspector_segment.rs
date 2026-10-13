//! #1495: the shared historical Inspector selector is a roving tab list.
use super::super::*;
use super::{inspector_fixture, MESSAGE};

pub fn scenario_compact_inspector_segment_keys(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = inspector_fixture();
    let before = crate::macos::repo_fingerprint(fixture.path());
    matrix(|case| {
        if case.height != 660. {
            return;
        }
        let (app, win) = mount(cx, fixture.path(), case);
        let panel = changes(cx, &app, win);
        let row = cx.read(|cx| {
            app.read(cx)
                .view()
                .rows
                .iter()
                .position(|row| row.summary.as_ref() == MESSAGE.lines().next().unwrap())
                .unwrap()
        });
        app.update(cx, |app, cx| {
            app.inspector_tree_view = false;
            app.select(row);
            cx.notify();
        });
        settle(cx, win);
        assert_ax("toggle-path", gpui::Role::Tab, "Path");
        assert_ax("toggle-tree", gpui::Role::Tab, "Tree");
        assert_ax(
            "files-toggle-row",
            gpui::Role::TabList,
            kagi_ui_core::i18n::Msg::A11yInspectorFiles.t(),
        );
        // Enter the historical selector through real Tab navigation from root.
        cx.update_window(win, |_, window, cx| {
            let root = app.read(cx).root_focus.clone().unwrap();
            root.focus(window, cx);
        })
        .unwrap();
        for _ in 0..32 {
            press_key(cx, win, "tab");
            if cx
                .update_window(win, |_, window, _| {
                    e2e::commit_panel_labels::focused("toggle-path", window)
                })
                .unwrap()
            {
                break;
            }
        }
        assert_control_focus(cx, win, "toggle-path");
        click(cx, win, "inspector-file-0");
        let selected = cx.read(|cx| {
            app.read(cx)
                .ui()
                .main_diff
                .as_ref()
                .map(|diff| diff.entity_id())
        });
        focus_control(cx, win, "toggle-path");
        for (key, id, tree) in [
            ("right", "toggle-tree", true),
            ("enter", "toggle-tree", true),
            ("space", "toggle-tree", true),
            ("left", "toggle-path", false),
            ("enter", "toggle-path", false),
            ("space", "toggle-path", false),
        ] {
            press_key(cx, win, key);
            assert_control_focus(cx, win, id);
            assert_eq!(cx.read(|cx| app.read(cx).inspector_tree_view), tree);
            assert_eq!(
                e2e::commit_panel_labels::accessible_selected("toggle-tree"),
                Some(tree)
            );
            assert_eq!(
                e2e::commit_panel_labels::accessible_selected("toggle-path"),
                Some(!tree)
            );
            assert_eq!(
                cx.read(|cx| app
                    .read(cx)
                    .ui()
                    .main_diff
                    .as_ref()
                    .map(|diff| diff.entity_id())),
                selected
            );
            assert_eq!(
                kagi::ui::list_a11y::recorded_list("inspector-files-scroll")
                    .unwrap()
                    .role,
                Some(if tree {
                    gpui::Role::Tree
                } else {
                    gpui::Role::ListBox
                })
            );
        }
        press_key(cx, win, "tab");
        assert!(!cx
            .update_window(win, |_, window, _| e2e::commit_panel_labels::focused(
                "toggle-tree",
                window
            ))
            .unwrap());
        press_key(cx, win, "shift-tab");
        assert_control_focus(cx, win, "toggle-path");
        resize(cx, win, (1392., 883.));
        assert_control_focus(cx, win, "toggle-path");
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .ui()
                .main_diff
                .as_ref()
                .map(|diff| diff.entity_id())),
            selected
        );
        resize(cx, win, (940., 660.));
        app.update(cx, |app, cx| {
            app.close_main_diff();
            cx.notify();
        });
        settle(cx, win);
        focus_control(cx, win, "toggle-tree");
        click(cx, win, "compact-wip-row");
        assert!(
            !cx.update_window(win, |_, window, _| e2e::commit_panel_labels::focused(
                "toggle-tree",
                window
            ))
            .unwrap(),
            "hidden historical selector yields focus"
        );
        app.update(cx, |app, cx| {
            app.select(row);
            cx.notify();
        });
        settle(cx, win);
        focus_control(cx, win, "toggle-path");
        press_key(cx, win, "right");
        assert!(cx.read(|cx| app.read(cx).inspector_tree_view));
        drop(panel);
        unmount(cx, app, win);
    });
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
}
