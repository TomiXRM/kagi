//! #711: real conflict controls must name the refusal, not just count blockers.
use super::{click_control, content_fixture, stuck_merging_fixture, wait_idle};
use crate::macos::{git, mount, unmount};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};
use std::path::Path;

/// The language as it was, and its saved key (#516), restored on drop.
struct RestoreLanguage {
    language: Lang,
    _saved: crate::gui_isolation::SavedKeys,
}

impl Drop for RestoreLanguage {
    fn drop(&mut self) {
        i18n::set_lang(self.language);
    }
}

pub(super) fn save_and_abort(cx: &mut VisualTestAppContext) {
    let _restore_language = RestoreLanguage {
        language: i18n::lang(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["lang"]),
    };
    for language in [Lang::En, Lang::Ja] {
        i18n::set_lang(language);
        save(cx);
        external_worktree(cx, false);
        external_worktree(cx, true);
        abort(cx, false);
        abort(cx, true);
    }
}

fn save(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_window_active_for_e2e(false, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_reset_all(Path::new("file.txt"));
        view.conflict_show_page(kagi::ui::conflict_view::ConflictPage::Review, cx);
        cx.notify();
    });
    assert!(cx.read(|cx| {
        let view = conflict.read(cx);
        let mode = view.mode.as_ref().expect("conflict mode");
        matches!(
            mode.buffer.conflict_draft(Path::new("file.txt")),
            Some(kagi_domain::conflict_family::ConflictDraft::Text { bytes, .. })
                if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
        )
    }));
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_hunk_side(
            Path::new("file.txt"),
            0,
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        cx.notify();
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    // Keep the ready draft and its rendered Save, but invalidate the observed
    // revision externally. The real Save route must refuse before any write.
    std::fs::write(repo.join("file.txt"), "changed outside Kagi\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    let save_bounds = e2e::control_bounds(window.window_id(), "conflict-save").unwrap();
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let before = std::fs::read(repo.join("file.txt")).unwrap();
    cx.simulate_click(window, save_bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "conflict-save:merge")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
        if blockers.iter().any(|reason| reason.contains("conflict changed since it was observed")))
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), before);
    assert_refusal(
        cx,
        &app,
        window,
        &entries[0],
        "conflict-save:merge: refused (1 blocker)",
    );
    drop(conflict);
    unmount(cx, app, window);
}

fn abort(cx: &mut VisualTestAppContext, banner: bool) {
    let fixture = if banner {
        stuck_merging_fixture()
    } else {
        content_fixture()
    };
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_window_active_for_e2e(false, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    if banner {
        click_control(cx, window, "conflict-continue");
        wait_idle(cx, &app);
        crate::busy_dismiss::ready(cx, &app);
    }
    let control = if banner {
        "operation-strip-abort"
    } else {
        "conflict-abort"
    };
    click_control(cx, window, control);
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()));
    // Drift after the confirmation opens. What the user confirms is still the
    // revision they saw, so the family must refuse it by name.
    std::fs::write(repo.join("file.txt"), "resolved outside Kagi\n").unwrap();
    git(&repo, &["add", "file.txt"]);
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
    app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
    wait_idle(cx, &app);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("file.txt")).unwrap(),
        "resolved outside Kagi\n"
    );
    let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
        .into_iter()
        .filter(|entry| entry.op == "merge-abort")
        .collect();
    assert_eq!(entries.len(), 1);
    assert!(
        matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
        if blockers.iter().any(|reason| reason.contains("conflict changed since it was observed")))
    );
    assert_refusal(
        cx,
        &app,
        window,
        &entries[0],
        "merge-abort: refused (1 blocker)",
    );

    // #755: a reload that lands while such a confirmation is open closes it
    // rather than refreshing it in place, and executes nothing on the way.
    click_control(cx, window, control);
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "the control reopens a confirmation against the current state"
    );
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "an accepted reload must sweep the stale abort confirmation"
    );
    assert_eq!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .filter(|entry| entry.op == "merge-abort")
            .count(),
        1,
        "sweeping a confirmation must not run the operation it was showing"
    );
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(
        std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
    unmount(cx, app, window);
}

