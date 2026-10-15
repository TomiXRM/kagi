//! #1617: native presentation, not transition-state-only assertions.
#[allow(dead_code)] // Shared fixture support also serves sibling identity scenarios.
#[path = "sidebar_identity/support.rs"]
mod support;

use crate::macos::{build_fixture, git, unmount};
use gpui::{AnyWindowHandle, Entity, Modifiers, VisualTestAppContext};
use kagi::ui::{e2e, sidebar, KagiApp};
use kagi_ui_core::{motion, theme};
use std::time::{Duration, Instant};
use support::{appearance, mount_dimensions, paint, Settings};

#[path = "sidebar_tree_motion/pointer.rs"]
mod pointer;
pub use pointer::scenario_sidebar_tree_pointer_motion;

struct MotionGuard(bool);
impl Drop for MotionGuard {
    fn drop(&mut self) {
        motion::set_clock_for_e2e(None);
        theme::set_reduce_motion(self.0);
    }
}

fn frame(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    at: Instant,
) {
    motion::set_clock_for_e2e(Some(at));
    sidebar::clear_sidebar_motion_for_e2e();
    paint(cx, app, window);
}

fn click(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle, id: &str) {
    let bounds = e2e::control_bounds(window.window_id(), id).expect("drawn sidebar control");
    cx.simulate_click(window, bounds.center(), Modifiers::none());
    paint(cx, app, window);
}

fn pane_height(cx: &VisualTestAppContext, app: &Entity<KagiApp>, pane: usize) -> f32 {
    app.read_with(cx, |app, _| {
        let (top, bottom) = app.sidebar.pane_geom[pane].get();
        bottom - top
    })
}

pub fn scenario_sidebar_tree_pane_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "topic/one"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/topic/one", "HEAD"],
    );
    git(fixture.path(), &["tag", "motion-pane-tag"]);
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("pane-linked");
    git(
        fixture.path(),
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(fixture.path().join("pane-stash.txt"), "pane content\n").unwrap();
    git(fixture.path(), &["stash", "push", "-uqm", "pane stash"]);
    let (app, window) = mount_dimensions(cx, fixture.path(), (1440., 900.));
    for (pane, section) in ["local", "remote", "worktrees", "tags", "stashes"]
        .into_iter()
        .enumerate()
    {
        theme::set_reduce_motion(false);
        appearance(
            cx,
            &app,
            window,
            kagi_ui_core::i18n::Lang::En,
            "apple-light",
            1.,
            pane,
        );
        let t = Instant::now();
        frame(cx, &app, window, t);
        let open = pane_height(cx, &app, pane);
        let first = app.read_with(cx, |app, _| app.sidebar_row_keys_for_e2e(pane)[0].clone());
        click(cx, &app, window, section);
        assert!(app.read_with(cx, |app, _| app.sidebar.collapsed.contains(section)));
        frame(cx, &app, window, t + Duration::from_millis(75));
        let middle = pane_height(cx, &app, pane);
        assert!(
            middle > 30. && middle < open - 1.,
            "closing body must be drawn at a real intermediate height: {middle}, open {open}"
        );
        assert!(
            sidebar::sidebar_motion_for_e2e(pane, &first, "closing").is_some(),
            "all five closing panes retain real painted content"
        );
        click(cx, &app, window, section);
        let reversed = pane_height(cx, &app, pane);
        assert!(
            (reversed - middle).abs() < 1.,
            "reversal preserves painted height"
        );
        frame(cx, &app, window, t + Duration::from_millis(400));
        assert!((pane_height(cx, &app, pane) - open).abs() < 1.);
        click(cx, &app, window, section);
        frame(cx, &app, window, t + Duration::from_millis(700));
        assert!(pane_height(cx, &app, pane) < 30.);
        click(cx, &app, window, section);
        frame(cx, &app, window, t + Duration::from_millis(750));
        theme::set_reduce_motion(true);
        frame(cx, &app, window, t + Duration::from_millis(751));
        assert!(
            (pane_height(cx, &app, pane) - open).abs() < 1.,
            "Reduce Motion settles the drawn body immediately"
        );
    }
    pane_reopen_from_all_closed(cx, &app, window);
    unmount(cx, app, window);
}

