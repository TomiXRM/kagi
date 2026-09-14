//! #643 Wave 4 S4: retained read caches revalidate on activation and undo
//! history stays session-owned without weakening the backend's stale-ref guard.

use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi_git::{CommitId, OperationKind};
use std::path::Path;
use std::process::Command;

fn output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("spawn git");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_string()
}

pub fn scenario_read_cache_revalidates_on_activation(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("canonical A");
    let repo_b = fixture_b.path().canonicalize().expect("canonical B");
    std::fs::write(repo_a.join("README.md"), "# fixture\nold dirty line\n").expect("dirty A");

    let (app, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    let (session_a, session_b) = app.update(cx, |app, cx| {
        app.reload_checked(cx).expect("baseline reload A");
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.select_headless(0);
        app.open_main_diff_commit(0, cx);
        assert!(
            !app.ui().diff_caches.changed_files.is_empty(),
            "cache-owner-a-populated: A must have a real commit-diff cache"
        );
        assert!(
            app.ui().main_diff.is_some(),
            "cache-owner-a-diff-open: A diff did not open"
        );
    });
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        app.switch_repo(1, cx);
        assert_eq!(app.active_session(), Some(session_b));
        assert!(
            app.ui().diff_caches.changed_files.is_empty(),
            "cache-does-not-leak-into-b: B exposed A's changed-file cache"
        );
    });
    std::fs::write(
        repo_a.join("README.md"),
        "# fixture\nreplacement line\nmore lines\nthan before\n",
    )
    .expect("change A while inactive");

    app.update(cx, |app, cx| {
        assert!(
            app.ui.get(&session_a).is_some_and(|ui| {
                !ui.diff_caches.changed_files.is_empty()
                    && ui.wip_diffstat.is_some()
                    && ui.last_working_status.is_some()
            }),
            "cache-a-retained-before-activation: A payload was not retained while inactive"
        );
        app.switch_repo(0, cx);
        assert_eq!(app.active_session(), Some(session_a));
        let ui = app.ui();
        assert!(
            ui.diff_caches.changed_files.is_empty()
                && ui.wip_diffstat.is_none()
                && ui.last_working_status.is_none(),
            "activation-clears-stale-cache-payloads: retained cache was authoritative before revalidation"
        );
        assert!(
            app.ui().main_diff.is_some(),
            "activation-retains-owned-open-diff: A's diff was discarded on departure"
        );
    });
    cx.run_until_parked();

    app.update(cx, |app, _| {
        let ui = app.ui();
        assert!(
            ui.last_working_status
                .as_ref()
                .is_some_and(kagi_git::WorkingTreeStatus::is_dirty),
            "activation-revalidates-working-status: A's external edit was not observed"
        );
        assert!(
            ui.wip_diffstat.is_some_and(|stat| stat.additions >= 3),
            "activation-revalidates-wip-diffstat: A kept the old diffstat"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS read_cache_revalidates_on_activation");
}

pub fn scenario_operation_history_session_and_stale_ref(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("canonical A");
    let repo_b = fixture_b.path().canonicalize().expect("canonical B");
    let before = output(&repo_a, &["rev-parse", "HEAD~1"]);
    let after = output(&repo_a, &["rev-parse", "HEAD"]);

    let (app, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    let session_a = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        app.tabs[0].session
    });
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        app.ui_mut().expect("active session").operation_history = Default::default();
        app.record_history(
            OperationKind::Commit,
            "main",
            CommitId(before.clone()),
            CommitId(after.clone()),
            "A retained operation",
        );
        app.ui_mut().expect("active session").history_seed_attempted = true;
        app.switch_repo(1, cx);
        assert!(
            kagi::ui::e2e::undo_head(app)
                .is_none_or(|(_, oid, summary)| oid != after || summary != "A retained operation"),
            "history-does-not-leak-into-b: B exposed A's undo target"
        );
    });

    git(
        &repo_a,
        &["commit", "-q", "--allow-empty", "-m", "external move"],
    );
    let external_head = output(&repo_a, &["rev-parse", "HEAD"]);
    app.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_eq!(app.active_session(), Some(session_a));
        assert_eq!(
            kagi::ui::e2e::undo_head(app),
            Some((
                "main".to_string(),
                after.clone(),
                "A retained operation".to_string()
            )),
            "history-restores-with-a: A's undo cursor was not retained"
        );
        assert!(
            app.ui().history_seed_attempted,
            "history-seed-guard-restores-with-a: A's run-once guard was reset"
        );
        app.open_history_undo_modal();
        assert!(
            app.history_modal()
                .is_some_and(|modal| !modal.plan.blockers.is_empty()),
            "history-stale-ref-is-blocked-on-return: external ref movement was accepted"
        );
        app.confirm_history(cx);
    });
    assert_eq!(
        output(&repo_a, &["rev-parse", "HEAD"]),
        external_head,
        "history-stale-ref-preserves-head: rejected undo moved the external ref"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS operation_history_session_and_stale_ref");
}

