//! Real root/modal inputs; no executor or approval-state seams.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::app::{PlanState, StashAction};
use kagi::remote::stash::{
    remote_stash_e2e_refreshes, set_remote_stash_e2e_mode, RemoteStashE2eMode,
};
use kagi::ui::{e2e, i18n, modals::ActiveModal, FooterStatus, KagiApp};
use kagi_domain::remote::RemoteHost;
use kagi_git::oplog::{read_oplog_tail, read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git_command, git_output};

fn ids(repo: &Path) -> Vec<String> {
    git_output(repo, &["stash", "list", "--format=%H"])
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
fn snapshot(repo: &Path) -> kagi_git::RepoSnapshot {
    kagi_git::Backend::open(repo)
        .unwrap()
        .snapshot(10_000)
        .unwrap()
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

pub fn scenario_stash_push_plan_accuracy(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_lang = i18n::lang();
    for lang in [i18n::Lang::En, i18n::Lang::Ja] {
        i18n::set_lang(lang);
        let fixture = tempfile::tempdir().unwrap();
        let repo = fixture.path().canonicalize().unwrap();
        git_fixture::init_repo(&repo, "main");
        std::fs::write(repo.join("new"), "keep\n").unwrap();
        let before_head = std::fs::read(repo.join(".git/HEAD")).unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.open_stash_push_modal(cx));
        wait(cx, &app, |app| {
            app.stash_push_modal().is_some_and(|m| m.plan.is_some())
        });
        let reason = match lang {
            i18n::Lang::En => "Stash push requires a HEAD commit. Create an initial commit before stashing.",
            i18n::Lang::Ja => "stash push には HEAD commit が必要です。最初の commit を作成してから stash してください。",
        };
        cx.read(|cx| {
            let plan = app
                .read(cx)
                .stash_push_modal()
                .unwrap()
                .plan
                .as_ref()
                .unwrap();
            assert!(plan
                .blockers
                .iter()
                .any(|note| i18n::plan_note_text(note) == reason));
            assert_eq!(plan.predicted, plan.current);
        });
        kagi::ui::button_style::clear_recorded_modal_buttons();
        app.update(cx, |_, cx| cx.notify());
        crate::recovery_operations::paint(cx, window);
        let button = kagi::ui::button_style::recorded_modal_button("stash-push-confirm").unwrap();
        assert!(button.disabled);
        assert_eq!(button.description.as_deref(), Some(reason));
        // The reason is passed to the actual rendered disabled action, not
        // merely present in the backend plan. Clicking cannot execute it.
        let bounds = e2e::confirm_bounds(window.window_id()).unwrap();
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(cx.read(|cx| app.read(cx).stash_push_modal().is_some()));
        confirm(cx, &app, window, false);
        wait(cx, &app, |app| {
            !app.app_sessions.has_leases() && app.stash_push_modal().is_none()
        });
        assert!(cx.read(|cx| matches!(&app.read(cx).status_footer,
            FooterStatus::Failed(text) if text.contains(reason))));
        let entries = read_oplog_tail_for_repo(&repo, 100);
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
            if blockers.iter().any(|b| b.contains("HEAD commit")))
        );
        assert_eq!(std::fs::read(repo.join(".git/HEAD")).unwrap(), before_head);
        assert_eq!(std::fs::read_to_string(repo.join("new")).unwrap(), "keep\n");
        assert!(!repo.join(".git/index").exists());
        assert!(!repo.join(".git/refs/stash").exists());
        unmount(cx, app, window);
    }
    i18n::set_lang(original_lang);
    eprintln!("[gui-e2e] PASS stash_push_plan_accuracy EN/JA");
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
            wait(cx, &app, |app| app.write_busy_op.is_none());
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

