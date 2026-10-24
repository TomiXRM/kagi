//! Restore failures own a durable receipt and one bounded preview (#1732).
use super::*;
use kagi::ui::e2e::{detect_payload_for_test, ConflictDetectOutcome};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::os::unix::fs::PermissionsExt;

pub fn scenario_conflict_draft_restore_io_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        for active in [false, true] {
            for recording_failure in [false, true] {
                for failure in [
                    "unreadable",
                    "lock-directory",
                    "lock-timeout",
                    "reserved-names",
                ] {
                    check_failure(cx, language, active, recording_failure, failure);
                }
            }
        }
    }
}

fn check_failure(
    cx: &mut VisualTestAppContext,
    language: Lang,
    active: bool,
    recording_failure: bool,
    failure: &str,
) {
    let _isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_window_active_for_e2e(active, cx);
    });
    let store = kagi_git::Backend::open(&repo)
        .unwrap()
        .resolution_buffer_from_repo()
        .unwrap()
        .autosave()
        .unwrap();
    // An unreadable newer primary must not be mistaken for a corrupt file and
    // silently replaced by its readable backup. Other cases exercise rescue I/O.
    let primary = if failure == "unreadable" {
        assert!(store.with_file_name("buffer.json.prev").is_file());
        std::fs::read(&store).unwrap()
    } else {
        std::fs::remove_file(store.with_file_name("buffer.json.prev")).unwrap();
        let torn = b"{\"files\":[damaged legacy draft";
        std::fs::write(&store, torn).unwrap();
        torn.to_vec()
    };
    let lock_path = store.with_file_name("buffer.json.lock");
    let mut lock = None;
    match failure {
        "unreadable" => {
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap()
        }
        "lock-directory" => {
            std::fs::remove_file(&lock_path).unwrap();
            std::fs::create_dir(&lock_path).unwrap();
        }
        "lock-timeout" => {
            lock = Some(
                kagi_git::advisory_lock::lock_exclusive(&lock_path, std::time::Duration::ZERO)
                    .unwrap(),
            )
        }
        "reserved-names" => {
            for index in 0..100 {
                let name = if index == 0 {
                    "buffer.json.corrupt".into()
                } else {
                    format!("buffer.json.corrupt.{index}")
                };
                std::fs::write(store.with_file_name(name), b"already preserved").unwrap();
            }
        }
        _ => unreachable!(),
    }
    let before = RepositoryState::read(&repo);
    let payload = detect_payload_for_test(&repo, "main");
    let cause = match &payload {
        ConflictDetectOutcome::OpenFailed(Some(failure)) => failure.cause.clone(),
        _ => panic!("restore failure was lost: {failure}"),
    };
    assert!(cause.contains(store.to_str().unwrap()));
    let owner = cx.read(|cx| {
        let state = app.read(cx);
        state
            .app_sessions
            .attachment(state.active_session().unwrap())
            .unwrap()
    });
    let append_lock = recording_failure.then(|| {
        let path =
            PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("operations.jsonl.lock");
        kagi_git::advisory_lock::lock_exclusive(&path, std::time::Duration::ZERO).unwrap()
    });
    app.update(cx, |app, cx| {
        // These cases cover first-open failure, not a retained live editor.
        app.ui_mut().unwrap().conflict = None;
        app.apply_conflict_detect(owner, payload, cx);
    });
    paint(cx, &app, window);
    let receipts = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-draft-restore")
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), usize::from(!recording_failure));
    if !recording_failure {
        assert!(matches!(&receipts[0].outcome, OpOutcome::Failed { error } if error == &cause));
    }
    cx.read(|cx| {
        let state = app.read(cx);
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert!(state.ui().conflict.is_none());
        assert!(state.ui().conflict_detected, "no per-render retries");
        if recording_failure {
            let notice = state
                .app_notice()
                .expect("failed append requires acknowledgment");
            assert!(notice.message.contains(&cause));
            assert!(notice
                .message
                .contains(&std::fs::TryLockError::WouldBlock.to_string()));
            assert!(
                toasts.is_empty(),
                "unrecorded failure has only its acknowledgment"
            );
            assert!(!matches!(
                state.status_footer,
                kagi::ui::FooterStatus::Failed(_)
            ));
        } else {
            assert!(
                state.app_notice().is_none(),
                "a recorded failure is not a modal"
            );
            let preview = kagi_ui_core::i18n::Msg::conflict_draft_restore_failed();
            let footer = match &state.status_footer {
                kagi::ui::FooterStatus::Failed(message) => Some(message.as_str()),
                _ => None,
            };
            assert_eq!(
                usize::from(footer.is_some()) + toasts.len(),
                1,
                "one result owner"
            );
            if active {
                assert_eq!(footer, Some(preview.as_str()));
                assert!(toasts.is_empty());
            } else {
                assert!(footer.is_none());
                assert_eq!(toasts.len(), 1);
            }
            for toast in toasts {
                assert_eq!(toast.kind, kagi::ui::ToastKind::Error);
                assert_eq!(
                    toast.message.as_ref(),
                    format!(
                        "{preview} — {}",
                        repo.file_name().unwrap().to_str().unwrap()
                    )
                );
                assert!(!toast.message.contains('/') && !toast.message.contains(&cause));
            }
            assert!(preview.chars().count() < 100);
            assert!(!preview.contains('/') && !preview.contains(&cause));
        }
    });
    drop(append_lock);
    app.update(cx, |app, cx| {
        if recording_failure {
            app.confirm_app_notice(cx);
        }
        app.detect_conflict_mode(cx);
    });
    assert!(
        cx.read(|cx| app.read(cx).app_notice().is_none()),
        "run-once guard retains the failure"
    );
    assert_eq!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "conflict-draft-restore")
            .count(),
        usize::from(!recording_failure),
        "a render or notice dismissal never retries persistence",
    );
    if failure == "unreadable" {
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(std::fs::read(&store).unwrap(), primary);
    let expected = if recording_failure {
        before
    } else {
        before.with_appended_oplog(&receipts[0])
    };
    assert_eq!(expected, RepositoryState::read(&repo));
    drop(lock.take());
    unmount(cx, app, window);
}
