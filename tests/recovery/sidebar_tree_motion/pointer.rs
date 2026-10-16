//! Frozen native pointer ownership, including the compact slot's real semantics.
use super::{click, frame, support, MotionGuard};
use crate::macos::{build_fixture, git, unmount};
use gpui::{point, px, AnyWindowHandle, Entity, Modifiers, MouseButton, VisualTestAppContext};
use kagi::ui::{
    e2e,
    sidebar::{self, SidebarRow},
    KagiApp,
};
use kagi_ui_core::theme;
use std::time::{Duration, Instant};

#[path = "pointer_ghost.rs"]
mod pointer_ghost;
#[path = "pointer_other.rs"]
mod pointer_other;

const POPUP: &str = "compact-sidebar-popover";
fn drawn(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    at: Instant,
) {
    e2e::clear_control_bounds(window.window_id(), POPUP);
    cx.run_until_parked();
    frame(cx, app, window, at);
}
fn collapsed(cx: &VisualTestAppContext, app: &Entity<KagiApp>, wanted: &str) -> bool {
    app.read_with(cx, |app, _| {
        app.sidebar
            .rows
            .iter()
            .find_map(|row| match row {
                SidebarRow::LocalGroupHeader { key, collapsed, .. }
                | SidebarRow::RemoteHeader { key, collapsed, .. }
                | SidebarRow::RemoteSubGroup { key, collapsed, .. }
                    if key == wanted =>
                {
                    Some(*collapsed)
                }
                _ => None,
            })
            .expect("drawn group")
    })
}
fn pointer(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    hit: gpui::Point<gpui::Pixels>,
    button: MouseButton,
) {
    cx.simulate_mouse_move(window, hit, None, Modifiers::none());
    cx.simulate_mouse_down(window, hit, button, Modifiers::none());
    cx.simulate_mouse_up(window, hit, button, Modifiers::none());
}