pub fn scenario_background_operation_history_owner(cx: &mut VisualTestAppContext) {
    let fixture_a = build_fixture();
    let fixture_b = build_fixture();
    let repo_a = fixture_a.path().canonicalize().expect("canonical A");
    let repo_b = fixture_b.path().canonicalize().expect("canonical B");
    git(&repo_a, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    std::fs::write(repo_a.join("first.txt"), "first\n").expect("write first");
    git(&repo_a, &["add", "first.txt"]);
    git(&repo_a, &["commit", "-q", "-m", "first side"]);
    let first = output(&repo_a, &["rev-parse", "HEAD"]);
    std::fs::write(repo_a.join("second.txt"), "second\n").expect("write second");
    git(&repo_a, &["add", "second.txt"]);
    git(&repo_a, &["commit", "-q", "-m", "second side"]);
    let second = output(&repo_a, &["rev-parse", "HEAD"]);
    git(&repo_a, &["checkout", "-q", "main"]);

    let (app, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    let (session_a, session_b) = app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b, cx), "open B");
        let session_a = app.tabs[0].session;
        let session_b = app.tabs[1].session;
        app.ui.get_mut(&session_a).expect("A ui").operation_history = Default::default();
        app.ui.get_mut(&session_b).expect("B ui").operation_history = Default::default();
        app.switch_repo(0, cx);
        app.open_cherry_pick_modal(CommitId(first));
        (session_a, session_b)
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            app.cherry_pick_modal()
                .is_some_and(|modal| modal.plan.blockers.is_empty()),
            "background-history-fixture: first cherry-pick plan is blocked"
        );
        app.start_cherry_pick(cx);
        app.switch_repo(1, cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            app.ui[&session_b]
                .operation_history
                .peek_undo()
                .is_none_or(|entry| entry.kind != OperationKind::CherryPick),
            "background-history-does-not-leak-to-active: B received A's operation"
        );
        assert!(
            app.ui[&session_a]
                .operation_history
                .peek_undo()
                .is_some_and(|entry| entry.kind == OperationKind::CherryPick),
            "background-history-records-frozen-owner: A lost its departed completion"
        );
        app.switch_repo(0, cx);
        assert!(
            app.ui().operation_history.peek_undo().is_some(),
            "background-history-restores-on-return: A cannot undo its completed operation"
        );
        app.open_cherry_pick_modal(CommitId(second));
    });
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            app.cherry_pick_modal()
                .is_some_and(|modal| modal.plan.blockers.is_empty()),
            "detached-history-fixture: second cherry-pick plan is blocked"
        );
        app.start_cherry_pick(cx);
        app.switch_repo(1, cx);
        let a_index = app
            .tabs
            .iter()
            .position(|tab| tab.session == session_a)
            .expect("A tab");
        app.close_tab(a_index, cx);
    });
    cx.run_until_parked();
    app.update(cx, |app, _| {
        assert!(
            !app.ui.contains_key(&session_a),
            "detached-history-drops-owner: closed A retained UI history"
        );
        assert!(
            app.ui[&session_b]
                .operation_history
                .peek_undo()
                .is_none_or(|entry| entry.kind != OperationKind::CherryPick),
            "detached-history-does-not-fall-through: closed A wrote B's history"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS background_operation_history_owner");
}

pub fn scenario_welcome_drops_root_main_diff(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.run_until_parked();
    let weak = app.update(cx, |app, cx| {
        app.select_headless(0);
        app.open_main_diff_commit(0, cx);
        let weak = app
            .ui()
            .main_diff
            .as_ref()
            .expect("main diff opened")
            .downgrade();
        app.close_tab(0, cx);
        assert!(app.tabs.is_empty(), "Welcome still has a tab");
        assert!(
            app.ui().main_diff.is_none(),
            "welcome-exposes-no-main-diff: closed owner remained active"
        );
        weak
    });
    cx.run_until_parked();
    assert!(
        weak.upgrade().is_none(),
        "welcome-drops-root-main-diff-entity: MainDiffPane remained alive"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS welcome_drops_root_main_diff");
}
