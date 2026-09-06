//! Raw Enter and actual confirmation-button clicks through the real root.
use crate::macos::{build_fixture, git, mount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::KagiApp;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::time::{Duration, Instant};

pub fn scenario_remove_public_boundary(cx: &mut VisualTestAppContext) {
    for button in [false, true] {
        let fixture = build_fixture();
        let worktrees = tempfile::tempdir().unwrap();
        let linked = worktrees.path().join("remove-target");
        git(
            fixture.path(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "remove-target",
                linked.to_str().unwrap(),
            ],
        );
        let (app, window) = mount(cx, fixture.path());
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
            cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
        } else {
            cx.simulate_keystrokes(window, "enter");
        }
        wait_idle(cx, &app);
        assert!(!linked.exists(), "input must reach real remove executor");
        let entries: Vec<_> = read_oplog_tail_for_repo(fixture.path(), 100)
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