fn pane_reopen_from_all_closed(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    let mut t = Instant::now();
    theme::set_reduce_motion(true);
    app.update(cx, |app, cx| {
        app.sidebar
            .collapsed
            .extend(["local", "remote", "worktrees", "tags", "stashes"]);
        cx.notify();
    });
    frame(cx, app, window, t);
    for height in [900., 720., 1040.] {
        let wanted = gpui::size(gpui::px(1440.), gpui::px(height));
        cx.update_window(window, |_, window, _| window.resize(wanted))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            crate::macos::drain_native_events();
            cx.run_until_parked();
            if cx
                .update_window(window, |_, window, _| window.viewport_size())
                .unwrap()
                == wanted
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native all-closed pane window resized"
            );
        }
        frame(cx, app, window, t);
        let closed = pane_height(cx, app, 0);
        let container = e2e::control_bounds(window.window_id(), "sidebar-panes").unwrap();
        let occupied = app.read_with(cx, |app, _| {
            app.sidebar.pane_geom[4].get().1 - app.sidebar.pane_geom[0].get().0
        });
        let expected_body = f32::from(container.size.height) - occupied;
        assert!(
            expected_body > closed,
            "all-closed stack leaves real flex space"
        );
        theme::set_reduce_motion(false);
        click(cx, app, window, "local");
        frame(cx, app, window, t + Duration::from_millis(75));
        let middle_body = pane_height(cx, app, 0) - closed;
        frame(cx, app, window, t + Duration::from_millis(179));
        let near_endpoint = pane_height(cx, app, 0);
        frame(cx, app, window, t + Duration::from_millis(400));
        let endpoint = pane_height(cx, app, 0);
        let body = endpoint - closed;
        assert!(
            middle_body > 1. && middle_body < body - 1.,
            "all-closed reopen at viewport {height}: midpoint body must be nonzero and below endpoint: middle={middle_body}, endpoint={body}"
        );
        assert!(
            (body - expected_body).abs() < 2.,
            "pane consumes measured unused container space"
        );
        assert!(
            (near_endpoint - endpoint).abs() < 2.,
            "opening must not jump when flex growth resumes"
        );
        click(cx, app, window, "local");
        frame(cx, app, window, t + Duration::from_millis(700));
        assert!((pane_height(cx, app, 0) - closed).abs() < 1.);
        theme::set_reduce_motion(true);
        click(cx, app, window, "local");
        frame(cx, app, window, t + Duration::from_millis(701));
        assert!(
            (pane_height(cx, app, 0) - endpoint).abs() < 1.,
            "Reduce Motion opens all-closed pane immediately"
        );
        click(cx, app, window, "local");
        frame(cx, app, window, t + Duration::from_millis(702));
        assert!(
            (pane_height(cx, app, 0) - closed).abs() < 1.,
            "Reduce Motion closes pane immediately"
        );
        t += Duration::from_secs(1);
    }
}

