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

/// #643 A1: when the oplog append fails, the user is told.
///
/// The operation itself already happened, so the append failure was logged as
/// "non-fatal" and nothing reached the screen. But Kagi's promise includes
/// leaving a record to recover and audit from, and the in-memory panel still
/// shows the entry — so nothing looks wrong until the next launch, when it is
/// simply gone.
pub fn scenario_oplog_append_failure_is_visible(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);

    // Hold the oplog's lock so the append cannot take it, the same way the
    // staging-failure test induces this.
    let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
        .join("operations.jsonl.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    app.update(cx, |app, cx| {
        e2e::record_persisted_op(app, &repo, cx);
    });

    cx.read(|cx| {
        let stack = app
            .read(cx)
            .toast_stack
            .clone()
            .expect("the mounted app has a toast stack");
        let said_so = stack
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast.message.contains("oplog") || toast.message.contains("記録"));
        assert!(
            said_so,
            "a failed oplog append must reach the user: they are about to trust \
             a log that is missing this entry (#643 A1)"
        );
    });

    drop(lock);
    let _ = std::fs::remove_file(&lock_path);
    unmount(cx, app, window);
}
