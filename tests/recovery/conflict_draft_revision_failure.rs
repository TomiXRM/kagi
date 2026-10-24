//! Unrelated index revisions cannot discard an unsaved Result after restore failure.
use super::*;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use kagi_ui_core::i18n::{Lang, Msg};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

const UNSAVED: &str = "UNRELATED INDEX; UNSAVED RESULT\n";

pub(super) fn check_unrelated_restore_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        check_language(cx, language);
    }
}

fn check_language(cx: &mut VisualTestAppContext, language: Lang) {
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let file = Path::new(FILE);
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_window_active_for_e2e(true, cx);
    });
    let input = result_input(cx, &app, window);
    let view = conflict(cx, &app);
    let original = cx.read(|cx| input.read(cx).value().to_string());
    let owner = cx.read(|cx| app.read(cx).active_session().unwrap());
    let (revision, identity, store) = cx.read(|cx| {
        let mode = view.read(cx).mode.as_ref().unwrap();
        (
            mode.revision.clone(),
            mode.buffer.file_identity(file).unwrap().clone(),
            mode.buffer.autosave().unwrap(),
        )
    });
    let primary = std::fs::read(&store).unwrap();
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        Duration::ZERO,
    )
    .unwrap();
    // Native input owns both InputState history and the production buffer callback.
    replace_result(cx, &app, window, &input, UNSAVED);
    input.update(cx, |input, cx| input.set_selected_range(2..8, cx));
    let mut history = cx.read(|cx| {
        let buffer = &view.read(cx).mode.as_ref().unwrap().buffer;
        assert!(buffer.queue_autosave().unwrap().wait().is_err());
        assert!(buffer.has_unflushed_drafts());
        buffer.clone()
    });
    assert!(history.undo(file));
    let previous = history.resolved_text(file);
    assert_ne!(previous.as_deref(), Some(UNSAVED));
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
    assert_eq!(
        std::fs::read(&store).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    drop(lock);
    let stages = || {
        let repository = git2::Repository::open(&repo).unwrap();
        let index = repository.index().unwrap();
        (1..=3)
            .map(|stage| {
                let entry = index.get_path(file, stage).unwrap();
                (entry.id, entry.mode, entry.path)
            })
            .collect::<Vec<_>>()
    };
    let original_stages = stages();
    fixture_git::write_file(&repo, "unrelated.txt", "external staged content\n");
    fixture_git::git(&repo, &["add", "--", "unrelated.txt"]);
    assert_eq!(stages(), original_stages);
    let before = RepositoryState::read(&repo);
    let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner));
    // Do not use detect(): its success oracle requires a newly adopted revision.
    app.update(cx, |app, cx| app.reload_manual(cx));
    crate::busy_dismiss::wait(
        cx,
        &app,
        "unrelated index unreadable draft reload",
        |state| state.reads.current_key(owner) != read_key && !state.reads.is_loading(owner),
    );
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_ne!(state.view().operation.as_ref().unwrap().revision(), &revision);
        assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), view.entity_id());
        let retained = conflict_read(&app, cx);
        let mode = retained.mode.as_ref().unwrap();
        assert_eq!(mode.revision, revision, "failed restore cannot replace the live mode");
        assert_eq!(mode.buffer.file_identity(file), Some(&identity));
        assert_eq!(retained.editor_inputs.as_ref().unwrap().result.entity_id(), input.entity_id());
        assert_eq!(input.read(cx).value().as_str(), UNSAVED);
        assert_eq!(input.read(cx).selected_range(), 2..8);
        assert_eq!(mode.buffer.resolved_text(file).as_deref(), Some(UNSAVED));
        assert!(mode.buffer.has_unflushed_drafts());
        let mut retained_history = mode.buffer.clone();
        assert!(retained_history.undo(file));
        assert_eq!(retained_history.resolved_text(file), previous);
        assert!(retained_history.redo(file));
        assert_eq!(retained_history.resolved_text(file).as_deref(), Some(UNSAVED));
        assert!(state.ui().panes_revalidating());
        assert_eq!(format!("{:?}", state.ui().pane_revalidation), "AwaitingConflict");
        assert!(state.ui().conflict_detected);
        assert!(state.ui().conflict_restore_failed);
        assert!(state.app_notice().is_none());
        let preview = Msg::conflict_draft_restore_failed();
        assert!(matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message) if message == &preview));
        assert!(state.toast_stack.as_ref().unwrap().read(cx).toasts().is_empty());
        assert!(!preview.contains('/') && preview.chars().count() < 100);
    });
    let receipts = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].op, "conflict-draft-restore");
    assert!(matches!(&receipts[0].outcome, OpOutcome::Failed { error }
        if error.contains(store.to_str().unwrap()) && error.contains("Permission denied")));
    let expected = before.with_appended_oplog(&receipts[0]);
    assert_eq!(expected, RepositoryState::read(&repo));
    click_control(cx, window, "conflict-save");
    cx.run_until_parked();
    assert_eq!(
        expected,
        RepositoryState::read(&repo),
        "closed gate blocks Save"
    );
    assert_eq!(stages(), original_stages);
    assert_eq!(head(&repo), fixture.main);
    assert_eq!(replay_head(&repo), fixture.first);

    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&store).unwrap(), primary);
    cx.read(|cx| {
        let buffer = &view.read(cx).mode.as_ref().unwrap().buffer;
        buffer.queue_autosave().unwrap().wait().unwrap();
        assert!(!buffer.has_unflushed_drafts());
    });
    detect(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(!state.ui().panes_revalidating());
        assert!(!state.ui().conflict_restore_failed);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        let retained = conflict_read(&app, cx);
        let mode = retained.mode.as_ref().unwrap();
        assert_ne!(mode.revision, revision);
        assert_eq!(mode.buffer.file_identity(file), Some(&identity));
        assert_eq!(
            retained.editor_inputs.as_ref().unwrap().result.entity_id(),
            input.entity_id()
        );
        assert_eq!(input.read(cx).value().as_str(), UNSAVED);
        assert_eq!(input.read(cx).selected_range(), 2..8);
        assert_eq!(mode.buffer.resolved_text(file).as_deref(), Some(UNSAVED));
        assert!(mode.can_continue());
    });
    view.update(cx, |view, _| {
        let buffer = &mut view.mode.as_mut().unwrap().buffer;
        assert!(buffer.undo(file));
        assert_eq!(buffer.resolved_text(file), previous);
        assert!(buffer.redo(file));
        assert_eq!(buffer.resolved_text(file).as_deref(), Some(UNSAVED));
    });
    cx.update_window(window, |_, window, cx| {
        window.focus(&input.read(cx).focus_handle(cx), cx);
    })
    .unwrap();
    // Lock-bound autosaves separate native key timestamps into multiple undo
    // groups. Preserve the original oracle and verify every actual redo state,
    // rather than assuming that the whole replacement was one native event.
    let mut undone = Vec::new();
    for _ in 0..=UNSAVED.chars().count() {
        let current = cx.read(|cx| input.read(cx).value().to_string());
        if current == original {
            break;
        }
        cx.simulate_keystrokes(window, "cmd-z");
        paint(cx, &app, window);
        assert_ne!(
            cx.read(|cx| input.read(cx).value().to_string()),
            current,
            "retained native undo history must reach the original Result"
        );
        undone.push(current);
    }
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), original);
    for expected_input in undone.into_iter().rev() {
        cx.simulate_keystrokes(window, "cmd-shift-z");
        paint(cx, &app, window);
        assert_eq!(
            cx.read(|cx| input.read(cx).value().to_string()),
            expected_input
        );
    }
    assert_eq!(cx.read(|cx| input.read(cx).value().to_string()), UNSAVED);
    assert_eq!(expected, RepositoryState::read(&repo));
    assert_eq!(stages(), original_stages);
    drop(input);
    drop(view);
    unmount(cx, app, window);
}
