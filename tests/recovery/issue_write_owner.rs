//! Issue-write failures remain durable without opening a modal after a tab switch.

use gpui::VisualTestAppContext;

use crate::macos::{build_fixture, mount, repo_fingerprint, unmount};
use crate::recovery_operations::wait_idle;

pub fn scenario_issue_failure_notice_survives_tab_switch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let other = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let repo_before = repo_fingerprint(&repo);
    let other_before = repo_fingerprint(other.path());
    let (app, window) = mount(cx, &repo);

    app.update(cx, |app, cx| {
        assert!(app.open_repository(other.path().to_path_buf(), cx));
        app.switch_repo(0, cx);
    });
    cx.run_until_parked();

    // Start from A and leave before its synthetic recorded terminal can land.
    // B already has a modal by then. The recorded failure must not take over
    // the visible slot or add a dismiss-only notice behind it.
    app.update(cx, |app, cx| {
        assert!(app.start_failed_issue_create_for_e2e(repo.clone(), cx));
        app.switch_repo(1, cx);
        kagi::ui::e2e::deliver_app_notice(app, "tab B foreground notice");
    });
    wait_idle(cx, &app);

    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_tab, 1);
        assert_eq!(
            kagi::ui::e2e::app_notice_message(state),
            Some("tab B foreground notice"),
            "the departed owner's failure must not replace B's modal"
        );
        assert!(!kagi::ui::e2e::queued_notice_contains(
            state,
            "the server refused the issue"
        ));
        let panel = state.op_log.as_ref().unwrap().read(cx);
        assert!(panel.entries().iter().any(|entry| {
            entry.op == "issue-create"
                && entry.repo == repo.display().to_string()
                && matches!(entry.outcome, kagi_git::oplog::OpOutcome::Failed { .. })
        }));
        assert!(state.toast_stack.as_ref().is_some_and(|stack| stack
            .read(cx)
            .toasts()
            .iter()
            .any(
                |toast| toast.message.contains("the server refused the issue")
                    && toast.message.contains(&repo.display().to_string())
            )));
    });

    app.update(cx, |app, _| {
        app.clear_app_notice();
        kagi::ui::e2e::present_app_notice(app);
    });
    cx.read(|cx| {
        assert!(app.read(cx).app_notice().is_none());
    });

    // Current-tab delivery still owns the old path; this stale completion was
    // enqueued exactly once, not once before and once after the owner guard.
    app.update(cx, |app, _| {
        app.clear_app_notice();
        kagi::ui::e2e::present_app_notice(app);
    });
    cx.read(|cx| {
        assert!(app.read(cx).app_notice().is_none());
    });

    assert_eq!(repo_fingerprint(&repo), repo_before, "repo A mutated");
    assert_eq!(
        repo_fingerprint(other.path()),
        other_before,
        "repo B mutated"
    );
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS issue_failure_notice_survives_tab_switch: owner failure remains in Operation Log without a modal"
    );
}
