//! Navigator Popovers and logical-breakpoint geometry on the real root.
use super::*;
fn rail_filter(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    section: usize,
) {
    if section < 5 {
        let popup = bounds(win, "compact-sidebar-popover");
        let filter_point = point(popup.center().x, popup.top() + theme::scaled_px(14.));
        // Click the real lazy placeholder and then the actual mounted Input.
        cx.simulate_click(win, filter_point, Modifiers::none());
        settle(cx, win);
        cx.simulate_click(win, filter_point, Modifiers::none());
        settle(cx, win);
        let filter = cx
            .read(|cx| app.read(cx).sidebar.filter.clone())
            .expect("real sidebar filter");
        assert!(
            cx.update_window(win, |_, window, cx| {
                filter.read(cx).focus_handle(cx).is_focused(window)
            })
            .unwrap(),
            "sidebar filter acquires actual focus"
        );
        cx.simulate_input(win, "compact-no-match");
        settle(cx, win);
        assert_eq!(
            cx.read(|cx| filter.read(cx).value().to_string()),
            "compact-no-match"
        );
        cx.simulate_keystrokes(win, "cmd-a backspace");
        settle(cx, win);
        assert!(
            cx.read(|cx| filter.read(cx).value().is_empty()),
            "filter clears through real keys"
        );
        drop(filter);
    } else {
        click(cx, win, "list-filter-text");
        cx.simulate_input(win, "compact-no-match");
        settle(cx, win);
        let query = cx.read(|cx| {
            let ui = app.read(cx).ui();
            if section == 5 {
                ui.github_pr_filter.common.text.clone()
            } else {
                ui.github_issue_filter.common.text.clone()
            }
        });
        assert_eq!(
            query, "compact-no-match",
            "real focused PR/Issue filter updates its owner"
        );
        cx.simulate_keystrokes(win, "cmd-a backspace");
        settle(cx, win);
    }
}
pub(super) fn rail_interactions(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    repo: &Path,
    dimensions: (f32, f32),
) {
    let before = crate::macos::repo_fingerprint(repo);
    let staged_before = index(repo);
    for (section, trigger) in [
        "compact-rail-local",
        "compact-rail-remote",
        "compact-rail-worktrees",
        "compact-rail-tags",
        "compact-rail-stashes",
        "compact-rail-prs",
        "compact-rail-issues",
    ]
    .into_iter()
    .enumerate()
    {
        rail_section(cx, app, win, section, trigger, dimensions);
    }
    // The checked-out local leaf dispatches its real callback without a write.
    click(cx, win, "compact-rail-local");
    click(cx, win, "sidebar-local-main");
    assert!(
        e2e::control_bounds(win.window_id(), "compact-sidebar-popover").is_none(),
        "choosing a real local leaf closes its navigator"
    );
    assert!(
        cx.read(|cx| app.read(cx).active_modal.is_none()),
        "leaf choose leaves no ghost modal"
    );
    assert_eq!(
        crate::macos::repo_fingerprint(repo),
        before,
        "navigator/filter/leaf does not write Git"
    );
    assert_eq!(
        index(repo),
        staged_before,
        "navigator leaves real staged content unchanged"
    );
    rail_graph(win, dimensions);
}
pub fn scenario_compact_layout_rail(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = fixture();
    let _offline = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\necho 'compact fixture: offline GitHub' >&2\nexit 1\n",
    );
    matrix(|case| {
        let (app, win) = mount(cx, fixture.path(), case);
        rail_graph(win, case.dimensions());
        rail_interactions(cx, &app, win, fixture.path(), case.dimensions());
        rail_thresholds(cx, win, case.height);
        unmount(cx, app, win);
    });
    let case = Case {
        height: 660.,
        zoom: 1.,
        locale: "en",
        preset: "catppuccin",
    };
    let (app, win) = mount(cx, fixture.path(), case);
    rail_graph(win, case.dimensions());
    rail_thresholds(cx, win, case.height);
    unmount(cx, app, win);
}

fn rail_section(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    section: usize,
    trigger: &str,
    dimensions: (f32, f32),
) {
    contained(bounds(win, "compact-rail"), bounds(win, trigger), trigger);
    focus_root(cx, app, win);
    click(cx, win, trigger);
    contained(
        viewport(dimensions),
        bounds(win, "compact-sidebar-popover"),
        "real navigator popover is window-contained",
    );
    assert!(
        cx.read(|cx| app.read(cx).active_modal.is_none()),
        "rail does not open a ghost modal"
    );
    rail_filter(cx, app, win, section);
    // Escape must route from the actual Input, without a test-side refocus.
    cx.simulate_keystrokes(win, "escape");
    settle(cx, win);
    assert!(
        e2e::control_bounds(win.window_id(), "compact-sidebar-popover").is_none(),
        "{trigger}: Escape closes navigator from filter focus"
    );
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx).root_focus.as_ref().unwrap().is_focused(window)
        })
        .unwrap(),
        "{trigger}: Escape restores pre-popover root focus"
    );
    click(cx, win, trigger);
    bounds(win, "compact-sidebar-popover");
    cx.simulate_click(
        win,
        point(px(dimensions.0 - 2.), px(dimensions.1 / 2.)),
        Modifiers::none(),
    );
    settle(cx, win);
    assert!(
        e2e::control_bounds(win.window_id(), "compact-sidebar-popover").is_none(),
        "{trigger}: real outside click dismisses navigator"
    );
}

fn rail_thresholds(cx: &mut VisualTestAppContext, win: AnyWindowHandle, height: f32) {
    resize(cx, win, (999., height));
    rail_graph(win, (999., height));
    resize(cx, win, (1000., height));
    assert!(
        e2e::control_bounds(win.window_id(), "compact-rail").is_none(),
        "exactly 1000 logical px restores the unchanged desktop navigator"
    );
    resize(cx, win, (WIDTH, height));
    rail_graph(win, (WIDTH, height));
}
