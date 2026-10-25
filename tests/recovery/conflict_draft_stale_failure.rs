//! Failed draft restores cannot erase a retained live Result owner (#1732).
use super::*;
use crate::conflict_draft_revision::{conflict_read, result_input};
use kagi::ui::e2e::{detect_payload_for_test, ConflictDetectOutcome};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const UNSAVED: &str = "NEWER RESULT; AUTOSAVE FAILED\n";

fn blocked_restore(repo: &Path) -> (PathBuf, Vec<u8>, std::fs::File) {
    let store = kagi_git::Backend::open(repo)
        .unwrap()
        .resolution_buffer_from_repo()
        .unwrap()
        .autosave()
        .unwrap();
    let good = std::fs::read(&store).unwrap();
    std::fs::remove_file(store.with_file_name("buffer.json.prev")).unwrap();
    std::fs::write(&store, b"{\"files\":[torn old draft").unwrap();
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    (store, good, lock)
}

fn failed_restore(repo: &Path) -> (ConflictDetectOutcome, PathBuf) {
    let (store, good, lock) = blocked_restore(repo);
    let payload = detect_payload_for_test(repo, "side");
    assert!(matches!(
        payload,
        ConflictDetectOutcome::OpenFailed(Some(_))
    ));
    drop(lock);
    std::fs::write(&store, good).unwrap();
    (payload, store)
}

pub fn scenario_conflict_draft_stale_restore_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    let _isolated = IsolatedLog::new();
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let owner = cx.read(|cx| {
        let state = app.read(cx);
        state
            .app_sessions
            .attachment(state.active_session().unwrap())
            .unwrap()
    });
    let (old, store) = failed_restore(&repo);
    let old = match old {
        ConflictDetectOutcome::OpenFailed(Some(error)) => error,
        _ => unreachable!(),
    };
    let old_successes = [
        detect_payload_for_test(&repo, "main"),
        detect_payload_for_test(&repo, "main"),
    ];
    assert!(old_successes
        .iter()
        .all(|outcome| matches!(outcome, ConflictDetectOutcome::Detected(_))));
    let (_, old_revision) = advance_first(cx, &app, window, &fixture, false);
    let input = result_input(cx, &app, window);
    let view = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    let revision = cx.read(|cx| {
        conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .revision
            .clone()
    });
    assert_ne!(old_revision, revision, "a newer conflict was accepted");
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(UNSAVED, window, cx));
        app.update(cx, |_, cx| cx.notify());
    })
    .unwrap();
    paint(cx, &app, window);
    cx.read(|cx| {
        let buffer = &conflict_read(&app, cx).mode.as_ref().unwrap().buffer;
        assert!(buffer.queue_autosave().unwrap().wait().is_err());
        assert!(buffer.autosave().is_err(), "the live edit is not durable");
        assert!(buffer.has_unflushed_drafts());
    });
    let before = RepositoryState::read(&repo);
    let settled = cx.read(|cx| app.read(cx).ui().pane_revalidation);
    for (awaiting, old_success) in [false, true].into_iter().zip(old_successes) {
        let gate = app.update(cx, |app, _| {
            if awaiting {
                e2e::await_conflict_revalidation(app);
            }
            app.ui().pane_revalidation
        });
        app.update(cx, |app, cx| {
            app.apply_conflict_detect(
                owner.clone(),
                ConflictDetectOutcome::OpenFailed(Some(old.clone())),
                cx,
            );
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().conflict.as_ref().map(|view| view.entity_id()),
                Some(view.entity_id()),
                "a superseded restore failure must retain the current pane"
            );
            let retained = conflict_read(&app, cx);
            assert_eq!(retained.mode.as_ref().unwrap().revision, revision);
            assert_eq!(
                retained.editor_inputs.as_ref().unwrap().result.entity_id(),
                input.entity_id()
            );
            assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
            assert_eq!(
                retained
                    .mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new(FILE))
                    .as_deref(),
                Some(UNSAVED)
            );
            assert_eq!(state.ui().pane_revalidation, gate);
            assert!(
                state.app_notice().is_none(),
                "an obsolete failure is silent"
            );
        });
        let (detected, review_needed) = cx.read(|cx| {
            (
                app.read(cx).ui().conflict_detected,
                e2e::conflict_saved_review_needed(view.read(cx)),
            )
        });
        app.update(cx, |app, cx| {
            app.apply_conflict_detect(owner.clone(), old_success, cx)
        });
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().conflict.as_ref().unwrap().entity_id(),
                view.entity_id(),
                "a rejected old success cannot discard a newer live Result"
            );
            let retained = conflict_read(&app, cx);
            assert_eq!(retained.mode.as_ref().unwrap().revision, revision);
            assert_eq!(
                retained.editor_inputs.as_ref().unwrap().result.entity_id(),
                input.entity_id()
            );
            assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
            assert_eq!(
                retained
                    .mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new(FILE))
                    .as_deref(),
                Some(UNSAVED)
            );
            assert!(retained
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .has_unflushed_drafts());
            assert_eq!(state.ui().pane_revalidation, gate);
            assert_eq!(state.ui().conflict_detected, detected);
            assert_eq!(e2e::conflict_saved_review_needed(&retained), review_needed);
            assert!(state.app_notice().is_none());
        });
    }
    assert_eq!(before, RepositoryState::read(&repo));
    app.update(cx, |app, _| {
        app.ui_mut().unwrap().pane_revalidation = settled
    });
    drop(lock);
    retry_autosave(cx, &app);
    drop(input);
    drop(view);
    unmount(cx, app, window);
}

