//! Equal, independently scrolling Changes allocations over the original panel.
use super::*;

pub fn scenario_compact_layout_changes(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    matrix(|case| {
        e2e::commit_panel_labels::clear();
        let (app, win) = mount(cx, fixture.path(), case);
        rail_graph(win, case.dimensions());
        let panel = changes(cx, &app, win);
        columns(win, case.dimensions());
        rail_graph(win, case.dimensions());
        header_controls(cx, &panel, win);
        independent_scroll(cx, &app, &panel, win);
        set_draft(cx, &panel, win);
        selector_keys(cx, &panel, win);
        let before = draft(cx, &panel);
        let identity = panel.entity_id();
        resize(cx, win, (1392., case.height));
        resize(cx, win, case.dimensions());
        let returned = changes(cx, &app, win);
        assert_eq!(
            returned.entity_id(),
            identity,
            "Changes retains original panel across width transitions"
        );
        assert_eq!(
            draft(cx, &returned),
            before,
            "Changes relocation retains original input draft"
        );
        columns(win, case.dimensions());
        drop(returned);
        drop(panel);
        unmount(cx, app, win);
    });
    bulk_controls(cx, fixture.path());
}

fn header_controls(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
) {
    for (column, list, tree, bulk) in [
        (
            "compact-changes-unstaged",
            "cp-view-list",
            "cp-view-tree",
            "cp-stage-all",
        ),
        (
            "compact-changes-staged",
            "cp-staged-view-list",
            "cp-staged-view-tree",
            "cp-unstage-all",
        ),
    ] {
        for control in [list, tree, bulk] {
            contained(
                bounds(win, column),
                bounds(win, control),
                "header control belongs to its column",
            );
        }
        click(cx, win, tree);
        assert!(
            cx.read(|cx| panel.read(cx).state.tree_view),
            "real Tree control updates original panel"
        );
        click(cx, win, list);
        assert!(
            !cx.read(|cx| panel.read(cx).state.tree_view),
            "real List control updates original panel"
        );
    }
}

fn selector_keys(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
) {
    use kagi_ui_core::i18n::Msg;
    let preserved = draft(cx, panel);
    let before_scroll = cx.read(|cx| {
        let view = panel.read(cx);
        (
            view.unstaged_scroll_handle.0.borrow().base_handle.offset(),
            view.staged_scroll_handle.0.borrow().base_handle.offset(),
        )
    });
    for (group, list, tree) in [
        ("cp-view-group", "cp-view-list", "cp-view-tree"),
        (
            "cp-staged-view-group",
            "cp-staged-view-list",
            "cp-staged-view-tree",
        ),
    ] {
        assert_ax(group, gpui::Role::TabList, Msg::CommitPanelTitle.t());
        assert_ax(list, gpui::Role::Tab, Msg::CommitPanelList.t());
        assert_ax(tree, gpui::Role::Tab, Msg::CommitPanelTree.t());
        let initial = bounds(win, list);
        focus_control(cx, win, list);
        for (key, id, selected) in [("right", tree, true), ("home", list, false)] {
            press_key(cx, win, key);
            assert_control_focus(cx, win, id);
            assert_eq!(cx.read(|cx| panel.read(cx).state.tree_view), selected);
            assert_eq!(
                e2e::commit_panel_labels::accessible_selected(tree),
                Some(selected)
            );
            assert_eq!(
                e2e::commit_panel_labels::accessible_selected(list),
                Some(!selected)
            );
            assert_eq!(bounds(win, list), initial, "selector focus cannot reflow");
            assert_eq!(
                draft(cx, panel),
                preserved,
                "selectors retain the same draft"
            );
        }
    }
    assert_eq!(
        cx.read(|cx| {
            let view = panel.read(cx);
            (
                view.unstaged_scroll_handle.0.borrow().base_handle.offset(),
                view.staged_scroll_handle.0.borrow().base_handle.offset(),
            )
        }),
        before_scroll,
        "both keyboard selectors retain independent scroll offsets"
    );
}

fn bulk_controls(cx: &mut VisualTestAppContext, repo: &Path) {
    let case = Case {
        height: 660.,
        zoom: 1.67,
        locale: "ja",
        preset: "apple-dark",
    };
    let (app, win) = mount(cx, repo, case);
    let panel = changes(cx, &app, win);
    let initial = index(repo);
    click(cx, win, "cp-stage-all");
    await_files(cx, &panel, win, (0, FILES_PER_SIDE * 2));
    assert_ne!(index(repo), initial, "real Stage all updates Git index");
    let staged = index(repo);
    click(cx, win, "cp-stage-all");
    assert_eq!(
        index(repo),
        staged,
        "empty Unstaged header keeps disabled Stage all inert"
    );
    click(cx, win, "cp-unstage-all");
    await_files(cx, &panel, win, (FILES_PER_SIDE * 2, 0));
    let unstaged = index(repo);
    assert_ne!(unstaged, staged, "real Unstage all updates Git index");
    click(cx, win, "cp-unstage-all");
    assert_eq!(
        index(repo),
        unstaged,
        "empty Staged header keeps disabled Unstage all inert"
    );
    columns(win, case.dimensions());
    drop(panel);
    unmount(cx, app, win);
}

fn await_files(
    cx: &mut VisualTestAppContext,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    expected: (usize, usize),
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    while cx.read(|cx| {
        let state = &panel.read(cx).state;
        (state.unstaged.len(), state.staged.len())
    }) != expected
    {
        assert!(
            std::time::Instant::now() < deadline,
            "real Git operation must publish {expected:?}"
        );
        cx.advance_clock(Duration::from_millis(100));
        settle(cx, win);
    }
    settle(cx, win);
}
