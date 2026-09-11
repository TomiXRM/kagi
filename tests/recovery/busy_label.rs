//! Fetch's real admission/dispatch path supplies the snackbar text before completion.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};

pub fn scenario_fetch_busy_label(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // Local transport only; no network or delayed child process in the runner.
    git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    let (app, window) = mount(cx, &repo);
    let original_language = i18n::lang();
    for (language, expected) in [(Lang::En, "Fetching…"), (Lang::Ja, "fetch 中…")] {
        i18n::set_lang(language);
        app.update(cx, |app, cx| {
            app.fetch_async(false, cx);
            // Assert in the same UI turn before the completion can be delivered.
            // This is the label consumed by render_busy_snackbar, not the footer.
            assert!(app.fetch_in_flight);
            assert_eq!(app.busy_op, Some("fetch"));
            assert_eq!(e2e::busy_snackbar_label(app), Some(expected));
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while cx.read(|cx| app.read(cx).fetch_in_flight) {
            cx.run_until_parked();
            assert!(std::time::Instant::now() < deadline, "fetch did not settle");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        git(
            &repo,
            &["rev-parse", "--verify", "refs/remotes/origin/main"],
        );
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(!state.fetch_in_flight);
            assert!(!state.app_sessions.has_leases());
            assert_eq!(state.busy_op, None);
            assert_eq!(e2e::busy_snackbar_label(state), None);
        });
    }
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS fetch_busy_label: EN/JA fetch snackbar and busy release");
}

/// #646: a failed fetch reaches the oplog, not just the footer.
///
/// `AGENTS.md` requires a user-facing error to surface via the oplog **and** a
/// modal. Fetch had only the footer and a toast, so a failure left nothing to
/// recover or audit from — and a silent auto-fetch left no trace at all.
pub fn scenario_fetch_failure_reaches_the_oplog(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    // A remote that cannot be reached: a local path with no repository in it.
    let missing = repo.join("no-such-remote");
    git(
        &repo,
        &["remote", "add", "origin", missing.to_str().unwrap()],
    );

    let before = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .count();

    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.fetch_async(false, cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cx.read(|cx| app.read(cx).fetch_in_flight) {
        cx.run_until_parked();
        assert!(std::time::Instant::now() < deadline, "fetch did not settle");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let entries: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "fetch")
        .collect();
    assert_eq!(
        entries.len(),
        before + 1,
        "a failed fetch must leave exactly one oplog record (#646)"
    );
    let outcome = &entries.last().expect("the record just written").outcome;
    assert!(
        matches!(
            outcome,
            kagi_git::oplog::OpOutcome::Failed { .. } | kagi_git::oplog::OpOutcome::Unknown { .. }
        ),
        "an unreachable remote is a failure, not a success: {outcome:?}"
    );

    // The visible surface must still be there — the oplog record replaces
    // nothing.
    cx.read(|cx| {
        assert!(
            matches!(
                app.read(cx).status_footer,
                kagi::ui::FooterStatus::Failed(_)
            ),
            "the footer must still report the failure"
        );
    });
    unmount(cx, app, window);
}
