//! A failed draft restore revokes Continue approval at both producer and consumer.
use super::*;
use crate::conflict_draft_revision::{conflict_read, open_continue, replace_result, result_input};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::os::unix::fs::PermissionsExt;

pub fn scenario_conflict_draft_continue_restore_admission(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        check_continue(cx, language);
    }
}

fn check_continue(cx: &mut VisualTestAppContext, language: Lang) {
    let _isolated = IsolatedLog::new();
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    app.update(cx, |app, cx| {
        app.set_lang(language, cx);
        app.set_window_active_for_e2e(true, cx);
    });
    let input = result_input(cx, &app, window);
    replace_result(cx, &app, window, &input, FIRST);
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
    open_continue(cx, &app, window);
    let approval = cx.read(|cx| app.read(cx).conflict_continue_modal().cloned().unwrap());
    assert_eq!(approval.revision, revision);

    // Paging preserves modals (unlike Reload's modal sweep), then the real pane
    // revalidation fails restore I/O without changing the Git observation.
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
    assert_eq!(
        std::fs::read(&store).unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let before = RepositoryState::read(&repo);
    let read_key = cx.read(|cx| app.read(cx).reads.current_key(owner));
    let publish_generation = cx.read(|cx| app.read(cx).ui().view_publish_gen);
    app.update(cx, |app, cx| {
        app.load_more_commits(cx);
        assert!(app.conflict_continue_modal().is_some());
    });
    crate::busy_dismiss::wait(
        cx,
        &app,
        "Continue approval paging restore failure",
        |state| {
            state.ui().view_publish_gen != publish_generation && state.ui().conflict_restore_failed
        },
    );
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.reads.current_key(owner), read_key, "paging must not pass through Reload's modal sweep");
        assert!(state.conflict_continue_modal().is_none(), "fresh restore failure dismisses approval");
        assert!(state.ui().panes_revalidating());
        assert!(state.ui().conflict_restore_failed);
        assert!(state.ui().conflict_detected);
        assert!(!state.app_sessions.has_leases());
        assert_eq!(state.view().operation.as_ref().unwrap().revision(), &revision);
        assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), view.entity_id());
        let retained = conflict_read(&app, cx);
        assert_eq!(retained.mode.as_ref().unwrap().revision, revision);
        assert_eq!(retained.editor_inputs.as_ref().unwrap().result.entity_id(), input.entity_id());
        assert_eq!(input.read(cx).value().as_ref(), FIRST);
        assert_eq!(retained.mode.as_ref().unwrap().buffer.resolved_text(Path::new(FILE)).as_deref(), Some(FIRST));
        assert!(state.app_notice().is_none());
        let preview = kagi_ui_core::i18n::Msg::conflict_draft_restore_failed();
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

    // Independently restore the captured, genuinely planned approval. No detector
    // runs between this setter and native Confirm: dismissal alone cannot pass.
    app.update(cx, |app, cx| {
        assert!(app.ui().panes_revalidating());
        app.set_conflict_continue_modal(approval);
        cx.notify();
    });
    paint(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_some()));
    click_control(cx, window, "plan-confirm");
    cx.run_until_parked();
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.conflict_continue_modal().is_none(), "consumer invalidates blocked confirmation");
        assert!(!state.app_sessions.has_leases(), "no writer admission before pane admission");
        assert!(state.ui().panes_revalidating());
        assert!(state.ui().conflict_restore_failed);
        assert_eq!(state.ui().conflict.as_ref().unwrap().entity_id(), view.entity_id());
        let retained = conflict_read(&app, cx);
        assert_eq!(retained.mode.as_ref().unwrap().revision, revision);
        assert_eq!(retained.editor_inputs.as_ref().unwrap().result.entity_id(), input.entity_id());
        assert_eq!(input.read(cx).value().as_ref(), FIRST);
        assert_eq!(retained.mode.as_ref().unwrap().buffer.resolved_text(Path::new(FILE)).as_deref(), Some(FIRST));
        let preview = kagi_ui_core::i18n::Msg::conflict_draft_restore_failed();
        assert!(matches!(&state.status_footer, kagi::ui::FooterStatus::Failed(message) if message == &preview));
        assert!(state.app_notice().is_none());
        assert!(state.toast_stack.as_ref().unwrap().read(cx).toasts().is_empty());
    });
    assert_eq!(
        expected,
        RepositoryState::read(&repo),
        "blocked Confirm never stages or advances Git or appends a receipt"
    );

    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&store).unwrap(), primary);
    detect(cx, &app, window);
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
        assert_eq!(input.read(cx).value().as_ref(), FIRST);
    });
    open_continue(cx, &app, window);
    click_control(cx, window, "plan-confirm");
    crate::app_conflict::wait_idle(cx, &app);
    detect(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.conflict_continue_modal().is_none());
        assert!(!state.app_sessions.has_leases());
        assert_ne!(
            conflict_read(&app, cx).mode.as_ref().unwrap().revision,
            revision
        );
    });
    assert_unresolved(cx, &app);
    let continued = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "rebase-continue")
        .collect::<Vec<_>>();
    assert_eq!(
        continued.len(),
        1,
        "only repaired, current approval advances the sequencer"
    );
    assert!(matches!(continued[0].outcome, OpOutcome::Success { .. }));
    assert_ne!(expected, RepositoryState::read(&repo));
    drop(input);
    drop(view);
    unmount(cx, app, window);
}