pub fn scenario_sidebar_tree_subgroup_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    for name in ["topic/one", "topic/two", "zzz"] {
        git(fixture.path(), &["branch", name]);
        git(
            fixture.path(),
            &["update-ref", &format!("refs/remotes/origin/{name}"), "HEAD"],
        );
    }
    let (app, window) = mount_dimensions(cx, fixture.path(), (1440., 900.));
    for (pane, group, below) in [
        (0, "sidebar-group-local:topic", "sidebar-local-zzz"),
        (
            1,
            "sidebar-group-remote:origin:topic",
            "sidebar-remote-origin/zzz",
        ),
        (
            1,
            "sidebar-group-remote:origin",
            "sidebar-group-remote:origin",
        ),
    ] {
        appearance(
            cx,
            &app,
            window,
            kagi_ui_core::i18n::Lang::En,
            "apple-light",
            1.,
            pane,
        );
        let t = Instant::now();
        frame(cx, &app, window, t);
        let before = e2e::control_bounds(window.window_id(), below)
            .expect("below row drawn")
            .top();
        click(cx, &app, window, group);
        frame(cx, &app, window, t + Duration::from_millis(75));
        let middle = e2e::control_bounds(window.window_id(), below)
            .expect("below row drawn")
            .top();
        let ghost_key = if pane == 0 {
            "branch:topic/one"
        } else {
            "remote:origin/topic/one"
        };
        let ghost = sidebar::sidebar_motion_for_e2e(pane, ghost_key, "ghost")
            .expect("removed real content is still painted");
        assert!(ghost.opacity > 0. && ghost.opacity < 1. && ghost.height > 15.);
        assert!(
            !app.read_with(cx, |app, _| app.sidebar_row_keys_for_e2e(pane))
                .iter()
                .any(|key| key == ghost_key),
            "presentation ghosts are not logical focus/AX rows"
        );
        if group != below {
            assert!(
                middle < before && middle > before - gpui::px(36.),
                "two removed slots must still displace following content at midpoint"
            );
        }
        click(cx, &app, window, group);
        let reversed = e2e::control_bounds(window.window_id(), below)
            .unwrap()
            .top();
        assert!(
            (f32::from(reversed - middle)).abs() < 1.,
            "accordion reversal is continuous"
        );
        frame(cx, &app, window, t + Duration::from_millis(400));
        assert!(
            (f32::from(
                e2e::control_bounds(window.window_id(), below)
                    .unwrap()
                    .top()
                    - before
            ))
            .abs()
                < 1.
        );
        click(cx, &app, window, group);
        frame(cx, &app, window, t + Duration::from_millis(650));
        assert!(
            sidebar::sidebar_motion_for_e2e(pane, ghost_key, "ghost").is_none(),
            "elapsed settlement retires painted ghosts"
        );
        click(cx, &app, window, group);
        frame(cx, &app, window, t + Duration::from_millis(900));
        click(cx, &app, window, group);
        frame(cx, &app, window, t + Duration::from_millis(950));
        theme::set_reduce_motion(true);
        frame(cx, &app, window, t + Duration::from_millis(951));
        assert!(
            sidebar::sidebar_motion_for_e2e(pane, ghost_key, "ghost").is_none(),
            "Reduce Motion retires painted ghosts"
        );
        theme::set_reduce_motion(false);
        click(cx, &app, window, group);
        frame(cx, &app, window, t + Duration::from_millis(1200));
    }
    for n in 0..60 {
        git(fixture.path(), &["branch", &format!("aaa-{n:02}")]);
        git(fixture.path(), &["branch", &format!("zzz-{n:02}")]);
    }
    theme::set_reduce_motion(true);
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    paint(cx, &app, window);
    theme::set_reduce_motion(false);
    // Pixel-snapped uniform-list pitch differs from the theme height at
    // fractional zoom. Keep the removed rows anchored well above the drawn
    // range, so ghost mounting accumulates any per-slot placement error.
    appearance(
        cx,
        &app,
        window,
        kagi_ui_core::i18n::Lang::En,
        "apple-light",
        1.1,
        0,
    );
    app.update(cx, |app, cx| {
        app.sidebar.scroll_handles[0].scroll_to_item_strict(58, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    let t = Instant::now();
    frame(cx, &app, window, t);
    let group = sidebar::sidebar_motion_for_e2e(0, "group:local:topic", "row")
        .expect("scrolled fractional group slot");
    let child = sidebar::sidebar_motion_for_e2e(0, "branch:topic/one", "row").unwrap();
    let neighbor = sidebar::sidebar_motion_for_e2e(0, "branch:zzz-00", "row").unwrap();
    let pitch = (neighbor.y - child.y) / 3.;
    assert!(
        (child.y - group.y - pitch).abs() < 0.6,
        "adjacent native slots share pitch: group={group:?}, child={child:?}, neighbor={neighbor:?}, pitch={pitch}"
    );
    click(cx, &app, window, "sidebar-group-local:topic");
    frame(cx, &app, window, t);
    app.update(cx, |app, cx| {
        app.sidebar.scroll_handles[0].scroll_to_item_strict(85, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    frame(cx, &app, window, t);
    let ghost = sidebar::sidebar_motion_for_e2e(0, "branch:topic/one", "ghost")
        .expect("retained ghost anchored above drawn range");
    let neighbor = sidebar::sidebar_motion_for_e2e(0, "branch:zzz-30", "row")
        .expect("real fractional-zoom neighbor");
    assert!(
        (neighbor.y - ghost.y - pitch * 33.).abs() < 0.6,
        "scrolled ghost and real neighbor share measured native pitch: ghost={ghost:?}, neighbor={neighbor:?}, pitch={pitch}"
    );
    frame(cx, &app, window, t + Duration::from_millis(75));
    let ghost = sidebar::sidebar_motion_for_e2e(0, "branch:topic/one", "ghost").unwrap();
    assert!(ghost.opacity > 0. && ghost.opacity < 1.);
    let neighbor = sidebar::sidebar_motion_for_e2e(0, "branch:zzz-30", "row").unwrap();
    assert!(
        (neighbor.y - ghost.y - pitch * (31. + 2. * ghost.opacity)).abs() < 0.6,
        "mid-splice alignment: ghost={ghost:?}, neighbor={neighbor:?}, pitch={pitch}, expected={}",
        pitch * (31. + 2. * ghost.opacity)
    );
    frame(cx, &app, window, t + Duration::from_millis(300));
    assert!(sidebar::sidebar_motion_for_e2e(0, "branch:topic/one", "ghost").is_none());
    theme::set_zoom(1.);
    unmount(cx, app, window);
}

fn fill(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> [u8; 4] {
    let bounds = e2e::control_bounds(window.window_id(), id).expect("native row bounds");
    let name = id.strip_prefix("sidebar-local-").expect("local branch row");
    let ring = sidebar::sidebar_motion_for_e2e(0, &format!("branch:{name}"), "ring")
        .expect("painted native ring geometry");
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native sidebar ring capture");
        let scale = window.scale_factor();
        image
            .get_pixel(
                (f32::from(bounds.center().x) * scale) as u32,
                ((ring.y + 1.) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

fn distance(a: [u8; 4], b: [u8; 4]) -> i32 {
    (0..3)
        .map(|i| (i32::from(a[i]) - i32::from(b[i])).abs())
        .sum()
}

pub fn scenario_sidebar_tree_selection_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let (app, window) = mount_dimensions(cx, fixture.path(), (1440., 900.));
    appearance(
        cx,
        &app,
        window,
        kagi_ui_core::i18n::Lang::En,
        "apple-light",
        1.,
        0,
    );
    let t = Instant::now();
    frame(cx, &app, window, t);
    let plain = fill(cx, window, "sidebar-local-main");
    cx.simulate_keystrokes(window, "f19");
    support::focus(cx, &app, window, 0, "branch:main");
    frame(cx, &app, window, t + Duration::from_millis(300));
    let selected = fill(cx, window, "sidebar-local-main");
    assert!(
        distance(plain, selected) > 12,
        "focused identity paints the existing keyboard ring"
    );
    support::focus(cx, &app, window, 0, "branch:feature");
    assert_eq!(
        support::focused(cx, &app, window),
        Some((0, "branch:feature".into())),
        "logical focus changes immediately"
    );
    frame(cx, &app, window, t + Duration::from_millis(375));
    let outgoing = fill(cx, window, "sidebar-local-main");
    let wash =
        sidebar::sidebar_motion_for_e2e(0, "branch:main", "ring").expect("painted outgoing ring");
    assert!(wash.opacity > 0. && wash.opacity < 1.);
    assert!(
        distance(outgoing, plain) > 2 && distance(outgoing, selected) > 2,
        "outgoing keyboard ring is painted mid-crossfade"
    );
    support::focus(cx, &app, window, 0, "branch:main");
    assert!(
        distance(fill(cx, window, "sidebar-local-main"), outgoing) <= 3,
        "reversal preserves painted opacity"
    );
    frame(cx, &app, window, t + Duration::from_millis(650));
    assert!(distance(fill(cx, window, "sidebar-local-main"), selected) <= 3);
    support::focus(cx, &app, window, 0, "branch:feature");
    frame(cx, &app, window, t + Duration::from_millis(700));
    theme::set_reduce_motion(true);
    frame(cx, &app, window, t + Duration::from_millis(701));
    assert!(distance(fill(cx, window, "sidebar-local-main"), plain) <= 3);
    theme::set_reduce_motion(false);
    frame(cx, &app, window, t + Duration::from_millis(702));
    assert!(
        distance(fill(cx, window, "sidebar-local-main"), plain) <= 3,
        "settled motion does not revive"
    );
    unmount(cx, app, window);
}

/// PR #1621: a hidden Navigator cannot retain a keyboard ring until its next
/// virtual-list paint. Check the first returning native frame, not just focus.
pub fn scenario_sidebar_tree_focus_return_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(fixture.path(), &["branch", "feature"]);
    let (app, window) = mount_dimensions(cx, fixture.path(), (1440., 900.));
    appearance(
        cx,
        &app,
        window,
        kagi_ui_core::i18n::Lang::En,
        "apple-light",
        1.,
        0,
    );
    let mut t = Instant::now();
    frame(cx, &app, window, t);
    let plain = fill(cx, window, "sidebar-local-main");
    for departure in [
        "view.toggleSidebar",
        "view.togglePrMode",
        "issues",
        "view.toggleEditorWorkspace",
    ] {
        cx.simulate_keystrokes(window, "f19");
        support::focus(cx, &app, window, 0, "branch:main");
        frame(cx, &app, window, t + Duration::from_millis(300));
        assert!(
            distance(fill(cx, window, "sidebar-local-main"), plain) > 12,
            "{departure}: departing row must paint a settled keyboard ring"
        );
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                if departure == "issues" {
                    app.show_issues_mode(cx);
                } else {
                    app.handle_menu_command(departure, window, cx);
                }
            });
        })
        .unwrap();
        frame(cx, &app, window, t + Duration::from_millis(301));
        frame(cx, &app, window, t + Duration::from_millis(700));
        assert_eq!(
            support::focused(cx, &app, window),
            None,
            "{departure}: hidden Navigator must release logical row focus"
        );
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                if departure == "view.toggleSidebar" {
                    app.handle_menu_command(departure, window, cx);
                } else {
                    app.show_graph_mode(cx);
                }
            });
        })
        .unwrap();
        frame(cx, &app, window, t + Duration::from_millis(701));
        // The View toggle initially has a zero-width opening clip. Sample its
        // first positive-width frame; mode takeovers return at full width.
        frame(cx, &app, window, t + Duration::from_millis(702));
        assert_eq!(support::focused(cx, &app, window), None);
        let ring = sidebar::sidebar_motion_for_e2e(0, "branch:main", "ring")
            .expect("returning row paints its presentation");
        assert!(
            ring.opacity <= f32::EPSILON,
            "{departure}: first returning unfocused row must not replay its old ring: {}",
            ring.opacity
        );
        if departure != "view.toggleSidebar" {
            assert!(
                distance(fill(cx, window, "sidebar-local-main"), plain) <= 3,
                "{departure}: returning native ring pixels must match the unfocused baseline"
            );
        }
        frame(cx, &app, window, t + Duration::from_millis(775));
        let ring = sidebar::sidebar_motion_for_e2e(0, "branch:main", "ring").unwrap();
        assert!(
            ring.opacity <= f32::EPSILON,
            "{departure}: unfocused row must not fade an obsolete ring: {}",
            ring.opacity
        );
        t += Duration::from_millis(1100);
        frame(cx, &app, window, t);
        // The View toggle translates the whole sidebar while it opens: its
        // left-border pixel is comparable only once that clip has settled.
        assert!(
            distance(fill(cx, window, "sidebar-local-main"), plain) <= 3,
            "{departure}: settled native pixels must match the unfocused baseline"
        );
    }
    unmount(cx, app, window);
}