pub fn scenario_remote_stash_drop(cx: &mut VisualTestAppContext) {
    let host = RemoteHost {
        user: Some("kagi-e2e".into()),
        host: "e2e.invalid".into(),
        port: Some(22),
        identity_file: Some("/e2e/id".into()),
    };
    for button in [false, true] {
        set_remote_stash_e2e_mode(Some(RemoteStashE2eMode::Success));
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        stash_three(&repo);
        let snap = snapshot(&repo);
        // The whole shared log: a 100-entry window stops growing once
        // earlier scenarios have filled it (#516).
        let before = read_oplog_tail(usize::MAX).len();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.enter_remote_view(host.clone(), "/srv/repo".into(), snap, cx);
            // #733: this tab's worktree is fabricated with the *remote host's*
            // path while every worktree-menu action runs locally, so the menu
            // must refuse to open here at all. The refusal lives at that seam
            // rather than at each call site, which is what this exercises.
            app.open_worktree_menu(
                "main".into(),
                false,
                true,
                Some("/srv/repo".into()),
                gpui::Point::default(),
            );
            assert!(
                app.worktree_menu.is_none(),
                "a remote view must open no worktree menu"
            );
            app.open_stash_drop_modal(1, cx);
        });
        wait(cx, &app, |app| {
            matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
                && app
                    .stash_drop_modal()
                    .is_some_and(|modal| modal.plan.is_some())
        });
        confirm(cx, &app, window, button);
        wait(cx, &app, |app| {
            !app.app_sessions.has_leases() && matches!(&app.status_footer, FooterStatus::Success(_))
        });
        let entries = read_oplog_tail(usize::MAX);
        assert_eq!(entries.len(), before + 1);
        assert_eq!(entries[0].op, "stash-drop");
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(remote_stash_e2e_refreshes(), 1);
        unmount(cx, app, window);
        set_remote_stash_e2e_mode(None);
        eprintln!(
            "[gui-e2e] PASS remote stash-drop {} → typed transport → refresh + one receipt",
            if button { "button" } else { "raw Enter" }
        );
    }

    let (lang_before, saved_lang) = (i18n::lang(), kagi::ui::settings::read_setting("lang"));
    for (lang, mode, expected_lease) in [
        (i18n::Lang::En, RemoteStashE2eMode::Refused, false),
        (i18n::Lang::Ja, RemoteStashE2eMode::Unknown, true),
    ] {
        i18n::set_lang(lang);
        set_remote_stash_e2e_mode(Some(mode));
        let fixture = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        stash_three(&repo);
        let snap = snapshot(&repo);
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.enter_remote_view(host.clone(), "/srv/repo".into(), snap, cx);
            app.open_stash_drop_modal(1, cx);
        });
        wait(cx, &app, |app| {
            matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
        });
        confirm(cx, &app, window, false);
        wait(cx, &app, |app| {
            app.app_sessions.has_leases() == expected_lease
                && matches!(&app.status_footer, FooterStatus::Failed(_))
        });
        if mode == RemoteStashE2eMode::Unknown {
            assert!(cx.read(|cx| matches!(
                &app.read(cx).status_footer,
                FooterStatus::Failed(message)
                    if message.contains(i18n::Msg::RemoteOpAwaitingCompletion.t())
            )));
        }
        unmount(cx, app, window);
        set_remote_stash_e2e_mode(None);
    }

    set_remote_stash_e2e_mode(Some(RemoteStashE2eMode::Success));
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    stash_three(&repo);
    let snap = snapshot(&repo);
    let before = read_oplog_tail(usize::MAX).len();
    let (app, window) = mount(cx, &repo);
    let guard = app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap());
    app.update(cx, |app, cx| {
        app.enter_remote_view(host, "/srv/repo".into(), snap, cx);
        app.open_stash_drop_modal(1, cx);
    });
    wait(cx, &app, |app| {
        matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
    });
    confirm(cx, &app, window, false);
    wait(cx, &app, |app| {
        matches!(
            &app.status_footer,
            FooterStatus::Failed(message) if message.as_ref() == i18n::Msg::OpInProgress.t()
        )
    });
    assert_eq!(read_oplog_tail(usize::MAX).len(), before);
    guard.complete();
    unmount(cx, app, window);
    set_remote_stash_e2e_mode(None);
    // The language and its saved key as this scenario found them (#516).
    i18n::set_lang(lang_before);
    kagi::ui::settings::write_setting("lang", saved_lang.as_deref());
    eprintln!("[gui-e2e] PASS remote stash-drop Busy/Refused/Unknown EN/JA + Unknown lease hold");
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
            app.write_busy_op.is_none() && app.ui().conflict.is_some()
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
        let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
        app.update(cx, |app, cx| {
            assert!(app.open_repository(other_path.clone(), cx));
        });
        cx.run_until_parked();
        // #482 stage 1: the conflict belongs to the *session* that popped it, and
        // to the visit it popped it in. B is a different session of a different
        // repository and stays clean; A has been departed, so its payload
        // proposes nothing while the user is away.
        let sibling = cx.read(|cx| app.read(cx).active_session().unwrap());
        assert_ne!(owner, sibling);
        cx.read(|cx| {
            let sessions = &app.read(cx).app_sessions;
            assert!(sessions.stash_conflict(sibling).is_none());
            assert!(
                sessions.stash_conflict(owner).is_none(),
                "#482: leaving A ends its visit — a departed proposal is inert"
            );
        });
        app.update(cx, |app, cx| app.switch_repo(0, cx));
        wait(cx, &app, |app| app.ui().conflict.is_some());
        // Returning re-detects the conflict from the repository, and *that* live
        // re-observation is what makes it proposable again — the OID belongs to
        // the conflict that is actually still there, not to a preserved payload.
        assert_eq!(
            cx.read(|cx| app
                .read(cx)
                .app_sessions
                .stash_conflict(owner)
                .expect("the live re-detect re-proves the conflict")
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
                app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                    view.mode
                        .as_mut()
                        .unwrap()
                        .buffer
                        .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                        .unwrap();
                });
                let owner = app
                    .active_session()
                    .and_then(|session| app.app_sessions.attachment(session))
                    .expect("continue owner");
                app.conflict_continue(owner, window, cx);
            });
        })
        .unwrap();
        wait(cx, &app, |app| app.ui().conflict.is_none());
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
                // The follow-up resolved its OID to the entry that is still
                // there, not to the placeholder it reserved the slot with.
                assert_eq!(app.stash_drop_modal().unwrap().stash_index, Some(1));
            }
        });
        if duplicate {
            cx.simulate_keystrokes(window, "escape");
            assert_eq!(ids(&repo), kept);
        } else {
            // Cancelling is not the only thing this prompt must support: the
            // point of the follow-up is that it can be *confirmed*, and that it
            // drops the entry its OID resolved to — not the placeholder index
            // it reserved the modal slot with.
            confirm(cx, &app, window, false);
            wait(cx, &app, |app| {
                app.write_busy_op.is_none() && app.stash_drop_modal().is_none()
            });
            assert_eq!(ids(&repo), vec![kept[0].clone(), kept[2].clone()]);
        }
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
        app.write_busy_op.is_none() && app.ui().conflict.is_some()
    });

    // #891 review: Continue on the still-unresolved conflict is refused at
    // planning; that refusal is "recorded, nothing moved", not "no record".
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            let owner = app
                .active_session()
                .and_then(|session| app.app_sessions.attachment(session))
                .expect("continue owner");
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    let refused = kagi_git::oplog::read_oplog_tail(1).pop().unwrap();
    assert!(
        refused.op.ends_with("-continue")
            && matches!(refused.outcome, kagi_git::OpOutcome::Refused { .. }),
        "{refused:?}"
    );
    assert_eq!(refused.ref_moves, Some(Vec::new()));
    let closed = cx.read(|cx| app.read(cx).active_session().unwrap());
    // Continue and close in the same host turn, before its async reload can
    // present the one-shot drop follow-up.
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            let owner = app
                .active_session()
                .and_then(|session| app.app_sessions.attachment(session))
                .expect("continue owner");
            app.conflict_continue(owner, window, cx);
            app.close_tab(0, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    // #884: the UI continue records the refs it moved (a stash continue only
    // stages, so: recorded, nothing moved) — not "no record".
    let continued = kagi_git::oplog::read_oplog_tail(1).pop().unwrap();
    assert!(continued.op.ends_with("-continue"), "{}", continued.op);
    assert_eq!(continued.ref_moves, Some(Vec::new()));
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo.clone(), cx));
    });
    wait(cx, &app, |app| {
        app.repo_path.as_ref() == Some(&repo) && app.ui().conflict.is_none()
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
    let result = git_command(&repo)
        .args(["stash", "pop", "stash@{1}"])
        .output()
        .unwrap();
    assert!(!result.status.success(), "fixture must conflict");
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.reload(cx));
    wait(cx, &app, |app| app.ui().conflict.is_some());
    assert!(cx.read(|cx| {
        let app = app.read(cx);
        app.app_sessions
            .stash_conflict(app.active_session().unwrap())
            .is_none()
    }));
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            let owner = app
                .active_session()
                .and_then(|session| app.app_sessions.attachment(session))
                .expect("continue owner");
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    wait(cx, &app, |app| app.ui().conflict.is_none());
    assert!(cx.read(|cx| app.read(cx).stash_drop_modal().is_none()));
    assert_eq!(ids(&repo), before);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS external stash conflict → continue → no drop prompt");
}