pub fn scenario_conflict_draft_recovered_save_frozen_payload(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    let _isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let repository = git2::Repository::open(&repo).unwrap();
    repository
        .config()
        .unwrap()
        .set_bool("core.filemode", false)
        .unwrap();
    repository
        .config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();
    std::fs::write(repo.join(".gitattributes"), "* text=auto\n").unwrap();
    let mut index = repository.index().unwrap();
    let mut ours = index.get_path(Path::new(FILE), 2).unwrap();
    ours.mode = 0o100755;
    index.add(&ours).unwrap();
    index.write().unwrap();
    std::fs::set_permissions(repo.join(FILE), std::fs::Permissions::from_mode(0o644)).unwrap();
    let head = repository.head().unwrap().target();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    replace_result(cx, &app, window, &input, "APPROVED\r\n");
    let view = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    let store = cx.read(|cx| {
        view.read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0)).unwrap();
    let before = RepositoryState::read(&repo);
    app.update(cx, |app, cx| app.load_more_commits(cx));
    crate::busy_dismiss::wait(cx, &app, "frozen Save restore admission", |state| {
        state.ui().conflict_restore_failed
    });
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.ui().panes_revalidating());
        assert!(!state.app_sessions.has_leases());
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        assert_eq!(input.read(cx).value().as_ref(), "APPROVED\r\n");
    });
    let failed = read_oplog_tail_for_repo(&repo, 100);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].op, "conflict-draft-restore");
    assert_eq!(
        before.with_appended_oplog(&failed[0]),
        RepositoryState::read(&repo)
    );

    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    detect(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(!state.ui().panes_revalidating());
        assert!(!state.ui().conflict_restore_failed);
        assert_eq!(
            state.ui().conflict.as_ref().unwrap().entity_id(),
            view.entity_id()
        );
        assert_eq!(input.read(cx).value().as_ref(), "APPROVED\r\n");
    });
    click_control(cx, window, "conflict-save");
    crate::app_conflict::wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    index.read(true).unwrap();
    assert!(!index.has_conflicts());
    let staged = index.get_path(Path::new(FILE), 0).unwrap();
    assert_eq!(staged.mode, 0o100755);
    assert_eq!(
        staged.id,
        git2::Oid::hash_object(git2::ObjectType::Blob, b"APPROVED\n").unwrap()
    );
    assert_eq!(
        repository.find_blob(staged.id).unwrap().content(),
        b"APPROVED\n"
    );
    assert_eq!(std::fs::read(repo.join(FILE)).unwrap(), b"APPROVED\r\n");
    assert_eq!(repository.head().unwrap().target(), head);
    let saves = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect::<Vec<_>>();
    assert_eq!(saves.len(), 1);
    assert!(matches!(saves[0].outcome, OpOutcome::Success { .. }));
    assert!(!cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    drop(input);
    drop(view);
    unmount(cx, app, window);
}