fn retry_autosave(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        let buffer = &conflict_read(app, cx).mode.as_ref().unwrap().buffer;
        buffer.queue_autosave().unwrap().wait().unwrap();
        assert!(!buffer.has_unflushed_drafts());
    });
}

struct DetectionHold {
    release: Option<crate::evidence_support::Reply<()>>,
    reached: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl DetectionHold {
    fn new(cx: &mut VisualTestAppContext, expect_restore_error: bool) -> Self {
        let (hold, release) = crate::evidence_support::deferred(cx);
        Self {
            release: Some(release),
            reached: KagiApp::hold_next_conflict_detection_for_e2e(hold, expect_restore_error),
        }
    }

    fn wait(&self, cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
        crate::busy_dismiss::wait(
            cx,
            app,
            "expected real detector result reaches admission",
            |_| self.reached.load(std::sync::atomic::Ordering::Acquire),
        );
    }
}

impl Drop for DetectionHold {
    fn drop(&mut self) {
        KagiApp::clear_conflict_detection_hold_for_e2e();
        if let Some(release) = self.release.take() {
            release.send(());
        }
    }
}

pub fn scenario_conflict_draft_stale_restore_async(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for superseding in ["read", "detection"] {
        let _isolated = IsolatedLog::new();
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
        let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner));
        let (store, good, lock) = blocked_restore(&repo);
        let hold = DetectionHold::new(cx, true);
        app.update(cx, |app, cx| {
            app.ui_mut().unwrap().conflict_detected = false;
            app.detect_conflict_mode_async(cx);
        });
        hold.wait(cx, &app);
        drop(lock);
        std::fs::write(&store, good).unwrap();

        app.update(cx, |app, cx| {
            if superseding == "read" {
                // Accept an identical observation through the production Reads
                // owner without starting another detector: only ReadKey differs.
                let view = app.view().clone();
                app.reads.publish(owner, view);
                assert_ne!(app.reads.current_key(owner), read_key);
            } else {
                // A real newer detection, with no read revision change: only
                // the detector generation distinguishes this from the held error.
                app.ui_mut().unwrap().conflict_detected = false;
                app.detect_conflict_mode(cx);
                assert_eq!(app.reads.current_key(owner), read_key);
            }
        });
        let input = result_input(cx, &app, window);
        let view = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
        let revision = cx.read(|cx| view.read(cx).mode.as_ref().unwrap().revision.clone());
        let lock = kagi_git::advisory_lock::lock_exclusive(
            &store.with_file_name("buffer.json.lock"),
            Duration::ZERO,
        )
        .unwrap();
        cx.update_window(window, |_, window, cx| {
            input.update(cx, |input, cx| input.set_value(UNSAVED, window, cx));
            app.update(cx, |_, cx| cx.notify());
        })
        .unwrap();
        paint(cx, &app, window);
        cx.read(|cx| {
            let buffer = &conflict_read(&app, cx).mode.as_ref().unwrap().buffer;
            assert!(buffer.queue_autosave().unwrap().wait().is_err());
            assert!(buffer.has_unflushed_drafts());
        });
        let before = RepositoryState::read(&repo);
        let settled = cx.read(|cx| app.read(cx).ui().pane_revalidation);
        app.update(cx, |app, _| e2e::await_conflict_revalidation(app));
        let awaiting = cx.read(|cx| app.read(cx).ui().pane_revalidation);
        drop(hold);
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().conflict.as_ref().map(|view| view.entity_id()),
                Some(view.entity_id()),
                "superseded {superseding}: same-observation error cannot erase Result"
            );
            let retained = conflict_read(&app, cx);
            assert_eq!(retained.mode.as_ref().unwrap().revision, revision);
            assert_eq!(
                retained.editor_inputs.as_ref().unwrap().result.entity_id(),
                input.entity_id()
            );
            assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
            assert_eq!(
                retained
                    .mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new(FILE))
                    .as_deref(),
                Some(UNSAVED)
            );
            assert!(retained
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .has_unflushed_drafts());
            assert_eq!(state.ui().pane_revalidation, awaiting);
            assert!(state.app_notice().is_none(), "no obsolete failure notice");
        });
        assert_eq!(before, RepositoryState::read(&repo));
        app.update(cx, |app, _| {
            app.ui_mut().unwrap().pane_revalidation = settled
        });
        drop(lock);
        retry_autosave(cx, &app);
        drop(input);
        drop(view);
        unmount(cx, app, window);
    }
}

