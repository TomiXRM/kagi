//! Failed paging restore revokes Abort approval at producer and consumer seams.
use super::*;
use crate::conflict_draft_revision::{conflict_read, replace_result, result_input};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::os::unix::fs::PermissionsExt;

pub fn scenario_conflict_draft_abort_restore_admission(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        check_abort(cx, language);
    }
}

fn check_abort(cx: &mut VisualTestAppContext, language: Lang) {
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
    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    let approval = cx.read(|cx| app.read(cx).conflict_abort_modal().cloned().unwrap());
    assert_eq!(approval.operation.revision(), &revision);

    // Paging leaves the genuine confirmation open until restore itself fails;
    // Reload would sweep it first and could conceal missing producer revocation.
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
        assert!(app.conflict_abort_modal().is_some());
    });
    crate::busy_dismiss::wait(cx, &app, "Abort approval paging restore failure", |state| {
        state.ui().view_publish_gen != publish_generation && state.ui().conflict_restore_failed
    });
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.reads.current_key(owner), read_key);
        assert!(state.conflict_abort_modal().is_none(), "restore failure revokes Abort");
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

    // Reinstall only the captured real approval, never a mocked plan or outcome.
    // No detector runs before native Confirm: producer dismissal cannot pass this.
    app.update(cx, |app, cx| {
        assert!(app.ui().panes_revalidating());
        app.set_conflict_abort_modal(approval);
        cx.notify();
    });
    paint(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()));
    click_control(cx, window, "plan-confirm");
    cx.run_until_parked();
    paint(cx, &app, window);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.conflict_abort_modal().is_none(), "consumer clears denied approval");
        assert!(!state.app_sessions.has_leases(), "no writer admission before pane admission");
        assert!(state.ui().panes_revalidating());
        assert!(state.ui().conflict_restore_failed);
        assert_eq!(state.view().operation.as_ref().unwrap().revision(), &revision);
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
        "blocked Abort changes no Git state and appends no duplicate receipt"
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
    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()));
    click_control(cx, window, "plan-confirm");
    crate::app_conflict::wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.conflict_abort_modal().is_none());
        assert!(!state.app_sessions.has_leases());
        assert!(state.view().operation.is_none());
    });
    assert!(!repo.join(".git/rebase-merge").exists());
    assert!(!repo.join(".git/rebase-apply").exists());
    assert_eq!(
        std::fs::read_to_string(repo.join(FILE)).unwrap(),
        "SIDE TWO\n"
    );
    let aborted = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "rebase-abort")
        .collect::<Vec<_>>();
    assert_eq!(
        aborted.len(),
        1,
        "only repaired approval aborts the sequencer"
    );
    assert!(matches!(aborted[0].outcome, OpOutcome::Success { .. }));
    assert_ne!(expected, RepositoryState::read(&repo));
    drop(input);
    drop(view);
    unmount(cx, app, window);
}