pub fn scenario_sidebar_tree_compact_focus_return_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    let _offline = crate::pr_fields_focus::OfflineGh::with_script("#!/bin/sh\nexit 1\n");
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/feature", "HEAD"],
    );
    let (app, window) = mount_dimensions(cx, fixture.path(), (940., 660.));
    appearance(
        cx,
        &app,
        window,
        kagi_ui_core::i18n::Lang::En,
        "apple-light",
        1.,
        0,
    );
    let mut t = Instant::now();
    frame(cx, &app, window, t);
    click(cx, &app, window, "compact-rail-local");
    let plain = fill(cx, window, "sidebar-local-main");
    for destination in [
        "compact-rail-remote",
        "compact-rail-prs",
        "compact-rail-issues",
    ] {
        cx.simulate_keystrokes(window, "f19");
        support::focus(cx, &app, window, 0, "branch:main");
        frame(cx, &app, window, t + Duration::from_millis(300));
        let selected = fill(cx, window, "sidebar-local-main");
        let departing = sidebar::sidebar_motion_for_e2e(0, "branch:main", "ring").unwrap();
        assert!(
            distance(selected, plain) > 12,
            "{destination}: compact ring precondition: plain={plain:?}, selected={selected:?}, ring={departing:?}"
        );
        click(cx, &app, window, destination);
        frame(cx, &app, window, t + Duration::from_millis(301));
        assert!(support::focused(cx, &app, window).is_none_or(|(pane, _)| pane != 0));
        click(cx, &app, window, "compact-rail-local");
        frame(cx, &app, window, t + Duration::from_millis(302));
        let ring = sidebar::sidebar_motion_for_e2e(0, "branch:main", "ring")
            .expect("returning compact row paints its presentation");
        assert!(
            ring.opacity <= f32::EPSILON,
            "{destination}: compact return must not replay its skipped pane's ring: {}",
            ring.opacity
        );
        assert!(
            distance(fill(cx, window, "sidebar-local-main"), plain) <= 3,
            "{destination}: compact return paints the unfocused native baseline"
        );
        t += Duration::from_millis(600);
        frame(cx, &app, window, t);
    }
    unmount(cx, app, window);
}

