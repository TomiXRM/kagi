//! Initial Graph filter keyboard entry (#1309).
use gpui::{Focusable, VisualTestAppContext};
use kagi::ui::sidebar::SidebarRow;

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::paint;

pub fn scenario_sidebar_filter_tab_entry(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "filter-target"]);
    let (app, window) = mount(cx, fixture.path());
    paint(cx, window);
    let input = cx
        .read(|cx| app.read(cx).sidebar.filter.clone())
        .expect("fresh sidebar renders an Input from its first frame");
    let input_id = input.entity_id();
    let mut reached = false;
    for _ in 0..100 {
        keys(cx, window, "tab");
        reached = cx
            .update_window(window, |_, window, cx| {
                input.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap();
        if reached {
            break;
        }
    }
    assert!(reached, "Tab reaches the initial sidebar filter");
    keys(cx, window, "f i l t e r - t a r g e t");
    paint(cx, window);
    cx.read(|cx| {
        let rows = &app.read(cx).sidebar.rows;
        assert!(rows.iter().any(
            |row| matches!(row, SidebarRow::LocalBranchLeaf { name, .. } if name == "filter-target")
        ));
        assert!(!rows
            .iter()
            .any(|row| matches!(row, SidebarRow::LocalBranchLeaf { name, .. } if name == "main")));
    });
    cx.update_window(window, |_, window, cx| {
        assert!(
            input.read(cx).focus_handle(cx).is_focused(window),
            "typing keeps filter focus"
        );
    })
    .unwrap();
    keys(cx, window, "escape");
    paint(cx, window);
    assert!(
        cx.read(|cx| input.read(cx).value().is_empty()),
        "Escape clears the filter"
    );
    cx.update_window(window, |_, window, cx| {
        assert!(
            app.read(cx).sidebar_row_focused_for_e2e(window).is_some(),
            "Escape returns focus to the list"
        );
    })
    .unwrap();
    cx.read(|cx| {
        assert!(
            app.read(cx).sidebar.rows.iter().any(|row| {
                matches!(row, SidebarRow::LocalBranchLeaf { name, .. } if name == "main")
            }),
            "clearing restores the original rows"
        );
    });
    keys(cx, window, "cmd-f");
    cx.update_window(window, |_, window, cx| {
        assert!(
            input.read(cx).focus_handle(cx).is_focused(window),
            "Cmd+F focuses the same field"
        );
    })
    .unwrap();
    assert_eq!(
        cx.read(|cx| app.read(cx).sidebar.filter.as_ref().unwrap().entity_id()),
        input_id
    );
    drop(input);
    unmount(cx, app, window);
}
