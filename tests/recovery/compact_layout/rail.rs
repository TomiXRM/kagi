//! Navigator Popovers and logical-breakpoint geometry on the real root.
use super::*;

const RAIL_CONTROLS: [&str; 7] = [
    "compact-rail-local",
    "compact-rail-remote",
    "compact-rail-worktrees",
    "compact-rail-tags",
    "compact-rail-stashes",
    "compact-rail-prs",
    "compact-rail-issues",
];

fn rail_icon_paint(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    case: Case,
) {
    focus_root(cx, app, win);
    cx.simulate_mouse_move(
        win,
        point(px(WIDTH - 2.), px(case.height / 2.)),
        None,
        Modifiers::none(),
    );
    settle(cx, win);
    cx.update_window(win, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native compact rail SVG paint capture");
        let scale = window.scale_factor();
        let channels = |color: u32| [(color >> 16) as u8, (color >> 8) as u8, color as u8];
        let background = channels(theme::theme().sidebar);
        let foreground = channels(theme::theme().text_main);
        let matches = |pixel: [u8; 4], color: [u8; 3], tolerance: u8| {
            pixel[..3]
                .iter()
                .zip(color)
                .all(|(actual, expected)| actual.abs_diff(expected) <= tolerance)
        };
        for name in RAIL_CONTROLS {
            let trigger = bounds(win, name);
            let sample_x = ((f32::from(trigger.left() + theme::scaled_px(3.))) * scale) as u32;
            let sample_y = (f32::from(trigger.center().y) * scale) as u32;
            assert!(
                matches(image.get_pixel(sample_x, sample_y).0, background, 2),
                "{case:?} {name}: capture must show the unhovered, unselected rail"
            );
            // Only the centred SVG box: no control border, focus ring, tooltip
            // or text. A solid hover/selection fill cannot count as stroke paint.
            let half = theme::scaled_px(9.);
            let left = (f32::from(trigger.center().x - half) * scale).ceil() as u32;
            let right = (f32::from(trigger.center().x + half) * scale).floor() as u32;
            let top = (f32::from(trigger.center().y - half) * scale).ceil() as u32;
            let bottom = (f32::from(trigger.center().y + half) * scale).floor() as u32;
            let mut strokes = 0;
            let mut empty = 0;
            for y in top..bottom {
                for x in left..right {
                    let pixel = image.get_pixel(x, y).0;
                    strokes += usize::from(
                        matches(pixel, foreground, 8) && !matches(pixel, background, 8),
                    );
                    empty += usize::from(matches(pixel, background, 2));
                }
            }
            assert!(
                strokes as f32 >= 4. * scale * scale,
                "{case:?} {name}: actual SVG strokes must paint, found {strokes} foreground pixels"
            );
            assert!(
                empty >= ((right - left) * (bottom - top)) as usize / 4,
                "{case:?} {name}: outline glyph must retain transparent background"
            );
        }
    })
    .unwrap();
}

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
    for (section, trigger) in RAIL_CONTROLS.into_iter().enumerate() {
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
        rail_icon_paint(cx, &app, win, case);
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
