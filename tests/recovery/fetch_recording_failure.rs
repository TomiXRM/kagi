//! #1698: a real successful Fetch cannot out-announce its failed receipt.
use crate::busy_dismiss::{advance, ready, wait};
use crate::evidence_support::deferred;
use crate::fetch_owner::local_remote_with_change;
use crate::macos::{build_fixture, mount, unmount};
use crate::pull_freshness::SettingsGuard;
use gpui::VisualTestAppContext;
use kagi::ui::{
    i18n::{self, Lang},
    FooterStatus, KagiApp, ToastKind,
};
use kagi_git::oplog::{self, OpOutcome};

#[path = "../support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output;

pub fn scenario_fetch_changed_ref_recording_failure(cx: &mut VisualTestAppContext) {
    let _settings = SettingsGuard::install();
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    let append_diagnostic = std::fs::TryLockError::WouldBlock.to_string();
    for language in [Lang::En, Lang::Ja] {
        // The appended control must retain its ordinary result. The departed
        // failed-receipt case must not clear another repository's footer.
        for (recording_failure, departed) in [(false, false), (true, false), (true, true)] {
            let fixture = build_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let recorded_repo = repo.display().to_string();
            let remote = tempfile::tempdir().unwrap();
            local_remote_with_change(&repo, remote.path());
            let expected = git_output(&remote.path().join("other"), &["rev-parse", "HEAD"]);
            let head_before = git_output(&repo, &["rev-parse", "HEAD"]);
            let index_before = git_output(&repo, &["ls-files", "--stage"]);
            let status_before = git_output(&repo, &["status", "--porcelain"]);
            assert_ne!(
                git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
                expected
            );
            let durable_before = oplog::read_oplog_tail_for_repo(&repo, 100).len();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                // Explicit app-level active-surface fixture, not an OS
                // activation claim. Ordinary Tier B exercises the owned key window.
                app.set_window_active_for_e2e(true, cx);
            });
            ready(cx, &app);
            let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
            let (hold, release) = deferred::<()>(cx);
            KagiApp::hold_next_fetch_for_e2e(hold);
            app.update(cx, |app, cx| app.fetch_async(false, cx));
            cx.run_until_parked();
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(state.fetch_in_flight.is_some());
                assert!(
                    state.app_sessions.has_leases(),
                    "actual writer admission precedes the fault"
                );
            });
            let foreign = departed.then(build_fixture);
            if let Some(foreign) = &foreign {
                app.update(cx, |app, cx| {
                    assert!(app.open_repository(foreign.path().canonicalize().unwrap(), cx));
                });
                ready(cx, &app);
                assert_ne!(cx.read(|cx| app.read(cx).active_session()), Some(owner));
            }
            let before_presentation = cx.read(|cx| {
                let state = app.read(cx);
                (
                    state.bottom_panel_open,
                    state.bottom_tab,
                    format!("{:?}", state.status_footer),
                )
            });
            let lock_path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                .join("operations.jsonl.lock");
            let lock = recording_failure.then(|| {
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(&lock_path)
                    .unwrap();
                lock.lock().unwrap();
                lock
            });
            advance(cx, 20);
            release.send(());
            wait(cx, &app, "the real changed-ref Fetch settles", |state| {
                state.fetch_in_flight.is_none() && !state.app_sessions.has_leases()
            });
            assert_eq!(
                git_output(&repo, &["rev-parse", "refs/remotes/origin/main"]),
                expected
            );
            assert_eq!(git_output(&repo, &["rev-parse", "HEAD"]), head_before);
            assert_eq!(git_output(&repo, &["ls-files", "--stage"]), index_before);
            assert_eq!(git_output(&repo, &["status", "--porcelain"]), status_before);
            cx.read(|cx| {
                let state = app.read(cx);
                assert_eq!(
                    (state.bottom_panel_open, state.bottom_tab),
                    (before_presentation.0, before_presentation.1)
                );
                let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                let panel = state.op_log.as_ref().unwrap().read(cx);
                let rows: Vec<_> = panel
                    .entries()
                    .iter()
                    .filter(|entry| entry.op == "fetch" && entry.repo == recorded_repo)
                    .collect();
                assert_eq!(
                    rows.len(),
                    1,
                    "one attempted result, not a forged second receipt"
                );
                if recording_failure {
                    let notice = state
                        .app_notice()
                        .expect("failed append requires acknowledgment");
                    assert!(notice.message.contains(&recorded_repo));
                    assert!(
                        notice.message.contains(&append_diagnostic),
                        "acknowledgment must retain the actual failed append diagnostic"
                    );
                    assert!(
                        toasts.is_empty(),
                        "no green or error result beside required acknowledgment"
                    );
                    if departed {
                        assert_eq!(format!("{:?}", state.status_footer), before_presentation.2);
                    } else {
                        assert!(matches!(&state.status_footer,
                            FooterStatus::Idle(text) if text.is_empty()));
                    }
                    match &rows[0].outcome {
                        OpOutcome::Partial { after, error } => {
                            assert!(
                                error.contains(&append_diagnostic),
                                "the attempted row must retain the append's actual IO cause"
                            );
                            assert_eq!(after.head, rows[0].before.head);
                            assert_eq!(after.dirty, rows[0].before.dirty);
                        }
                        other => panic!(
                            "an unsaved success must remain explicitly incomplete: {other:?}"
                        ),
                    }
                    assert!(
                        rows[0].ref_moves.is_none(),
                        "an unsaved attempt has no restore ref information"
                    );
                } else {
                    assert!(state.app_notice().is_none());
                    assert!(matches!(rows[0].outcome, OpOutcome::Success { .. }));
                    let footer_result = matches!(&state.status_footer,
                        FooterStatus::Success(text) if !text.is_empty());
                    let toast_result = toasts.len() == 1 && toasts[0].kind == ToastKind::Success;
                    assert_ne!(
                        footer_result, toast_result,
                        "the ordinary appended result stays in one place"
                    );
                    assert!(toasts.is_empty() || toast_result);
                }
            });
            drop(lock);
            if recording_failure {
                let _ = std::fs::remove_file(&lock_path);
            }
            advance(cx, 4);
            assert_eq!(
                oplog::read_oplog_tail_for_repo(&repo, 100).len(),
                durable_before + usize::from(!recording_failure),
                "failed recording must not be forged or retried after the lock is released"
            );
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(original_language);
}