pub fn scenario_conflict_draft_fresh_restore_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        for reload in ["quiet", "manual", "external"] {
            let _isolated = IsolatedLog::new();
            let fixture = content_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            detect(cx, &app, window);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                app.set_window_active_for_e2e(true, cx);
            });
            let input = result_input(cx, &app, window);
            let view = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
            let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
            let revision = cx.read(|cx| view.read(cx).mode.as_ref().unwrap().revision.clone());
            let store = cx.read(|cx| {
                view.read(cx)
                    .mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .autosave()
                    .unwrap()
            });
            let primary = std::fs::read(&store).unwrap();
            let lock = kagi_git::advisory_lock::lock_exclusive(
                &store.with_file_name("buffer.json.lock"),
                Duration::ZERO,
            )
            .unwrap();
            cx.update_window(window, |_, window, cx| {
                input.update(cx, |input, cx| input.set_value(UNSAVED, window, cx));
                app.update(cx, |_, cx| cx.notify());
            })
            .unwrap();
            paint(cx, &app, window);
            cx.read(|cx| {
                let buffer = &view.read(cx).mode.as_ref().unwrap().buffer;
                assert_eq!(
                    buffer.resolved_text(Path::new(FILE)).as_deref(),
                    Some(UNSAVED)
                );
                assert!(buffer.queue_autosave().unwrap().wait().is_err());
                assert!(buffer.has_unflushed_drafts());
            });
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
            assert_eq!(
                std::fs::read(&store).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
            drop(lock);
            crate::busy_dismiss::wait_ui(
                cx,
                &app,
                "queued autosave observation settles",
                |_, cx| view.read(cx).draft_autosave_settled_for_e2e(),
            );
            // A failed queued edit has its own mandatory #1731 presentation.
            // Reload must add exactly its restore receipt, not duplicate that
            // already-observed autosave error or erase its notification.
            let setup_receipts = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
            assert!(setup_receipts.iter().all(|entry| {
                entry.op == "conflict-draft-autosave"
                    && matches!(entry.outcome, kagi_git::oplog::OpOutcome::Failed { .. })
            }));
            let setup_toasts = cx.read(|cx| {
                app.read(cx)
                    .toast_stack
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .toasts()
                    .iter()
                    .map(|toast| toast.id)
                    .collect::<Vec<_>>()
            });
            let before = RepositoryState::read(&repo);
            let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner));
            // Exercise the ordinary production reload, not a forged failure payload.
            app.update(cx, |app, cx| match reload {
                "quiet" => app.reload(cx),
                "manual" => app.reload_manual(cx),
                "external" => app.reload_external(cx),
                _ => unreachable!(),
            });
            crate::busy_dismiss::wait(cx, &app, "fresh unreadable draft reload", |state| {
                state.reads.current_key(owner) != read_key && !state.reads.is_loading(owner)
            });
            paint(cx, &app, window);
            cx.read(|cx| {
            let state = app.read(cx);
            assert_eq!(
                state.ui().conflict.as_ref().map(|entity| entity.entity_id()),
                Some(view.entity_id()),
                "a fresh restore failure must retain the current pane"
            );
            let retained = view.read(cx);
            let mode = retained.mode.as_ref().unwrap();
            assert_eq!(mode.revision, revision);
            assert_eq!(
                retained.editor_inputs.as_ref().unwrap().result.entity_id(),
                input.entity_id()
            );
            assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
            assert_eq!(mode.buffer.resolved_text(Path::new(FILE)).as_deref(), Some(UNSAVED));
            assert!(mode.buffer.has_unflushed_drafts());
            assert!(state.ui().panes_revalidating(), "failed checked read stays closed");
            assert!(state.ui().conflict_detected, "no per-frame restore retries");
            assert!(state.ui().conflict_restore_failed);
            assert!(state.app_notice().is_none(), "recorded failure is not a modal");
            let preview = kagi_ui_core::i18n::Msg::conflict_draft_restore_failed();
            assert!(matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message) if message == &preview));
            assert_eq!(
                state.toast_stack.as_ref().unwrap().read(cx).toasts()
                    .iter().map(|toast| toast.id).collect::<Vec<_>>(),
                setup_toasts,
                "restore failure does not duplicate the observed autosave notice"
            );
            assert!(!preview.contains('/') && preview.chars().count() < 100);
        });
            let receipts = kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100);
            assert_eq!(receipts.len(), setup_receipts.len() + 1);
            let receipt = receipts.first().unwrap();
            assert_eq!(receipt.op, "conflict-draft-restore");
            assert!(
                matches!(&receipt.outcome, kagi_git::oplog::OpOutcome::Failed { error }
            if error.contains(store.to_str().unwrap()) && error.contains("Permission denied"))
            );
            let expected = before.with_appended_oplog(receipt);
            assert_eq!(expected, RepositoryState::read(&repo));
            click_control(cx, window, "conflict-save");
            cx.run_until_parked();
            assert_eq!(
                expected,
                RepositoryState::read(&repo),
                "Save cannot dispatch a write from the failed checked read"
            );
            assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), UNSAVED);
            std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(std::fs::read(&store).unwrap(), primary);
            retry_autosave(cx, &app);
            let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner));
            app.update(cx, |app, cx| match reload {
                "quiet" => app.reload(cx),
                "manual" => app.reload_manual(cx),
                "external" => app.reload_external(cx),
                _ => unreachable!(),
            });
            crate::busy_dismiss::wait(cx, &app, "successful draft revalidation", |state| {
                state.reads.current_key(owner) != read_key
                    && !state.reads.is_loading(owner)
                    && !state.ui().panes_revalidating()
            });
            assert_eq!(
                cx.read(|cx| app.read(cx).ui().conflict.as_ref().unwrap().entity_id()),
                view.entity_id()
            );
            cx.read(|cx| {
                assert!(!app.read(cx).ui().conflict_restore_failed);
                assert_eq!(
                    view.read(cx)
                        .editor_inputs
                        .as_ref()
                        .unwrap()
                        .result
                        .entity_id(),
                    input.entity_id()
                );
            });
            assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), UNSAVED);
            drop(input);
            drop(view);
            unmount(cx, app, window);
        }
    }
}