pub fn scenario_sidebar_tree_pointer_motion(cx: &mut VisualTestAppContext) {
    let _settings = support::Settings::capture();
    let _reduce_setting = crate::gui_isolation::SavedKeys::keep(&["reduce_motion"]);
    let _motion = MotionGuard(theme::reduce_motion());
    let fixture = build_fixture();
    let repo = fixture.path();
    for name in ["aaa/a", "aaa/c", "aaa/e"] {
        git(repo, &["branch", name]);
    }
    std::fs::write(repo.join("README.md"), "second\n").unwrap();
    git(repo, &["commit", "-qam", "second pointer target"]);
    for name in ["aaa/b", "aaa/d", "aaa/f", "bba/one", "bba/two"] {
        git(repo, &["branch", name]);
    }
    std::fs::write(repo.join("README.md"), "third\n").unwrap();
    git(repo, &["commit", "-qam", "head pointer target"]);
    git(repo, &["branch", "bbb"]);
    git(repo, &["checkout", "-q", "aaa/f"]);
    for name in [
        "aaa/a", "aaa/b", "aaa/c", "aaa/d", "aaa/e", "aaa/f", "bba/one", "bba/two", "bbb", "main",
    ] {
        git(
            repo,
            &["update-ref", &format!("refs/remotes/origin/{name}"), name],
        );
    }
    let linked = tempfile::tempdir().unwrap();
    let linked_path = linked.path().join("pointer-linked");
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            linked_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    let witness = support::Witness::read(repo);
    let mut failures = Vec::new();
    for (compact, pane) in [(false, 0), (true, 0), (false, 1), (true, 1)] {
        let (app, window) =
            support::mount_dimensions(cx, repo, if compact { (940., 660.) } else { (1440., 900.) });
        support::appearance(
            cx,
            &app,
            window,
            kagi_ui_core::i18n::Lang::En,
            "apple-light",
            1.,
            pane,
        );
        let rail = if pane == 0 {
            "compact-rail-local"
        } else {
            "compact-rail-remote"
        };
        let group_prefix = if pane == 0 { "local" } else { "remote:origin" };
        let aaa = format!("{group_prefix}:aaa");
        let bba = format!("{group_prefix}:bba");
        let aaa_control = format!("sidebar-group-{aaa}");
        let bba_control = format!("sidebar-group-{bba}");
        let leaf_key = |name: &str| {
            if pane == 0 {
                format!("branch:{name}")
            } else {
                format!("remote:origin/{name}")
            }
        };
        let leaf_control = |name: &str| {
            if pane == 0 {
                format!("sidebar-local-{name}")
            } else {
                format!("sidebar-remote-origin/{name}")
            }
        };
        let mut t = Instant::now();
        drawn(cx, &app, window, t);
        for target in [leaf_key("aaa/e"), leaf_key("aaa/f"), format!("group:{bba}")] {
            theme::set_reduce_motion(true);
            if compact && e2e::control_bounds(window.window_id(), POPUP).is_none() {
                click(cx, &app, window, rail);
            }
            drawn(cx, &app, window, t);
            for (key, id) in [(&aaa, &aaa_control), (&bba, &bba_control)] {
                if !collapsed(cx, &app, key) {
                    click(cx, &app, window, id);
                }
            }
            app.update(cx, |app, cx| {
                app.jump_to_branch("main");
                cx.notify();
            });
            drawn(cx, &app, window, t);
            let baseline = app.read_with(cx, |app, _| app.ui().selected);
            let name = target
                .strip_prefix(if pane == 0 {
                    "branch:"
                } else {
                    "remote:origin/"
                })
                .unwrap_or("aaa/e");
            let expected_tip = app.read_with(cx, |app, _| {
                app.view().commit_row_index[&app.view().branch_targets[name]]
            });
            assert_ne!(
                baseline,
                Some(expected_tip),
                "a no-op cannot satisfy the target oracle"
            );
            theme::set_reduce_motion(false);
            click(cx, &app, window, &aaa_control);
            let at = t + Duration::from_millis(37);
            drawn(cx, &app, window, at);
            let row = sidebar::sidebar_motion_for_e2e(pane, &target, "row")
                .expect("translated semantic row");
            assert!(row.opacity > 0., "the real target is being painted");
            let id = if target.starts_with("group:") {
                bba_control.clone()
            } else {
                leaf_control(name)
            };
            let bounds =
                e2e::control_bounds(window.window_id(), &id).expect("actual translated control");
            let (covered, top, bottom) = ["aaa/a", "aaa/b", "aaa/c", "aaa/d", "aaa/e", "aaa/f"]
                .into_iter()
                .find_map(|name| {
                    let key = leaf_key(name);
                    if key == target {
                        return None;
                    }
                    let below = e2e::control_bounds(window.window_id(), &leaf_control(name))?;
                    let top = bounds.top().max(below.top());
                    let bottom = bounds.bottom().min(below.bottom());
                    (bottom - top > px(2.)).then_some((key, top, bottom))
                })
                .expect("37ms actual translated control lies over a different real leaf");
            let hit = point(bounds.center().x, (top + bottom) * 0.5);
            cx.simulate_keystrokes(window, "f19");
            support::focus(cx, &app, window, pane, &covered);
            pointer(cx, window, hit, MouseButton::Right);
            let menu = app.read_with(cx, |app, _| {
                app.branch_menu.as_ref().map(|menu| menu.name.clone())
            });
            let expected_menu = target
                .strip_prefix("branch:")
                .or_else(|| target.strip_prefix("remote:"))
                .map(str::to_owned);
            if menu != expected_menu {
                failures.push(format!(
                    "compact={compact}/{target}: right-menu {menu:?}, expected {expected_menu:?}"
                ));
            }
            if support::focused(cx, &app, window).is_some() {
                failures.push(format!(
                    "compact={compact}/{target}: right press stole covered-row focus"
                ));
            }
            app.update(cx, |app, cx| {
                app.branch_menu = None;
                app.jump_to_branch("main");
                cx.notify();
            });
            drawn(cx, &app, window, at);
            cx.simulate_keystrokes(window, "f19");
            support::focus(cx, &app, window, pane, &covered);
            pointer(cx, window, hit, MouseButton::Left);
            let actual = app.read_with(cx, |app, _| app.ui().selected);
            let header = target.starts_with("group:");
            let expected = if header { baseline } else { Some(expected_tip) };
            if actual != expected {
                failures.push(format!(
                    "compact={compact}/{target}: selected {actual:?}, expected {expected:?}"
                ));
            }
            if support::focused(cx, &app, window).is_some() {
                failures.push(format!(
                    "compact={compact}/{target}: left press stole covered-row focus"
                ));
            }
            drawn(cx, &app, window, at);
            if header && collapsed(cx, &app, &bba) {
                failures.push(format!(
                    "compact={compact}/{target}: actual neighboring subgroup did not toggle"
                ));
            }
            if compact {
                let open = e2e::control_bounds(window.window_id(), POPUP).is_some();
                if open != header {
                    failures.push(format!(
                        "compact/{target}: popup open={open}, expected {header}"
                    ));
                }
            }
            t += Duration::from_millis(500);
            drawn(cx, &app, window, t);
            if compact && e2e::control_bounds(window.window_id(), POPUP).is_none() {
                click(cx, &app, window, rail);
                drawn(cx, &app, window, t);
            }
            cx.simulate_keystrokes(window, "f19");
            support::focus(cx, &app, window, pane, &leaf_key("bbb"));
            cx.simulate_keystrokes(window, "down");
            drawn(cx, &app, window, t);
            assert_eq!(
                support::focused(cx, &app, window),
                Some((pane, leaf_key("main"))),
                "real arrow navigation survives containment"
            );
            cx.simulate_keystrokes(window, "shift-f10");
            assert_eq!(
                app.read_with(cx, |app, _| app
                    .branch_menu
                    .as_ref()
                    .map(|menu| menu.name.as_str().to_owned())),
                Some(if pane == 0 { "main" } else { "origin/main" }.into()),
                "keyboard menu retains focused identity"
            );
            app.update(cx, |app, cx| {
                app.branch_menu = None;
                cx.notify();
            });
            drawn(cx, &app, window, t);
        }
        if compact && pane == 0 {
            theme::set_reduce_motion(true);
            cx.simulate_keystrokes(window, "escape");
            drawn(cx, &app, window, t);
            click(cx, &app, window, "compact-rail-worktrees");
            drawn(cx, &app, window, t);
            let name =
                e2e::control_bounds(window.window_id(), "sidebar-worktree-name-pointer-linked")
                    .expect("real linked worktree name");
            let owner = app.read_with(cx, |app, _| app.active_session());
            pointer(cx, window, name.center(), MouseButton::Left);
            assert_ne!(
                app.read_with(cx, |app, _| app.active_session()),
                owner,
                "compact worktree click must actually select another repository"
            );
            assert_eq!(
                app.read_with(cx, |app, _| app.repo_path.clone()),
                Some(linked_path.canonicalize().unwrap())
            );
            drawn(cx, &app, window, t);
            assert!(
                e2e::control_bounds(window.window_id(), POPUP).is_none(),
                "compact worktree selection also dismisses its popover"
            );
        }
        unmount(cx, app, window);
    }
    assert_eq!(
        support::Witness::read(repo),
        witness,
        "pointer navigation and menus do not mutate repository state"
    );
    pointer_other::other_rows(cx, &mut failures);
    pointer_ghost::presentation_rows(cx, &mut failures);
    assert!(
        failures.is_empty(),
        "compressed Sidebar semantic ownership: {}",
        failures.join("; ")
    );
    eprintln!("[gui-e2e] PASS sidebar_tree_pointer_motion");
}
