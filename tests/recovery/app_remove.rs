//! Raw Enter and actual confirmation-button clicks through the real root.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::time::{Duration, Instant};

pub fn scenario_remove_public_boundary(cx: &mut VisualTestAppContext) {
    for button in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let worktrees = tempfile::tempdir().unwrap();
        let linked = worktrees
            .path()
            .canonicalize()
            .unwrap()
            .join("remove-target");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "remove-target",
                linked.to_str().unwrap(),
            ],
        );
        let (app, window) = mount(cx, &repo);
        // #488 / #528: the target worktree is also open, in a *background* tab.
        // Removing it must close that tab without re-initializing the session
        // the operation is attached to (a re-init would strand this very
        // completion behind a stale `switch_generation`).
        let generation = app.update(cx, |app, cx| {
            assert!(app.open_repository(linked.clone(), cx));
            app.switch_repo(0, cx);
            app.switch_generation
        });
        cx.run_until_parked();
        app.update(cx, |app, cx| {
            app.open_remove_worktree_modal("remove-target".into(), true, cx);
            cx.notify();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.run_until_parked();
            if cx.read(|cx| app.read(cx).remove_worktree_modal().is_some()) {
                break;
            }
            assert!(Instant::now() < deadline, "remove plan did not arrive");
            std::thread::sleep(Duration::from_millis(2));
        }
        cx.update_window(window, |_, window, cx| {
            window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
            window.draw(cx).clear();
        })
        .unwrap();
        if button {
            let bounds = kagi::ui::e2e::confirm_bounds(window.window_id())
                .expect("real confirm button laid out");
            cx.update_window(window, |_, native_window, _| {
                eprintln!(
                    "[gui-e2e] app-remove button window={:?} bounds={:?} viewport={:?} window_bounds={:?}",
                    window.window_id(),
                    bounds,
                    native_window.viewport_size(),
                    native_window.bounds(),
                );
            })
            .unwrap();
            // GPUI's button hit testing needs the hover position established
            // before mouse-down; simulate_click only sends down/up events.
            cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
            cx.run_until_parked();
            cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
            assert!(
                cx.read(|cx| app.read(cx).remove_worktree_modal().is_none()),
                "confirm click must consume this window's remove modal"
            );
        } else {
            cx.simulate_keystrokes(window, "enter");
        }
        wait_idle(cx, &app);
        assert!(!linked.exists(), "input must reach real remove executor");
        cx.read(|cx| {
            let app = app.read(cx);
            assert!(
                !app.tabs.iter().any(|t| t.path == linked),
                "#528: the removed worktree's tab must close"
            );
            assert_eq!(app.active_tab, 0, "the surviving tab stays active");
            assert_eq!(
                app.switch_generation, generation,
                "#488: closing a background tab must not re-initialize the session"
            );
        });
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|e| e.op == "remove-worktree")
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "boundary receipt once; no legacy UI writer"
        );
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(entries[0].worktree.as_deref(), linked.to_str());
        assert!(cx.read(|cx| app.read(cx).app_sessions.may_close_host()));
        unmount(cx, app, window);
        eprintln!(
            "[gui-e2e] PASS app-remove {} → executor → one receipt",
            if button { "button" } else { "raw Enter" }
        );
    }
}

fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).busy_op.is_none()) {
            break;
        }
        assert!(Instant::now() < deadline, "remove did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
