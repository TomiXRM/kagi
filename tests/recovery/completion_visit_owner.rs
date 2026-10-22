//! Frozen Clone context and Stash visits own completion, not the active Graph.
use crate::busy_dismiss::{paint, ready, wait};
use crate::evidence_support::deferred;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;
use gpui::VisualTestAppContext;
use kagi::app::PlanState;
use kagi::ui::{e2e, i18n::Lang, FooterStatus, ToastAction, ToastKind};
use kagi_git::{github_repos::RepoListing, oplog::{self, OpOutcome}};

pub fn scenario_clone_departed_result_owner(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "recent_repos"]);
    for lang in [Lang::En, Lang::Ja] {
        for unknown in [false, true] {
            for recording_failure in [false, true] {
                clone_departed(cx, lang, unknown, recording_failure);
            }
        }
    }
    eprintln!("[gui-e2e] PASS clone_departed_result_owner");
}

fn clone_departed(cx: &mut VisualTestAppContext, lang: Lang, unknown: bool, recording_failure: bool) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let upstream = build_fixture();
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = bare_dir.path().join("widgets.git");
    git(upstream.path(), &["clone", "-q", "--bare", ".", bare.to_str().unwrap()]);
    // A real signal, not a fabricated RunReport or a shortened product timeout,
    // exercises Clone's negative-exit-status Unknown producer.
    let command = if unknown {
        "kill -KILL $$".to_owned()
    } else {
        format!("git clone -q '{}' \"$4\" && git -C \"$4\" remote set-url origin https://github.com/acme/widgets.git", bare.display())
    };
    let _gh = OfflineGh::with_script(&format!("#!/bin/sh\ncase \"$1 $2\" in\n'repo clone') {command} ;;\n*) echo '[]' ;;\nesac\n"));
    let parent = tempfile::tempdir().unwrap();
    let dest = parent.path().join("widgets");
    git(&repo, &["remote", "add", "origin", repo.join("missing-upstream").to_str().unwrap()]);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_window_active_for_e2e(false, cx);
        app.fetch_async(false, cx);
    });
    wait(cx, &app, "the independent Fetch failure settles", |app| app.fetch_in_flight.is_none());
    let (old_id, old_action) = cx.read(|cx| {
        let state = app.read(cx);
        let stack = state.toast_stack.as_ref().unwrap().read(cx);
        let toast = stack.toasts().last().expect("real Fetch supplies an independent result");
        assert!(matches!(toast.action, Some(ToastAction::OpenOperationLog(_))));
        (toast.id, format!("{:?}", toast.action))
    });
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |app, cx| app.open_home_tab(window, cx));
    }).unwrap();
    let (hold, release) = deferred(cx);
    e2e::queue_clone_run(hold);
    app.update(cx, |app, cx| {
        app.open_clone_card(RepoListing {
            name_with_owner: "acme/widgets".into(), host: "github.com".into(),
            updated_at: String::new(), description: String::new(), is_private: false, is_fork: false,
        }, cx);
        app.replan_clone(parent.path(), cx);
        app.start_clone(cx);
        app.cancel_clone();
        app.switch_repo(0, cx);
        // This is an explicit app-level active Graph fixture, not OS activation.
        app.set_window_active_for_e2e(true, cx);
    });
    ready(cx, &app);
    paint(cx, &app, window);
    let before = cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.repo_path.as_ref(), Some(&repo));
        assert!(!state.home_in_front());
        assert!(state.home_github.cloning.is_some());
        (state.active_session(), format!("{:?}", state.status_footer), state.bottom_panel_open, state.bottom_tab)
    });
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("operations.jsonl.lock");
    let recording_lock = recording_failure.then(|| {
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path).unwrap();
        lock.lock().unwrap();
        lock
    });
    release.send(());
    wait(cx, &app, "real Clone reaches its terminal receipt", |app| app.home_github.cloning.is_none());
    paint(cx, &app, window);
    let entries = oplog::read_oplog_tail_for_repo(&dest, 100);
    let clones: Vec<_> = entries.iter().filter(|entry| entry.op == "clone" && std::path::Path::new(&entry.repo).components().as_path() == dest).collect();
    assert_eq!(clones.len(), usize::from(!recording_failure));
    if !recording_failure {
        assert_eq!(matches!(clones[0].outcome, OpOutcome::Unknown { .. }), unknown);
        assert_eq!(matches!(clones[0].outcome, OpOutcome::Success { .. }), !unknown);
    }
    cx.read(|cx| {
        let state = app.read(cx);
        let stack = state.toast_stack.as_ref().unwrap().read(cx);
        let previous = stack.toasts().iter().find(|toast| toast.id == old_id).unwrap();
        eprintln!("[gui-e2e] clone_departed observed lang={lang:?} unknown={unknown} recording_failure={recording_failure} footer={:?} toasts={:?}", state.status_footer, stack.toasts().iter().map(|toast| (toast.id, toast.kind, &toast.action)).collect::<Vec<_>>());
        assert_eq!(format!("{:?}", previous.action), old_action, "Clone cannot rebind an independent receipt's Details action");
        assert_eq!((state.active_session(), format!("{:?}", state.status_footer), state.bottom_panel_open, state.bottom_tab), before, "departed Clone cannot change the current Graph presentation");
        assert_eq!(stack.toasts().len(), if recording_failure { 1 } else { 2 });
        if recording_failure {
            assert!(state.app_notice().is_some_and(|notice| notice.message.contains(&std::fs::TryLockError::WouldBlock.to_string())));
        } else {
            let toast = stack.toasts().iter().find(|toast| toast.id != old_id).unwrap();
            if unknown {
                assert_eq!(toast.kind, ToastKind::Error);
                assert!(matches!(toast.action, Some(ToastAction::OpenOperationLog(_))));
            } else {
                assert_eq!(toast.kind, ToastKind::Success);
                assert!(matches!(&toast.action, Some(ToastAction::OpenRepository(path)) if path == &dest));
            }
        }
    });
    drop(recording_lock);
    if recording_failure {
        app.update(cx, |app, cx| app.confirm_app_notice(cx));
    } else if !unknown {
        let bounds = e2e::control_bounds(window.window_id(), "clone-completion-open").expect("this Clone's Open Repository is rendered");
        cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
        ready(cx, &app);
        assert_eq!(cx.read(|cx| app.read(cx).repo_path.clone()), Some(dest.canonicalize().unwrap()));
    }
    assert_eq!(oplog::read_oplog_tail_for_repo(&dest, 100).iter().filter(|entry| entry.op == "clone").count(), usize::from(!recording_failure), "inspection/navigation never retries appending Clone");
    unmount(cx, app, window);
}