/// #652: the apply modal announces a conflict before the user confirms.
///
/// Pop predicted this and apply did not, so the same stash was announced through
/// one path and silent through the other. Showing what an operation will do
/// before it runs is the product, so this checks the plan the user actually
/// sees, not just the planner.
pub fn scenario_stash_apply_conflict_preview(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();

    // A stash that cannot apply cleanly, against a clean tree: stash one side,
    // then commit the other. A dirty tree would block the plan before it ever
    // reached the prediction.
    std::fs::write(repo.join("README.md"), "stashed side\n").unwrap();
    git(&repo, &["stash", "push", "-qm", "wip"]);
    std::fs::write(repo.join("README.md"), "committed side\n").unwrap();
    git(&repo, &["commit", "-qam", "diverge from the stash"]);

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_stash_apply_modal(0, cx));
    wait(cx, &app, |app| {
        matches!(app.app_sessions.plan_state(), PlanState::Ready { .. })
            && app
                .stash_apply_modal()
                .is_some_and(|modal| modal.plan.is_some())
    });

    cx.read(|cx| {
        let app = app.read(cx);
        let plan = app
            .stash_apply_modal()
            .and_then(|modal| modal.plan.as_ref())
            .expect("the apply modal must carry its plan");
        assert!(
            plan.blockers.is_empty(),
            "a conflicting apply stays confirmable — the stash survives it: {:?}",
            plan.blockers
        );
        let warned = plan.warnings.iter().any(|note| {
            matches!(
                note,
                kagi_domain::plan_note::PlanNote::Stash(
                    kagi_domain::plan_note::StashNote::ApplyWouldConflict { .. }
                )
            )
        });
        assert!(
            warned,
            "the modal must say the stash will conflict before it is confirmed (#652): {:?}",
            plan.warnings
        );
    });

    unmount(cx, app, window);
}

