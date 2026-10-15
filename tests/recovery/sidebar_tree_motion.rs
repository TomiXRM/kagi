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
    unmount(cx, app, window);
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
    unmount(cx, app, window);
}

fn fill(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) -> [u8; 4] {
    let bounds = e2e::control_bounds(window.window_id(), id).expect("native row bounds");
    cx.update_window(window, |_, window, _| {
        let image = window
            .render_to_image()
            .expect("native sidebar fill capture");
        let scale = window.scale_factor();
        image
            .get_pixel(
                ((f32::from(bounds.left()) + 6.) * scale) as u32,
                ((f32::from(bounds.top()) + 9.) * scale) as u32,
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
    support::focus(cx, &app, window, 0, "branch:main");
    frame(cx, &app, window, t + Duration::from_millis(300));
    let selected = fill(cx, window, "sidebar-local-main");
    assert!(
        distance(plain, selected) > 12,
        "focused identity has a real painted selection wash"
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
        sidebar::sidebar_motion_for_e2e(0, "branch:main", "wash").expect("painted outgoing wash");
    assert!(wash.opacity > 0. && wash.opacity < 1.);
    assert!(
        distance(outgoing, plain) > 2 && distance(outgoing, selected) > 2,
        "outgoing selection is painted mid-crossfade"
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
