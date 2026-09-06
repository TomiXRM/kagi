//! Real root/modal inputs; no executor or approval-state seams.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::app::{PlanState, StashAction};
use kagi::ui::{e2e, modals::ActiveModal, FooterStatus, KagiApp};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;
use std::time::{Duration, Instant};

fn ids(repo: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["stash", "list", "--format=%H"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn stash_three(repo: &Path) {
    for text in ["one\n", "two\n", "three\n"] {
        std::fs::write(repo.join("README.md"), text).unwrap();
        git(repo, &["stash", "push", "-qm", text.trim()]);
    }
}
fn wait(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    predicate: impl Fn(&KagiApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| predicate(app.read(cx))) {
            return;
        }
        assert!(Instant::now() < deadline, "stash adapter did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn confirm(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    button: bool,
) {
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    if button {
        let bounds =
            e2e::confirm_bounds(window.window_id()).expect("current window confirm bounds");
        cx.simulate_mouse_move(window, bounds.center(), None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    } else {
        cx.simulate_keystrokes(window, "enter");
    }
}
pub fn scenario_stash_public_boundary(cx: &mut VisualTestAppContext) {
    for button in [false, true] {
        for action in [
            StashAction::Push {
                message: None,
                include_untracked: true,
            },
            StashAction::Apply { index: 1 },
            StashAction::Pop { index: 1 },
            StashAction::Drop { index: 1 },
        ] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            stash_three(&repo);
            let before = ids(&repo);
            let bytes = std::fs::read(repo.join("README.md")).unwrap();
            if matches!(action, StashAction::Push { .. }) {
                std::fs::write(repo.join("new"), "four\n").unwrap();
            }
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| match action {
                StashAction::Push { .. } => app.open_stash_push_modal(cx),
                StashAction::Apply { index } => app.open_stash_apply_modal(index, cx),
                StashAction::Pop { index } => app.open_pop_modal(index, cx),
                StashAction::Drop { index } => app.open_stash_drop_modal(index, cx),
            });
            wait(cx, &app, |app| {
                matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
            });
            confirm(cx, &app, window, button);
            wait(cx, &app, |app| app.busy_op.is_none());
            let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|e| e.op == action.name())
                .collect();
            assert_eq!(entries.len(), 1, "one actual receipt for {action:?}");
            assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
            match action {
                StashAction::Push { .. } => {
                    assert_eq!(ids(&repo).len(), 4);
                    assert!(!repo.join("new").exists());
                }
                StashAction::Apply { .. } => {
                    assert_eq!(ids(&repo), before);
                    assert_eq!(
                        std::fs::read_to_string(repo.join("README.md")).unwrap(),
                        "two\n"
                    );
                }
                StashAction::Pop { .. } => {
                    assert_eq!(ids(&repo), vec![before[0].clone(), before[2].clone()]);
                    assert_eq!(
                        std::fs::read_to_string(repo.join("README.md")).unwrap(),
                        "two\n"
                    );
                }
                StashAction::Drop { .. } => {
                    assert_eq!(ids(&repo), vec![before[0].clone(), before[2].clone()]);
                    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), bytes);
                }
            }
            assert!(cx.read(|cx| app.read(cx).app_sessions.may_close_host()));
            unmount(cx, app, window);
            eprintln!(
                "[gui-e2e] PASS app-stash {} {} → executor → one receipt",
                action.name(),
                if button { "button" } else { "raw Enter" }
            );
        }
    }
}

pub fn scenario_stash_conflict_followup(cx: &mut VisualTestAppContext) {
    for duplicate in [false, true] {
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        stash_three(&repo);
        let before = ids(&repo);
        std::fs::write(repo.join("README.md"), "ours\n").unwrap();
        git(&repo, &["commit", "-qam", "ours"]);
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.open_pop_modal(1, cx));
        wait(cx, &app, |app| {
            matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
        });
        confirm(cx, &app, window, duplicate);
        wait(cx, &app, |app| {
            app.busy_op.is_none() && app.conflict.is_some()
        });
        let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|e| e.op == "stash-pop")
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].outcome, OpOutcome::Partial { .. }));
        assert_eq!(ids(&repo), before);
        assert!(
            cx.read(|cx| !matches!(&app.read(cx).active_modal, Some(ActiveModal::AppNotice(_))))
        );
        assert!(cx.read(|cx| !matches!(app.read(cx).status_footer, FooterStatus::Success(_))));
        let other = build_fixture();
        let other_path = other.path().canonicalize().unwrap();
        app.update(cx, |app, cx| {
            assert!(app.open_repository(other_path.clone(), cx));
        });
        cx.run_until_parked();
        app.update(cx, |app, cx| app.switch_repo(0, cx));
        wait(cx, &app, |app| app.conflict.is_some());
        // #482 stage 1: the conflict belongs to the *session* that popped it.
        // B is a different session of a different repository and stays clean.
        let (owner, sibling) = cx.read(|cx| {
            let app = app.read(cx);
            (app.active_session().unwrap(), app.tabs[1].session)
        });
        assert_ne!(owner, sibling);
        assert!(cx.read(|cx| app.read(cx).app_sessions.stash_conflict(sibling).is_none()));
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .app_sessions
                .stash_conflict(owner)
                .unwrap()
                .oid
                .clone()),
            before[1]
        );
        if duplicate {
            git(&repo, &["stash", "store", "-m", "duplicate", &before[1]]);
        }
        let kept = ids(&repo);
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.conflict.as_ref().unwrap().update(cx, |view, _| {
                    view.mode
                        .as_mut()
                        .unwrap()
                        .buffer
                        .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                        .unwrap();
                });
                app.conflict_continue(window, cx);
            });
        })
        .unwrap();
        wait(cx, &app, |app| app.conflict.is_none());
        wait(cx, &app, |app| {
            if duplicate {
                app.app_sessions.stash_conflict(owner).is_none()
                    && !matches!(app.app_sessions.plan_state(), PlanState::Planning { .. })
            } else {
                app.stash_drop_modal().is_some()
                    && matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
            }
        });
        cx.read(|cx| {
            let app = app.read(cx);
            if duplicate {
                assert!(app.stash_drop_modal().is_none());
            } else {
                assert_eq!(app.stash_drop_modal().unwrap().stash_index, 1);
            }
        });
        cx.simulate_keystrokes(window, "escape");
        assert_eq!(ids(&repo), kept);
        unmount(cx, app, window);
        eprintln!("[gui-e2e] PASS app-stash deep conflict A→B→A → continue → unique OID prompt/cancel (duplicate={duplicate})");
    }
}