pub fn scenario_conflict_draft_stale_restore_success(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for (superseding, language, rescue) in [
        ("read", Lang::En, false),
        ("detection", Lang::Ja, false),
        ("read", Lang::Ja, true),
        ("detection", Lang::En, true),
    ] {
        check_stale_success(cx, superseding, language, rescue);
    }
}

fn check_stale_success(
    cx: &mut VisualTestAppContext,
    superseding: &str,
    language: Lang,
    rescue: bool,
) {
    let _isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_window_active_for_e2e(true, cx);
    });
    let input = result_input(cx, &app, window);
    let view = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    let owner = cx.read(|cx| {
        app.read(cx)
            .app_sessions
            .attachment(app.read(cx).active_session().unwrap())
            .unwrap()
    });
    let revision = cx.read(|cx| view.read(cx).mode.as_ref().unwrap().revision.clone());
    let store = cx.read(|cx| {
        view.read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    let good = std::fs::read(&store).unwrap();
    if rescue {
        let previous = store.with_file_name("buffer.json.prev");
        if previous.exists() {
            std::fs::remove_file(previous).unwrap();
        }
        std::fs::write(
            &store,
            b"{torn document rescued by older successful request",
        )
        .unwrap();
    }
    let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner.session));
    let hold = DetectionHold::new(cx, false);
    app.update(cx, |app, cx| {
        app.ui_mut().unwrap().conflict_detected = false;
        app.detect_conflict_mode_async(cx);
    });
    hold.wait(cx, &app);
    let generation = cx.read(|cx| e2e::conflict_detector_generation(app.read(cx)));
    if rescue {
        assert!(store.with_file_name("buffer.json.corrupt").is_file());
        std::fs::write(&store, &good).unwrap();
    }
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |input, cx| input.set_value(UNSAVED, window, cx));
        app.update(cx, |_, cx| cx.notify());
    })
    .unwrap();
    paint(cx, &app, window);
    cx.read(|cx| {
        assert!(view
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .queue_autosave()
            .unwrap()
            .wait()
            .is_err());
    });
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
    drop(lock);
    if superseding == "read" {
        let failure = detect_payload_for_test(&repo, "main");
        assert!(matches!(
            failure,
            ConflictDetectOutcome::OpenFailed(Some(_))
        ));
        app.update(cx, |app, cx| {
            let read = app.view().clone();
            app.reads.publish(owner.session, read);
            assert_ne!(app.reads.current_key(owner.session), read_key);
            assert_eq!(e2e::conflict_detector_generation(app), generation);
            app.apply_conflict_detect(owner.clone(), failure, cx);
        });
    } else {
        app.update(cx, |app, cx| {
            app.ui_mut().unwrap().conflict_detected = false;
            app.detect_conflict_mode(cx);
            assert_eq!(app.reads.current_key(owner.session), read_key);
            assert_ne!(e2e::conflict_detector_generation(app), generation);
        });
    }
    let assert_closed = |cx: &gpui::App| {
        let state = app.read(cx);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        let retained = view.read(cx);
        let mode = retained.mode.as_ref().unwrap();
        assert_eq!(mode.revision, revision);
        assert_eq!(
            retained.editor_inputs.as_ref().unwrap().result.entity_id(),
            input.entity_id()
        );
        assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
        assert_eq!(
            mode.buffer.resolved_text(Path::new(FILE)).as_deref(),
            Some(UNSAVED)
        );
        assert!(mode.buffer.has_unflushed_drafts());
        assert!(
            state.ui().panes_revalidating(),
            "stale {superseding} success cannot acknowledge newer failed restore"
        );
        assert!(state.ui().conflict_restore_failed);
        assert!(state.ui().conflict_detected);
        assert!(state.conflict_continue_modal().is_none());
        assert!(!state.app_sessions.has_leases());
        let preview = kagi_ui_core::i18n::Msg::conflict_draft_restore_failed();
        assert!(
            matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message) if message == &preview)
        );
        assert!(state.app_notice().is_none());
    };
    cx.read(&assert_closed);
    let before = RepositoryState::read(&repo);
    drop(hold);
    cx.run_until_parked();
    paint(cx, &app, window);
    cx.read(&assert_closed);
    cx.read(|cx| {
        let state = app.read(cx);
        let expected = kagi_ui_core::i18n::Msg::conflict_draft_rescued("buffer.json.corrupt");
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert_eq!(
            toasts
                .iter()
                .filter(|toast| toast.message.as_ref() == expected)
                .count(),
            usize::from(rescue),
            "a superseded success must still announce its completed rescue"
        );
    });
    click_control(cx, window, "conflict-save");
    click_control(cx, window, "conflict-continue");
    cx.run_until_parked();
    cx.read(&assert_closed);
    assert_eq!(
        before,
        RepositoryState::read(&repo),
        "stale success cannot admit Save or Continue or append another receipt"
    );
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    retry_autosave(cx, &app);
    app.update(cx, |app, cx| {
        app.ui_mut().unwrap().conflict_detected = false;
        app.detect_conflict_mode(cx);
    });
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(!state.ui().panes_revalidating());
        assert!(!state.ui().conflict_restore_failed);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        assert_eq!(
            view.read(cx)
                .editor_inputs
                .as_ref()
                .unwrap()
                .result
                .entity_id(),
            input.entity_id()
        );
        assert_eq!(input.read(cx).value().as_ref(), UNSAVED);
    });
    drop(input);
    drop(view);
    unmount(cx, app, window);
}
