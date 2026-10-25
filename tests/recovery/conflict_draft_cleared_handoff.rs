//! An external operation end cannot discard a Result whose real autosave failed.
use super::*;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_ui_core::i18n::{Lang, Msg};
use std::sync::atomic::Ordering;
use std::time::Duration;

const UNSAVED: &str = "EXTERNAL END; UNSAVED RESULT\n";
const NEWER: &str = "NEWER RESULT WHILE HANDOFF IS HELD\n";

struct Release(Option<crate::evidence_support::Reply<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        KagiApp::clear_draft_handoff_hold_for_e2e();
        if let Some(reply) = self.0.take() {
            reply.send(());
        }
    }
}

pub(super) fn check_cleared_handoff(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        for complete in [false, true] {
            check_end(cx, language, complete);
        }
    }
}

pub(super) fn failed_autosave_baseline(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    view: &Entity<ConflictView>,
    repo: &Path,
) -> usize {
    // Native replacement can queue multiple generations. Observe the latest
    // watcher, not just the first failure receipt, before starting the operation.
    crate::busy_dismiss::wait_ui(cx, app, "failed autosave setup settles", |state, cx| {
        let view = view.read(cx);
        view.draft_autosave_settled_for_e2e()
            && view.draft_persist_failed.is_some()
            && state.toast_stack.as_ref().is_some_and(|stack| {
                stack
                    .read(cx)
                    .toasts()
                    .iter()
                    .any(|toast| toast.kind == kagi::ui::ToastKind::Error)
            })
    });
    let receipts = read_oplog_tail_for_repo(repo, 100);
    assert!(
        !receipts.is_empty(),
        "failed autosave must record its receipt"
    );
    assert!(receipts.iter().all(|entry| {
        entry.op == "conflict-draft-autosave"
            && matches!(&entry.outcome, OpOutcome::Failed { error } if error.contains("lock"))
    }));
    receipts.len()
}

fn check_end(cx: &mut VisualTestAppContext, language: Lang, complete: bool) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let file = Path::new(FILE);
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_window_active_for_e2e(true, cx);
    });
    let view = conflict(cx, &app);
    let input = result_input(cx, &app, window);
    let store = cx.read(|cx| {
        view.read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    replace_result(cx, &app, window, &input, UNSAVED);
    input.update(cx, |input, cx| input.set_selected_range(2..8, cx));
    let previous = cx.read(|cx| {
        let buffer = &view.read(cx).mode.as_ref().unwrap().buffer;
        assert!(buffer.queue_autosave().unwrap().wait().is_err());
        assert!(buffer.has_unflushed_drafts());
        let mut history = buffer.clone();
        assert!(history.undo(file));
        history.resolved_text(file)
    });
    let baseline = failed_autosave_baseline(cx, &app, &view, &repo);
    if complete {
        assert!(!fixture_git::git_succeeds(&repo, &["rebase", "--skip"]));
        fixture_git::git(&repo, &["rebase", "--skip"]);
    } else {
        fixture_git::git(&repo, &["rebase", "--abort"]);
    }
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(cx, &app, "external end retains failed handoff", |state| {
        state.view().operation.is_none() && state.ui().conflict_restore_failed
    });
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), view.entity_id());
        assert!(state.ui().panes_revalidating());
        assert!(state.ui().conflict_detected);
        let retained = view.read(cx);
        assert_eq!(retained.editor_inputs.as_ref().unwrap().result.entity_id(), input.entity_id());
        assert_eq!(input.read(cx).value().as_str(), UNSAVED);
        assert_eq!(input.read(cx).selected_range(), 2..8);
        let buffer = &retained.mode.as_ref().unwrap().buffer;
        assert!(buffer.has_unflushed_drafts());
        assert_eq!(buffer.resolved_text(file).as_deref(), Some(UNSAVED));
        let mut history = buffer.clone();
        assert!(history.undo(file));
        assert_eq!(history.resolved_text(file), previous);
        assert!(history.redo(file));
        assert_eq!(history.resolved_text(file).as_deref(), Some(UNSAVED));
        assert!(state.app_notice().is_none());
        let preview = Msg::conflict_draft_preserve_failed();
        assert!(matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message) if message == &preview));
        assert!(preview.chars().count() < 100 && !preview.contains('/'));
    });
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(receipts.len(), baseline + 1);
    let preservation = receipts
        .iter()
        .filter(|entry| entry.op == "conflict-draft-preserve")
        .collect::<Vec<_>>();
    assert_eq!(preservation.len(), 1);
    assert!(
        matches!(&preservation[0].outcome, OpOutcome::Failed { error }
        if error.contains("lock"))
    );
    let before = RepositoryState::read(&repo);
    click_control(cx, window, "conflict-save");
    paint(cx, &app, window);
    assert_eq!(before, RepositoryState::read(&repo));
    assert_eq!(
        read_oplog_tail_for_repo(&repo, 100).len(),
        baseline + 1,
        "no frame retries"
    );
    drop(lock);
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(cx, &app, "repaired handoff removes pane", |state| {
        state.ui().conflict.is_none() && !state.ui().panes_revalidating()
    });
    drop(input);
    drop(view);
    unmount(cx, app, window);
    let backend = kagi_git::Backend::open_with_policy(
        &repo,
        kagi_git::backend::ExecutionPolicy::human(false),
    )
    .unwrap();
    let restored = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert!(
        restored.recovery_drafts().any(|draft| {
            draft.path == file && draft.result.map(lines_to_text).as_deref() == Some(UNSAVED)
        }),
        "restart inspection must expose the durable expired draft"
    );
}

pub(super) fn check_stale_handoff(cx: &mut VisualTestAppContext) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let view = conflict(cx, &app);
    let input = result_input(cx, &app, window);
    let store = cx.read(|cx| {
        view.read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    replace_result(cx, &app, window, &input, UNSAVED);
    crate::busy_dismiss::wait_ui(cx, &app, "first handoff draft fails autosave", |_, cx| {
        let view = view.read(cx);
        view.draft_autosave_settled_for_e2e() && view.draft_persist_failed.is_some()
    });
    drop(lock);
    // Capture a real successful write, withholding only its UI completion.
    let (hold, reply) = crate::evidence_support::deferred::<()>(cx);
    let release = Release(Some(reply));
    let reached = KagiApp::hold_next_draft_handoff_for_e2e(hold);
    fixture_git::git(&repo, &["rebase", "--abort"]);
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(cx, &app, "durable handoff completion held", |_| {
        reached.load(Ordering::Acquire)
    });
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    replace_result(cx, &app, window, &input, NEWER);
    crate::busy_dismiss::wait_ui(cx, &app, "newer handoff draft fails autosave", |_, cx| {
        let view = view.read(cx);
        view.draft_autosave_settled_for_e2e() && view.draft_persist_failed.is_some()
    });
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(cx, &app, "new generation refuses failed handoff", |state| {
        state.ui().conflict_restore_failed
    });
    drop(release);
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        assert!(state.ui().panes_revalidating());
        assert_eq!(input.read(cx).value().as_str(), NEWER);
        assert_eq!(
            view.read(cx)
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new(FILE))
                .as_deref(),
            Some(NEWER)
        );
    });
    drop(lock);
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(cx, &app, "newest draft handoff succeeds", |state| {
        state.ui().conflict.is_none()
    });
    drop(input);
    drop(view);
    unmount(cx, app, window);
    assert_eq!(
        ResolutionBuffer::load(&repo)
            .unwrap()
            .resolved_text(Path::new(FILE))
            .as_deref(),
        Some(NEWER)
    );
}
