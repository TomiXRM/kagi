//! A failed, superseded restore cannot erase a newer Result owner (#1732).
use super::*;
use crate::conflict_draft_revision::{conflict_read, result_input};
use kagi::ui::e2e::{detect_payload_for_test, ConflictDetectOutcome};
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
    for awaiting in [false, true] {
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
    fn new(cx: &mut VisualTestAppContext) -> Self {
        let (hold, release) = crate::evidence_support::deferred(cx);
        Self {
            release: Some(release),
            reached: KagiApp::hold_next_conflict_detection_for_e2e(hold),
        }
    }

    fn wait(&self, cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
        crate::busy_dismiss::wait(cx, app, "real restore error reaches admission", |_| {
            self.reached.load(std::sync::atomic::Ordering::Acquire)
        });
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
        let hold = DetectionHold::new(cx);
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
