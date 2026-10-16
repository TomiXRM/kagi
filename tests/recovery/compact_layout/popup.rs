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

fn content_fit_geometry(win: AnyWindowHandle, dimensions: (f32, f32)) -> Pixels {
    popup_geometry(win, dimensions);
    let popup = bounds(win, "compact-composer");
    let commit = bounds(win, "cp-commit-action");
    let trailing = popup.bottom() - commit.bottom();
    assert!(
        trailing >= px(0.) && trailing <= px(theme::rem_size_px() * 0.5 + 3.),
        "popup must hug Commit plus footer padding, not retain a fixed empty floor: {trailing:?}",
    );
    assert!(
        popup.size.height <= theme::scaled_px(360.) + px(1.),
        "long drafts retain the existing bounded composer ceiling",
    );
    let anchor = bounds(win, "compact-wip-row");
    let gap = theme::scaled_px(4.);
    let margin = theme::scaled_px(8.);
    if anchor.bottom() + gap + popup.size.height <= px(dimensions.1) - margin {
        near(
            popup.top(),
            anchor.bottom() + gap,
            "content-fitting popup retains anchor gap below",
        );
    } else if anchor.top() - gap - popup.size.height >= margin {
        near(
            popup.bottom(),
            anchor.top() - gap,
            "content-fitting popup retains anchor gap above",
        );
    }
    popup.size.height
}

pub fn scenario_compact_layout_content_fit(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    for locale in ["en", "ja"] {
        for preset in ["apple-light", "apple-dark"] {
            for zoom in [1., 1.67] {
                let case = Case {
                    height: 800.,
                    zoom,
                    locale,
                    preset,
                };
                let (app, win) = mount(cx, fixture.path(), case);
                theme::set_compact_threshold_rem(80);
                let dimensions = (1080., 800.);
                resize(cx, win, dimensions);
                open_wip(cx, win);
                let panel = cx
                    .read(|cx| app.read(cx).ui().commit_panel.clone())
                    .unwrap();
                let (title, body) = cx.read(|cx| {
                    let panel = panel.read(cx);
                    (
                        panel.title_input.clone().unwrap(),
                        panel.body_input.clone().unwrap(),
                    )
                });
                let ids = (panel.entity_id(), title.entity_id(), body.entity_id());
                let owner = cx.read(|cx| app.read(cx).active_session());
                let staged_index = index(fixture.path());
                let long = "long retained template 本文\n".repeat(80);
                let mut heights = Vec::new();
                for text in ["", long.as_str(), ""] {
                    cx.update_window(win, |_, window, cx| {
                        title.update(cx, |input, cx| {
                            input.set_value("retained subject", window, cx)
                        });
                        body.update(cx, |input, cx| input.set_value(text.to_owned(), window, cx));
                        body.focus_handle(cx).focus(window, cx);
                    })
                    .unwrap();
                    settle(cx, win);
                    heights.push(content_fit_geometry(win, dimensions));
                    assert_eq!(draft(cx, &panel).0, "retained subject");
                    assert_eq!(draft(cx, &panel).1, text);
                    assert_eq!(cx.read(|cx| app.read(cx).active_session()), owner);
                    assert_eq!(
                        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
                        ids.0
                    );
                    assert_eq!(
                        cx.read(|cx| panel.read(cx).title_input.as_ref().unwrap().entity_id()),
                        ids.1
                    );
                    assert_eq!(
                        cx.read(|cx| panel.read(cx).body_input.as_ref().unwrap().entity_id()),
                        ids.2
                    );
                    assert!(
                        cx.update_window(win, |_, window, cx| {
                            body.focus_handle(cx).is_focused(window)
                        })
                        .unwrap(),
                        "content fitting must not move input focus"
                    );
                    assert_eq!(index(fixture.path()), staged_index);
                }
                assert!(
                    heights[1] > heights[0] + theme::scaled_px(40.),
                    "long draft must grow the popup"
                );
                near(
                    heights[2],
                    heights[0],
                    "short→long→short releases excess height",
                );
                let before = draft(cx, &panel);
                click(cx, win, "compact-popup-close");
                open_wip(cx, win);
                near(
                    content_fit_geometry(win, dimensions),
                    heights[0],
                    "reopen fits the retained short draft",
                );
                assert_eq!(draft(cx, &panel), before);
                cx.update_window(win, |_, window, cx| {
                    body.update(cx, |input, cx| input.set_value(long.clone(), window, cx));
                })
                .unwrap();
                settle(cx, win);
                let long_before = draft(cx, &panel);
                resize(cx, win, (600., 400.));
                content_fit_geometry(win, (600., 400.));
                assert_eq!(
                    draft(cx, &panel),
                    long_before,
                    "viewport cap preserves the long draft"
                );
                theme::set_zoom(if zoom == 1. { 1.67 } else { 1. });
                cx.update(theme::sync_gpui_component_theme);
                settle(cx, win);
                content_fit_geometry(win, (600., 400.));
                assert_eq!(
                    draft(cx, &panel),
                    long_before,
                    "live zoom remeasures without replacing inputs"
                );
                assert_eq!(
                    cx.read(|cx| panel.read(cx).body_input.as_ref().unwrap().entity_id()),
                    ids.2
                );
                drop((title, body, panel));
                unmount(cx, app, win);
            }
        }
    }
}