pub fn scenario_stash_conflict_close_reopen(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    stash_three(&repo);
    std::fs::write(repo.join("README.md"), "ours\n").unwrap();
    git(&repo, &["commit", "-qam", "ours"]);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_pop_modal(1, cx));
    wait(cx, &app, |app| {
        matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
    });
    confirm(cx, &app, window, false);
    wait(cx, &app, |app| {
        app.busy_op.is_none() && app.conflict.is_some()
    });

    let closed = cx.read(|cx| app.read(cx).active_session().unwrap());
    // Continue and close in the same host turn, before its async reload can
    // present the one-shot drop follow-up.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            app.conflict_continue(window, cx);
            app.close_tab(0, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo.clone(), cx));
    });
    wait(cx, &app, |app| {
        app.repo_path.as_ref() == Some(&repo) && app.conflict.is_none()
    });
    app.update(cx, |app, _| {
        // #482 stage 1: same path, new session. The closed owner's payloads went
        // with it, so nothing from before the close can be delivered here.
        let reopened = app.active_session().unwrap();
        assert_ne!(
            reopened, closed,
            "reopening a path must not reuse its owner"
        );
        assert!(!app.app_sessions.is_attached(closed));
        assert!(app.app_sessions.stash_conflict(reopened).is_none());
        assert!(app.app_sessions.take_stash_followup(reopened).is_none());
        assert!(app.app_sessions.stash_conflict(closed).is_none());
        assert!(app.stash_drop_modal().is_none());
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS app-stash continue → close → reopen has no drop prompt");
}

pub fn scenario_stash_replan_error(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    stash_three(&repo);
    let before = ids(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_stash_drop_modal(1, cx));
    wait(cx, &app, |app| {
        matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
    });
    cx.update_window(window, |_, window, cx| {
        window.focus(&app.read(cx).root_focus.clone().unwrap(), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    let old_bounds = e2e::confirm_bounds(window.window_id()).unwrap();
    let moved = repo.with_extension("moved");
    std::fs::rename(&repo, &moved).unwrap();
    app.update(cx, |app, cx| app.open_stash_drop_modal(1, cx));
    wait(cx, &app, |app| {
        matches!(app.app_sessions.plan_state(), PlanState::Error { .. })
    });
    cx.simulate_keystrokes(window, "enter");
    cx.simulate_mouse_move(window, old_bounds.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, old_bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(ids(&moved), before);
    assert!(cx.read(|cx| !app.read(cx).app_sessions.has_leases()));
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|e| e.op == "stash-drop")
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "only the adopted plan error, never the old execution"
    );
    assert!(matches!(entries[0].outcome, OpOutcome::Failed { .. }));
    std::fs::rename(&moved, &repo).unwrap();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS app-stash replan error rejects Enter and old button coordinate");
}

pub fn scenario_external_stash_conflict_has_no_drop_prompt(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    stash_three(&repo);
    let before = ids(&repo);
    std::fs::write(repo.join("README.md"), "ours\n").unwrap();
    git(&repo, &["commit", "-qam", "ours"]);
    let result = std::process::Command::new("git")
        .args(["stash", "pop", "stash@{1}"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(!result.status.success(), "fixture must conflict");
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.reload(cx));
    wait(cx, &app, |app| app.conflict.is_some());
    assert!(cx.read(|cx| {
        let app = app.read(cx);
        app.app_sessions
            .stash_conflict(app.active_session().unwrap())
            .is_none()
    }));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            app.conflict_continue(window, cx);
        });
    })
    .unwrap();
    wait(cx, &app, |app| app.conflict.is_none());
    assert!(cx.read(|cx| app.read(cx).stash_drop_modal().is_none()));
    assert_eq!(ids(&repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS external stash conflict → continue → no drop prompt");
}