fn assert_refusal(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
    receipt: &kagi_git::oplog::OpLogEntry,
    footer: &str,
) {
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.app_notice().is_none(),
            "a known refusal needs no dismissal"
        );
        assert!(!e2e::queued_notice_contains(state, &receipt.repo));
        let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
        assert_eq!(
            toasts.len(),
            1,
            "one unattended refusal, not three primary surfaces"
        );
        assert_eq!(toasts[0].kind, kagi::ui::ToastKind::Error);
        assert!(!matches!(
            state.status_footer,
            kagi::ui::FooterStatus::Failed(_) | kagi::ui::FooterStatus::Success(_)
        ));
        for internal in [
            "conflict-save:",
            "merge-abort:",
            "resolution buffer",
            "since it was observed",
            "(1 blocker)",
            receipt.repo.as_str(),
        ] {
            assert!(
                !toasts[0].message.contains(internal),
                "{}",
                toasts[0].message
            );
        }
        // English klog is an explicit external contract, not a user-copy pin.
        assert!(kagi_ui_core::klog::tail()
            .iter()
            .any(|line| line == &format!("[kagi] footer: {footer}")));
        assert!(!state.app_sessions.has_leases());
    });
    click_control(cx, window, "operation-result-details");
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(state.bottom_panel_open);
        assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
        let panel = state.op_log.as_ref().unwrap().read(cx);
        let row = &panel.entries()[panel.expanded().unwrap()];
        assert_eq!(
            (row.id, row.timestamp, &row.op, &row.repo),
            (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
        );
    });
}

/// An already delivered Continue intent is refused by the real unresolved
/// buffer. This does not claim the disabled Continue button accepts new input.
pub(super) fn continue_result_owner(cx: &mut VisualTestAppContext) {
    let _restore_language = RestoreLanguage {
        language: i18n::lang(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["lang"]),
    };
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    for language in [Lang::En, Lang::Ja] {
        for recording_failure in [false, true] {
            i18n::set_lang(language);
            let fixture = content_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_window_active_for_e2e(false, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
            let worktree = std::fs::read(repo.join("file.txt")).unwrap();
            let panel_before = cx.read(|cx| {
                let state = app.read(cx);
                (state.bottom_panel_open, state.bottom_tab)
            });
            let recording_lock = recording_failure.then(|| {
                let path = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                    .join("operations.jsonl.lock");
                let lock = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(path)
                    .unwrap();
                lock.lock().unwrap();
                lock
            });
            cx.update_window(window, |_, window, cx| {
                app.update(cx, |app, cx| {
                    let owner = app
                        .app_sessions
                        .attachment(app.active_session().unwrap())
                        .unwrap();
                    app.conflict_continue(owner, window, cx);
                });
                window.draw(cx).clear();
            })
            .unwrap();
            crate::busy_dismiss::wait(cx, &app, "Continue refusal after draft flush", |app| {
                if recording_failure {
                    app.app_notice().is_some()
                } else {
                    read_oplog_tail_for_repo(&repo, 100)
                        .iter()
                        .any(|entry| entry.op == "merge-continue")
                }
            });
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            cx.read(|cx| {
                let state = app.read(cx);
                assert!(!state.app_sessions.has_leases());
                assert_eq!((state.bottom_panel_open, state.bottom_tab), panel_before);
                assert!(!matches!(
                    state.status_footer,
                    kagi::ui::FooterStatus::Failed(_)
                ));
                let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
                if recording_failure {
                    let notice = state
                        .app_notice()
                        .expect("failed append requires acknowledgment");
                    assert!(notice
                        .message
                        .contains(&std::fs::TryLockError::WouldBlock.to_string()));
                    assert!(
                        toasts.is_empty(),
                        "no pre-announced refusal may survive beside acknowledgment"
                    );
                } else {
                    assert!(state.app_notice().is_none());
                    assert_eq!(
                        toasts.len(),
                        1,
                        "the typed refusal has one presentation owner"
                    );
                    assert_eq!(toasts[0].kind, kagi::ui::ToastKind::Error);
                    assert!(!toasts[0].message.contains(repo.to_str().unwrap()));
                }
            });
            let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "merge-continue")
                .collect();
            assert_eq!(entries.len(), usize::from(!recording_failure));
            if !recording_failure {
                assert!(
                    matches!(&entries[0].outcome, OpOutcome::Refused { blockers }
                    if blockers.len() == 1)
                );
                assert_eq!(entries[0].ref_moves, Some(Vec::new()));
                click_control(cx, window, "operation-result-details");
                cx.read(|cx| {
                    let state = app.read(cx);
                    assert!(state.bottom_panel_open);
                    assert_eq!(state.bottom_tab, kagi::ui::BottomTab::OperationLog);
                    let panel = state.op_log.as_ref().unwrap().read(cx);
                    let row = &panel.entries()[panel.expanded().unwrap()];
                    let receipt = &entries[0];
                    assert_eq!(
                        (row.id, row.timestamp, &row.op, &row.repo),
                        (receipt.id, receipt.timestamp, &receipt.op, &receipt.repo)
                    );
                });
            }
            drop(recording_lock);
            if recording_failure {
                click_control(cx, window, "app-notice-confirm");
                assert!(cx.read(|cx| app.read(cx).app_notice().is_none()));
            }
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
                merge_head
            );
            assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), worktree);
            assert_eq!(
                read_oplog_tail_for_repo(&repo, 100).len(),
                entries.len(),
                "inspection and acknowledgment do not execute or append again"
            );
            unmount(cx, app, window);
        }
    }
}

