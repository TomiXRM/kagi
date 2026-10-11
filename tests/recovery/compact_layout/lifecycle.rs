//! Review regressions: absent anchors, owner attachment and canonical wide tab.
use super::*;
use kagi::ui::BottomTab;

pub fn scenario_compact_layout_lifecycle(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    for locale in ["en", "ja"] {
        for preset in ["apple-light", "apple-dark"] {
            let case = Case {
                height: 660.,
                zoom: 1.67,
                locale,
                preset,
            };
            clean_shortcut(cx, case);
            owner_switches(cx, case);
        }
    }
}

fn root_focused(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window)
        })
        .unwrap(),
        "undrawn composer Inputs must yield focus to the root"
    );
}

fn clean_shortcut(cx: &mut VisualTestAppContext, case: Case) {
    let repo = build_fixture();
    let (app, win) = mount(cx, repo.path(), case);
    assert!(e2e::control_bounds(win.window_id(), "compact-wip-row").is_none());
    let tab = cx.read(|cx| app.read(cx).bottom_tab);
    focus_root(cx, &app, win);
    cx.simulate_keystrokes(win, "cmd-enter");
    settle(cx, win);
    assert!(
        cx.read(|cx| app.read(cx).ui().commit_panel.is_none()),
        "Cmd+Enter without WIP must not attach/focus an undrawn composer"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_tab),
        tab,
        "Cmd+Enter without WIP must not replace the bottom tab"
    );
    root_focused(cx, &app, win);
    // Exercise the defensive dismissal too: an existing API may open before
    // the row disappears, without any kit Popover having painted yet.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| app.open_commit_panel(window, cx));
    })
    .unwrap();
    settle(cx, win);
    assert!(e2e::control_bounds(win.window_id(), "compact-composer").is_none());
    root_focused(cx, &app, win);
    unmount(cx, app, win);
}

fn owner_switches(cx: &mut VisualTestAppContext, case: Case) {
    let first = build_fixture();
    let second = build_fixture();
    std::fs::write(
        first.path().join("README.md"),
        "tracked first line\ntracked second line\n",
    )
    .unwrap();
    std::fs::write(second.path().join("second-owner.txt"), "second owner WIP\n").unwrap();
    let (app, win) = mount(cx, first.path(), case);
    let original = changes(cx, &app, win);
    set_draft(cx, &original, win);
    let before = draft(cx, &original);
    let id = original.entity_id();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(second.path().to_path_buf(), cx));
    });
    await_owner_panel(cx, &app, win, second.path());
    columns(win, case.dimensions());
    let new_panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .unwrap();
    assert_ne!(
        new_panel.entity_id(),
        id,
        "new tab owns its own panel, never the old draft"
    );
    assert_eq!(
        cx.read(|cx| new_panel.read(cx).owner),
        cx.read(|cx| app.read(cx).active_session().unwrap())
    );
    assert!(
        draft(cx, &new_panel).0.is_empty(),
        "new owner must not inherit the first owner's draft"
    );
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    await_owner_panel(cx, &app, win, first.path());
    columns(win, case.dimensions());
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        id
    );
    assert_eq!(
        draft(cx, &original),
        before,
        "switching back retains the original unsent draft"
    );
    hover_stat(cx, &app, win);
    wide_tab_toggle(cx, &app, win, case.height);
    drop(new_panel);
    drop(original);
    unmount(cx, app, win);
}

fn await_owner_panel(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
) {
    let expected = repo.canonicalize().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(25);
    loop {
        settle(cx, win);
        let ready = cx.read(|cx| {
            let app = app.read(cx);
            app.ui().commit_panel.as_ref().is_some_and(|panel| {
                let panel = panel.read(cx);
                panel.repo_path == expected && Some(panel.owner) == app.active_session()
            })
        });
        if ready {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "visible Changes must attach its current owner without another tab click"
        );
        cx.advance_clock(Duration::from_millis(100));
    }
    assert_eq!(cx.read(|cx| app.read(cx).bottom_tab), BottomTab::Changes);
    assert!(
        e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "automatic Changes attach does not pin a composer"
    );
}

fn hover_stat(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    app.update(cx, |app, cx| app.refresh_working_tree_external(cx));
    settle(cx, win);
    let row = bounds(win, "compact-wip-row");
    cx.simulate_mouse_move(win, row.center(), None, Modifiers::none());
    settle(cx, win);
    let action = bounds(win, "compact-wip-affordance");
    let stat = bounds(win, "compact-wip-diffstat");
    assert!(
        stat.size.width > px(0.),
        "tracked fixture produces real WIP diffstat paint/bounds"
    );
    contained(row, action, "inline Commit reveal");
    contained(row, stat, "live diffstat");
    assert!(
        action.right() <= stat.left() + px(1.),
        "Commit reveal and diffstat must never intersect: {action:?}, {stat:?}"
    );
}

fn wide_tab_toggle(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    height: f32,
) {
    resize(cx, win, (1392., height));
    assert_eq!(
        cx.read(|cx| app.read(cx).bottom_tab),
        BottomTab::OperationLog,
        "wide mode canonicalizes stored Changes to the tab actually displayed"
    );
    assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
    click(cx, win, "status-icon-oplog");
    assert!(
        !cx.read(|cx| app.read(cx).bottom_panel_open),
        "first click on the visible Operation Log toggle closes it"
    );
}
