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
    if !active && !recording_failure {
        // First-open failure has no conflict pane: the repository Graph owns
        // the notification's content column, not the window or Inspector.
        let anchor = e2e::control_bounds(window.window_id(), "toast-content-anchor")
            .expect("repository content anchor");
        let stack = e2e::control_bounds(window.window_id(), "toast-stack")
            .expect("persistent recovery stack");
        assert!(stack.origin.x >= anchor.origin.x && stack.origin.y >= anchor.origin.y);
        assert!(stack.origin.x + stack.size.width <= anchor.origin.x + anchor.size.width);
        assert!(stack.origin.y + stack.size.height <= anchor.origin.y + anchor.size.height);
        assert!(stack.size.width <= anchor.size.width * 0.45);
        let inset = kagi::ui::theme::scaled_px(12.);
        assert!((stack.left() - anchor.left() - inset).abs() <= gpui::px(2.));
        assert!((anchor.bottom() - stack.bottom() - inset).abs() <= gpui::px(2.));
    }
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
                        "{preview}\n{}",
                        kagi_ui_core::i18n::Msg::conflict_draft_repository(
                            repo.file_name().unwrap().to_str().unwrap()
                        )
                    )
                );
                assert!(!toast.message.contains('/') && !toast.message.contains(&cause));
                assert!(toast
                    .refresh_owner
                    .as_ref()
                    .is_some_and(|owner| owner.path == repo));
                assert!(matches!(
                    &toast.action,
                    Some(kagi::ui::types::ToastAction::OpenOperationLog(_))
                ));
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
    if failure == "unreadable" && !active && !recording_failure {
        check_refresh_recovery(cx, &app, window, &repo, &store, expected, &receipts[0]);
    }
    drop(lock.take());
    unmount(cx, app, window);
}

fn check_refresh_recovery(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    store: &Path,
    before: RepositoryState,
    receipt: &kagi_git::oplog::OpLogEntry,
) {
    // Retry the same durable card by its button (EN) and ordinary reads (JA).
    // The latter must recover without ever activating a card action.
    std::fs::set_permissions(store, std::fs::Permissions::from_mode(0)).unwrap();
    let other = content_fixture();
    let other_repo = other.path().canonicalize().unwrap();
    app.update(cx, |app, cx| {
        assert!(app.open_repository(other_repo.clone(), cx))
    });
    crate::busy_dismiss::ready(cx, app);
    let other_before = RepositoryState::read(&other_repo);
    assert!(
        cx.read(|cx| app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| toast
                .refresh_owner
                .as_ref()
                .is_some_and(|owner| owner.path == repo)
                && toast.dismissing.is_none())),
        "an accepted read for another repository retains the failure"
    );
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    paint(cx, app, window);
    let card_refresh = Lang::En == kagi_ui_core::i18n::lang();
    if card_refresh {
        click_control(cx, window, "operation-result-refresh");
    } else {
        app.update(cx, |app, cx| {
            assert!(app.open_repository(repo.to_path_buf(), cx));
            app.reload_manual(cx);
        });
    }
    wait_refresh(cx, app, window, |app, cx| {
        app.ui().conflict_restore_failed
            && read_oplog_tail_for_repo(repo, 100).len() >= 2
            && app
                .toast_stack
                .as_ref()
                .unwrap()
                .read(cx)
                .toasts()
                .iter()
                .any(|toast| {
                    toast
                        .refresh_owner
                        .as_ref()
                        .is_some_and(|owner| owner.path == repo)
                        && toast.dismissing.is_none()
                })
    });
    assert_eq!(
        cx.read(|cx| app.read(cx).repo_path.clone()).as_deref(),
        Some(repo)
    );
    let retry_receipts = read_oplog_tail_for_repo(repo, 100);
    let retry = retry_receipts
        .iter()
        .find(|entry| entry.op == "conflict-draft-restore" && entry.id != receipt.id)
        .unwrap();
    // Both repositories share this isolated audit file: only the captured
    // owner's failed-read receipt may be appended, with no Git change to either.
    let other_before = other_before.with_appended_oplog(retry);
    assert_eq!(other_before, RepositoryState::read(&other_repo));
    std::fs::set_permissions(store, std::fs::Permissions::from_mode(0o600)).unwrap();
    let expected = before.with_appended_oplog(retry);
    assert_eq!(expected, RepositoryState::read(repo));
    paint(cx, app, window);
    if card_refresh {
        click_control(cx, window, "operation-result-refresh");
    } else {
        app.update(cx, |app, cx| app.reload_external(cx));
    }
    app.update(cx, |app, cx| {
        let stack = app.toast_stack.clone().unwrap();
        stack.update(cx, |stack, cx| {
            stack.push_notify(kagi::ui::ToastKind::Error, "Unrelated persistent error", cx);
        });
    });
    wait_refresh(cx, app, window, |app, cx| {
        let toasts = app.toast_stack.as_ref().unwrap().read(cx).toasts();
        !app.ui().conflict_restore_failed
            && app.ui().conflict.is_some()
            && toasts
                .iter()
                .filter(|toast| toast.is_persistent() && toast.dismissing.is_none())
                .count()
                == 1
            && toasts.iter().any(|toast| {
                toast.message.as_ref() == "Unrelated persistent error" && toast.dismissing.is_none()
            })
    });
    // Manual refresh also shows its normal transient success. It may remain
    // hovered; only the matching failure must leave, not unrelated feedback.
    cx.advance_clock(std::time::Duration::from_secs(1));
    paint(cx, app, window);
    cx.read(|cx| {
        assert!(!app
            .read(cx)
            .toast_stack
            .as_ref()
            .unwrap()
            .read(cx)
            .toasts()
            .iter()
            .any(|toast| {
                toast
                    .refresh_owner
                    .as_ref()
                    .is_some_and(|owner| owner.path == repo)
            }));
    });
    assert_eq!(expected, RepositoryState::read(repo));
    assert_eq!(other_before, RepositoryState::read(&other_repo));
    assert!(read_oplog_tail_for_repo(repo, 100)
        .iter()
        .any(|entry| entry.id == receipt.id
            && entry.op == receipt.op
            && entry.timestamp == receipt.timestamp));
}

fn wait_refresh(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    ready: impl Fn(&KagiApp, &gpui::App) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        paint(cx, app, window);
        if cx.read(|cx| ready(app.read(cx), cx)) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "refresh recovery did not settle: {:?}",
            cx.read(|cx| {
                let app = app.read(cx);
                (
                    app.ui().conflict_restore_failed,
                    app.ui().conflict.is_some(),
                    app.toast_stack
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .toasts()
                        .iter()
                        .map(|toast| {
                            (
                                toast.message.clone(),
                                toast.dismissing.is_some(),
                                toast.refresh_owner.clone(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
