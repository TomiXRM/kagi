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
            app.main_diff.is_some(),
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
            app.main_diff.is_none(),
            "activation-hides-stale-open-diff: A's old diff remained visible"
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
        app.ui_mut().operation_history = Default::default();
        app.record_history(
            OperationKind::Commit,
            "main",
            CommitId(before.clone()),
            CommitId(after.clone()),
            "A retained operation",
        );
        app.ui_mut().history_seed_attempted = true;
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