pub fn scenario_sidebar_tree_refresh_motion(cx: &mut VisualTestAppContext) {
    let _settings = Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    theme::set_reduce_motion(false);
    let fixture = build_fixture();
    let repo = fixture.path();
    std::fs::write(repo.join("motion-stash.txt"), "base\n").unwrap();
    git(repo, &["add", "motion-stash.txt"]);
    git(repo, &["commit", "-qm", "motion base"]);
    let linked = tempfile::tempdir().unwrap();
    let path = linked.path().join("linked");
    let (app, window) = mount_dimensions(cx, repo, (1440., 900.));
    app.update(cx, |app, cx| {
        app.sidebar.collapsed.clear();
        app.sidebar.pane_weights = [1; 5];
        cx.notify();
    });
    let t = Instant::now();
    frame(cx, &app, window, t);
    // A new owner first renders before its async read publishes. That empty
    // frame must not consume the initial-load instant contract.
    let initial = build_fixture();
    git(initial.path(), &["branch", "initial-small"]);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(initial.path().to_path_buf(), cx));
    });
    frame(cx, &app, window, t);
    cx.run_until_parked();
    frame(cx, &app, window, t);
    for key in ["branch:main", "branch:initial-small"] {
        let row = sidebar::sidebar_motion_for_e2e(0, key, "row")
            .expect("initial async small-list row is painted");
        assert_eq!(row.opacity, 1., "first publication is immediate: {key}");
    }
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo.to_path_buf(), cx));
    });
    frame(cx, &app, window, t);
    cx.run_until_parked();
    frame(cx, &app, window, t);
    assert_eq!(
        sidebar::sidebar_motion_for_e2e(0, "branch:main", "row")
            .expect("returning owner is painted")
            .opacity,
        1.,
        "switching back settles immediately"
    );
    git(repo, &["branch", "incoming"]);
    git(
        repo,
        &["update-ref", "refs/remotes/origin/incoming", "HEAD"],
    );
    git(repo, &["tag", "motion-tag"]);
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    let path = path.canonicalize().unwrap();
    std::fs::write(repo.join("motion-stash.txt"), "changed\n").unwrap();
    git(repo, &["stash", "push", "-qm", "motion stash"]);
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    frame(cx, &app, window, t);
    let stash = app.read_with(cx, |app, _| app.sidebar_row_keys_for_e2e(4)[0].clone());
    let rows = [
        (0, "branch:incoming".to_owned()),
        (1, "remote:origin/incoming".to_owned()),
        (2, format!("worktree:{}", path.display())),
        (3, "tag:motion-tag".to_owned()),
        (4, stash),
    ];
    frame(cx, &app, window, t + Duration::from_millis(90));
    for (pane, key) in &rows {
        let drawn = sidebar::sidebar_motion_for_e2e(*pane, key, "row")
            .expect("refreshed real row is painted");
        assert!(
            drawn.opacity > 0. && drawn.opacity < 1. && drawn.height > 15.,
            "{key}: {drawn:?}"
        );
    }
    frame(cx, &app, window, t + Duration::from_millis(300));
    for (pane, key) in &rows {
        assert_eq!(
            sidebar::sidebar_motion_for_e2e(*pane, key, "row")
                .unwrap()
                .opacity,
            1.
        );
    }
    unmount(cx, app, window);
}