pub fn scenario_stash_revisit_result_owner(cx: &mut VisualTestAppContext) {
    let _restore = crate::recovery_layout::GlobalSettings::capture();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang", "recent_repos"]);
    for lang in [Lang::En, Lang::Ja] {
        let fixture = build_fixture();
        let other = build_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let foreign = other.path().canonicalize().unwrap();
        let original = std::fs::read(repo.join("README.md")).unwrap();
        std::fs::write(repo.join("README.md"), "stash the real tracked edit\n").unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| {
            app.set_lang(lang, cx);
            app.set_window_active_for_e2e(true, cx);
            app.open_stash_push_modal(cx);
        });
        wait(cx, &app, "actual Stash plan is approved through its normal handler", |app| matches!(app.app_sessions.plan_state(), PlanState::Ready { .. }));
        let owner = cx.read(|cx| {
            let state = app.read(cx);
            state.app_sessions.attachment(state.active_session().unwrap()).unwrap()
        });
        app.update(cx, |app, cx| {
            // One UI turn freezes real approval, then real A→B→A navigation
            // before any completion can be delivered; no fake report/state.
            app.confirm_stash_push(cx);
            assert!(app.app_sessions.has_leases());
            assert!(app.open_repository(foreign.clone(), cx));
            app.switch_repo(0, cx);
            assert_eq!(app.active_session(), Some(owner.session));
            assert_ne!(app.app_sessions.attachment(owner.session).unwrap().visit, owner.visit);
            app.status_footer = FooterStatus::Idle("new visit's presentation owner".into());
        });
        wait(cx, &app, "the real departed Stash write completes", |app| app.write_busy_op.is_none());
        ready(cx, &app);
        paint(cx, &app, window);
        let entries = oplog::read_oplog_tail_for_repo(&repo, 100);
        let stash: Vec<_> = entries.iter().filter(|entry| entry.op == "stash-push").collect();
        assert_eq!(stash.len(), 1);
        assert!(matches!(stash[0].outcome, OpOutcome::Success { .. }));
        assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), original);
        cx.read(|cx| {
            let state = app.read(cx);
            let stack = state.toast_stack.as_ref().unwrap().read(cx);
            eprintln!("[gui-e2e] stash_revisit observed owner={owner:?} current={:?} footer={:?} toasts={}", state.app_sessions.attachment(owner.session), state.status_footer, stack.toasts().len());
            assert_eq!(state.repo_path.as_ref(), Some(&repo));
            assert!(matches!(&state.status_footer, FooterStatus::Idle(message) if message.as_ref() == "new visit's presentation owner"));
            assert_eq!(stack.toasts().len(), 1, "old visit delivers its result globally instead of replacing the new footer");
            assert_eq!(stack.toasts()[0].kind, ToastKind::Success);
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS stash_revisit_result_owner");
}