pub fn scenario_stash_continue_panic(cx: &mut VisualTestAppContext) {
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
        app.write_busy_op.is_none() && app.ui().conflict.is_some()
    });
    KagiApp::panic_next_continue_stash_for_e2e();
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
        });
    })
    .unwrap();
    wait(cx, &app, |app| app.app_sessions.reconcile_ids().len() == 1);
    let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 30)
        .into_iter()
        .filter(|entry| entry.op == "stash-continue")
        .collect();
    assert_eq!(receipts.len(), 1);
    assert!(matches!(receipts[0].outcome, OpOutcome::Unknown { .. }));
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert!(cx.read(|cx| app.read(cx).app_notice().is_some()));
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS stash_continue_panic");
}

pub fn scenario_stash_continue_after_tab_switch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_repo = other.path().canonicalize().unwrap();
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
        app.write_busy_op.is_none() && app.ui().conflict.is_some()
    });
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx))
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    wait(cx, &app, |app| app.ui().conflict.is_some());
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.ui().conflict.as_ref().unwrap().update(cx, |view, _| {
                view.mode
                    .as_mut()
                    .unwrap()
                    .buffer
                    .apply_choice(Path::new("README.md"), kagi_git::ResolutionChoice::Incoming)
                    .unwrap();
            });
            let owner = app
                .app_sessions
                .attachment(app.active_session().unwrap())
                .unwrap();
            app.conflict_continue(owner, window, cx);
            app.switch_repo(1, cx);
            app.status_footer = FooterStatus::Idle("other tab sentinel".into());
        });
    })
    .unwrap();
    wait(cx, &app, |app| app.write_busy_op.is_none());
    let receipts: Vec<_> = read_oplog_tail_for_repo(&repo, 30)
        .into_iter()
        .filter(|entry| entry.op == "stash-continue")
        .collect();
    assert_eq!(
        receipts.len(),
        1,
        "departed write must persist exactly once"
    );
    assert!(matches!(receipts[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(receipts[0].ref_moves, Some(Vec::new()));
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(matches!(&state.status_footer, FooterStatus::Idle(text) if text.as_ref() == "other tab sentinel"));
        assert!(!state.toast_stack.as_ref().unwrap().read(cx).toasts().iter()
            .any(|toast| toast.message.as_ref().contains("stash-continue")));
        assert_eq!(state.repo_path.as_ref(), Some(&other_repo));
    });
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS stash_continue_after_tab_switch");
}
