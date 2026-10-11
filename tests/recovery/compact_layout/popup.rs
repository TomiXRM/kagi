//! Pinned composer interaction oracles over the shared real-root fixture.
use super::*;
fn closed(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    before: &(String, String, String),
) {
    assert!(
        e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
        "popup must close"
    );
    assert!(
        e2e::control_bounds(win.window_id(), "compact-popup-close").is_none(),
        "close control must unmount"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        panel.entity_id(),
        "closing retains panel entity"
    );
    assert_eq!(
        &draft(cx, panel),
        before,
        "closing retains title/body/effective draft"
    );
}
pub fn scenario_compact_layout_popup(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        assert!(
            e2e::control_bounds(win.window_id(), "compact-composer").is_none(),
            "popup starts closed"
        );
        open_wip(cx, win);
        let panel = cx
            .read(|cx| app.read(cx).ui().commit_panel.clone())
            .unwrap();
        set_draft(cx, &panel, win);
        let before = draft(cx, &panel);
        popup_geometry(win, case.dimensions());
        rail_graph(win, case.dimensions());
        let composer_before = bounds(win, "compact-composer");
        let rail = bounds(win, "compact-rail");
        cx.simulate_mouse_move(win, rail.center(), None, Modifiers::none());
        cx.advance_clock(Duration::from_millis(500));
        settle(cx, win);
        assert_eq!(
            bounds(win, "compact-composer"),
            composer_before,
            "pointer leaving does not dismiss pinned popup"
        );
        escape(cx, &app, win);
        closed(cx, &app, &panel, win, &before);
        open_wip(cx, win);
        assert_eq!(draft(cx, &panel), before, "WIP reopen restores draft");
        click(cx, win, "compact-popup-close");
        closed(cx, &app, &panel, win, &before);
        keyboard_reopen(cx, &app, &panel, win, &before, case.dimensions());
        open_wip(cx, win);
        // The same popup is constrained again at a shorter native viewport.
        resize(cx, win, (WIDTH, 560.));
        popup_geometry(win, (WIDTH, 560.));
        assert_eq!(
            draft(cx, &panel),
            before,
            "short-height clamp retains draft"
        );
        resize(cx, win, case.dimensions());
        popup_geometry(win, case.dimensions());
        let graph = cx.read(|cx| app.read(cx).ui().commit_scroll_handle.clone());
        let graph_before = graph.0.borrow().base_handle.offset();
        let wip = bounds(win, "compact-wip-row");
        let list = graph.0.borrow().base_handle.bounds();
        // Stay outside the popup: use the actual graph owner's trailing edge.
        wheel(
            cx,
            win,
            point(list.right() - px(2.), wip.center().y),
            -100_000.,
        );
        assert!(
            graph.0.borrow().base_handle.offset().y < graph_before.y - wip.size.height,
            "real graph wheel moves current WIP out of view"
        );
        assert!(
            e2e::control_bounds(win.window_id(), "compact-wip-row").is_none(),
            "current WIP scrolls out"
        );
        closed(cx, &app, &panel, win, &before);
        let tab_before = cx.read(|cx| app.read(cx).bottom_tab);
        focus_root(cx, &app, win);
        cx.simulate_keystrokes(win, "cmd-enter");
        settle(cx, win);
        closed(cx, &app, &panel, win, &before);
        assert_eq!(
            cx.read(|cx| app.read(cx).bottom_tab),
            tab_before,
            "Cmd+Enter without a visible WIP anchor preserves the current bottom tab"
        );
        // Returning the anchor must not automatically reopen its dismissed popup.
        let list = graph.0.borrow().base_handle.bounds();
        wheel(cx, win, list.center(), 100_000.);
        bounds(win, "compact-wip-row");
        closed(cx, &app, &panel, win, &before);
        open_wip(cx, win);
        assert_eq!(draft(cx, &panel), before, "scroll-out/reopen retains draft");
        drop(graph);
        drop(panel);
        unmount(cx, app, win);
    });
}
fn keyboard_reopen(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    panel: &Entity<CommitPanelView>,
    win: AnyWindowHandle,
    before: &(String, String, String),
    dimensions: (f32, f32),
) {
    focus_root(cx, app, win);
    cx.simulate_keystrokes(win, "cmd-enter");
    settle(cx, win);
    popup_geometry(win, dimensions);
    assert_eq!(
        &draft(cx, panel),
        before,
        "Cmd+Enter reopens retained draft"
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        panel.entity_id(),
        "Cmd+Enter reuses the panel owner"
    );
    let composer = bounds(win, "compact-composer");
    let graph = bounds(win, "compact-graph");
    let outside = point(graph.right() - px(2.), graph.top() + px(2.));
    assert!(
        outside.x > composer.right(),
        "outside click is genuinely outside popup"
    );
    cx.simulate_click(win, outside, Modifiers::none());
    settle(cx, win);
    assert_eq!(
        bounds(win, "compact-composer"),
        composer,
        "outside click does not dismiss a pinned composer"
    );
    assert_eq!(&draft(cx, panel), before, "outside click keeps draft");
    escape(cx, app, win);
    closed(cx, app, panel, win, before);
}