fn external_worktree(cx: &mut VisualTestAppContext, before_first_choice: bool) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let original_bytes = std::fs::read(repo.join("file.txt")).unwrap();
    let mut bytes = original_bytes.clone();
    bytes.extend_from_slice(b"\n// EXTERNAL_WT_MUST_SURVIVE\n");
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        cx.notify();
    });
    cx.run_until_parked();
    if before_first_choice {
        std::fs::write(repo.join("file.txt"), &bytes).unwrap();
        app.update(cx, |app, cx| app.reload_external(cx));
        cx.run_until_parked();
    }
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_hunk_side(
            Path::new("file.txt"),
            0,
            kagi_git::resolution::SelectionSide::Incoming,
            true,
        );
        cx.notify();
    });
    cx.run_until_parked();
    let draft = cx.read(|cx| {
        conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .resolved_text(Path::new("file.txt"))
            .unwrap()
    });
    if !before_first_choice {
        std::fs::write(repo.join("file.txt"), &bytes).unwrap();
    }
    let index = std::fs::read(repo.join(".git/index")).unwrap();
    // Deliver the same refresh used by the worktree watcher's conflict branch.
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.read(|cx| {
        let view = conflict.read(cx);
        let header = view.file_header_status_for_e2e();
        assert!(header.contains(kagi::ui::i18n::Msg::ConflictWorktreeChanged.t()));
        let mode = view.mode.as_ref().unwrap();
        let path = Path::new("file.txt");
        assert!(mode.buffer.worktree_changed(path));
        assert!(
            mode.buffer.has_resolution(path),
            "the index materialization still matches"
        );
        assert_eq!(
            mode.buffer.resolved_text(path).as_deref(),
            Some(draft.as_str())
        );
        assert!(
            !mode
                .buffer
                .hunk_model(path)
                .unwrap()
                .assembled_text()
                .contains("EXTERNAL_WT_MUST_SURVIVE"),
            "index hunks are not rebuilt from WT markers"
        );
    });
    let save = e2e::control_bounds(window.window_id(), "conflict-save").unwrap();
    cx.simulate_click(window, save.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), bytes);
    assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(
        read_oplog_tail_for_repo(&repo, 100)
            .iter()
            .all(|entry| entry.op != "conflict-save:merge"),
        "disabled Save must not dispatch"
    );
    click_control(cx, window, "conflict-reload");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| conflict
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .worktree_changed(Path::new("file.txt"))),
        "reload cannot bless changed bytes"
    );
    assert_eq!(std::fs::read(repo.join("file.txt")).unwrap(), bytes);
    // The user restores the reviewed source externally; Reload may now clear
    // the blocker without discarding the draft's matching hunk decisions.
    std::fs::write(repo.join("file.txt"), &original_bytes).unwrap();
    click_control(cx, window, "conflict-reload");
    cx.run_until_parked();
    assert!(cx.read(|cx| {
        let view = conflict.read(cx);
        let buffer = &view.mode.as_ref().unwrap().buffer;
        !buffer.worktree_changed(Path::new("file.txt"))
            && buffer.resolved_text(Path::new("file.txt")).as_deref() == Some(draft.as_str())
    }));
    std::fs::write(
        repo.join("file.txt"),
        "resolved externally\n// EXTERNAL_WT_MUST_SURVIVE\n",
    )
    .unwrap();
    git(&repo, &["add", "file.txt"]);
    app.update(cx, |app, cx| app.reload_external(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| {
            let view = conflict.read(cx);
            let mode = view.mode.as_ref().unwrap();
            view.selected_saved.as_deref() == Some(Path::new("file.txt"))
                && mode.session.files.is_empty()
                && mode
                    .session
                    .resolved
                    .iter()
                    .any(|file| file.path == Path::new("file.txt"))
        }),
        "external staging moves the selected path to Saved"
    );
    drop(conflict);
    unmount(cx, app, window);
}
