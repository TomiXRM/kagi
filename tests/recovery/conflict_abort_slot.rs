//! #755: the window's single modal slot around the conflict Abort
//! confirmation — which focus its Escape resolves against, and which reload is
//! allowed to sweep it.
use crate::app_conflict::{click_control, content_fixture, wait_idle};
use crate::macos::{mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi_git::oplog::read_oplog_tail_for_repo;
use std::path::Path;

#[path = "../support/cherry_pick_sequence.rs"]
mod cherry_pick_sequence;

/// The native confirmation and its receipt must describe the entire sequence.
pub fn scenario_cherry_pick_abort_sequence(cx: &mut VisualTestAppContext) {
    use kagi::ui::i18n::{self, Lang};
    use kagi_git::oplog::OpOutcome;
    struct RestoreLanguage {
        lang: Lang,
        _saved: crate::gui_isolation::SavedKeys,
    }
    impl Drop for RestoreLanguage {
        fn drop(&mut self) {
            i18n::set_lang(self.lang);
        }
    }
    let _language = RestoreLanguage {
        lang: i18n::lang(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["lang"]),
    };
    for (lang, unavailable, moved) in [
        (
            Lang::En,
            "pre-sequence HEAD cannot be established",
            "HEAD moved since the sequence started",
        ),
        (
            Lang::Ja,
            "sequence 開始前の HEAD を確認できません",
            "sequence 開始後に HEAD が移動したため",
        ),
    ] {
        i18n::set_lang(lang);
        for case in ["ready", "missing-start", "head-moved"] {
            let fixture = cherry_pick_sequence::Fixture::new(true, false);
            let repo = fixture.path().canonicalize().unwrap();
            let refusal = match case {
                "missing-start" => {
                    std::fs::remove_file(repo.join(".git/sequencer/head")).unwrap();
                    Some(unavailable)
                }
                "head-moved" => {
                    std::fs::write(repo.join(".git/sequencer/abort-safety"), &fixture.start)
                        .unwrap();
                    Some(moved)
                }
                _ => None,
            };
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| app.detect_conflict_mode(cx));
            cx.run_until_parked();
            click_control(cx, window, "conflict-abort");
            cx.run_until_parked();
            cx.read(|cx| {
                let state = app.read(cx);
                let modal = state.conflict_abort_modal().expect("abort confirmation");
                if let Some(reason) = refusal {
                    assert!(modal
                        .plan
                        .blockers
                        .iter()
                        .any(|note| i18n::plan::plan_note_text(note).contains(reason)));
                } else {
                    assert!(modal.plan.predicted.head.contains(&fixture.start[..7]));
                }
            });
            draw(cx, window);
            let before = crate::macos::repo_fingerprint(&repo);
            let confirm = kagi::ui::button_style::recorded_modal_button("plan-confirm")
                .expect("abort confirmation button is drawn");
            assert_eq!(confirm.disabled, refusal.is_some(), "{lang:?}/{case}");
            if let Some(reason) = refusal {
                assert!(confirm.description.as_deref().unwrap().contains(reason));
                // A blocked action is intentionally inert and has no Tab stop.
                // Preserve the app-boundary refusal/receipt oracle separately,
                // as the other blocked destructive confirmations do (#1277).
                app.update(cx, |app, cx| app.confirm_conflict_abort(cx));
            } else {
                crate::destructive_modal_return::focused_confirm(cx, &app, window, "plan-confirm");
            }
            wait_idle(cx, &app);
            let entries: Vec<_> = read_oplog_tail_for_repo(&repo, 100)
                .into_iter()
                .filter(|entry| entry.op == "cherry-pick-abort")
                .collect();
            assert_eq!(entries.len(), 1);
            let backend = kagi_git::Backend::open(&repo).unwrap();
            if let Some(reason) = refusal {
                assert!(matches!(entries[0].outcome, OpOutcome::Refused { .. }));
                assert!(repo.join(".git/sequencer").exists());
                assert_eq!(crate::macos::repo_fingerprint(&repo), before);
                cx.read(|cx| {
                    let state = app.read(cx);
                    let toast = state
                        .toast_stack
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .toasts()
                        .last()
                        .unwrap();
                    assert!(toast.message.contains(reason), "{}", toast.message);
                });
            } else {
                assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
                assert!(backend.conflict_snapshot().unwrap().is_none());
                let repository = git2::Repository::open(&repo).unwrap();
                assert_eq!(
                    repository.head().unwrap().target().unwrap().to_string(),
                    fixture.start
                );
                assert!(repository.statuses(None).unwrap().is_empty());
                assert!(entries[0]
                    .ref_moves
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|movement| movement.new.as_deref() == Some(fixture.start.as_str())));
            }
            drop(backend);
            unmount(cx, app, window);
        }
    }
    eprintln!("[gui-e2e] PASS cherry_pick_abort_sequence");
}

fn draw(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    cx.run_until_parked();
}

/// Escape must reach the modal slot from the focus a user actually holds, not
/// only from the window root. Every other keyboard scenario focuses
/// `root_focus` before delivering the key (`press_key` in
/// `tests/recovery/operations.rs`), so the app's single Escape route — the
/// `CloseMainDiff` action — has never been exercised from the conflict
/// screen's own focus.
///
/// The reachable path: the Result pane mounts its code editor only in Edit
/// mode (`conflict_editor.rs`), so editing and switching back to Preview
/// leaves the window focused on an element that is no longer drawn. gpui then
/// dispatches from the tree root, where neither the routing wrapper's action
/// handler nor any key context exists, and every window action — Escape
/// included — silently does nothing. Opening the confirmation takes focus back
/// to the root, which is what this scenario holds.
pub fn scenario_conflict_abort_escape_focus(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();

    // Open the conflicting file and switch the Result pane to Edit, the only
    // mode that mounts the code editor. The parent's own render sync builds
    // the real `InputState`; nothing here is a test double.
    conflict.update(cx, |view, cx| {
        view.conflict_open_editor(Path::new("file.txt"));
        view.conflict_editor_set_result_mode(true);
        cx.notify();
    });
    draw(cx, window);
    let result_input = cx
        .read(|cx| {
            conflict
                .read(cx)
                .editor_inputs
                .as_ref()
                .map(|inputs| inputs.result.clone())
        })
        .expect("the Result pane's code editor is mounted");

    // The user clicks into the Result editor: its focus is live, so window
    // actions resolve against it.
    cx.update_window(window, |_, window, cx| {
        let handle = result_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    })
    .unwrap();
    draw(cx, window);
    assert!(
        cx.update_window(window, |_, window, cx| {
            window.is_action_available(&kagi::ui::CloseMainDiff, cx)
        })
        .unwrap(),
        "precondition: a mounted, focused Result editor still reaches the \
         window's Escape action"
    );

    // …and switches back to Preview, which unmounts the editor under the
    // focus: the reachable dangling-focus path diagnosed in #755. (Which focus
    // #711's Esc was delivered into was never observed.)
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_result_mode(false);
        cx.notify();
    });
    draw(cx, window);
    let text_before = cx.read(|cx| result_input.read(cx).value().to_string());

    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "precondition: the dashboard Abort opens the confirmation"
    );

    // Deliver Escape where the app left focus. The test never re-focuses.
    draw(cx, window);
    cx.simulate_keystrokes(window, "escape");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "Escape must close the Abort confirmation after the Result editor's \
         focus stopped being drawn"
    );
    assert_eq!(
        cx.read(|cx| result_input.read(cx).value().to_string()),
        text_before,
        "cancelling must not touch the resolution the editor is holding"
    );
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "and it mutates nothing"
    );
    assert!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .all(|entry| entry.op != "merge-abort"),
        "no abort may be recorded by a cancelled confirmation"
    );

    drop(result_input);
    drop(conflict);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_abort_escape_focus");
}

/// Only an **accepted** reload may sweep the confirmation. `apply_reload_data`
/// hands its read to `accept_tab_view` first and returns when that read has
/// been superseded — by a newer read or by an admitted mutation — so the
/// confirmation the user is looking at survives a reload whose observation is
/// already out of date, and is closed by the next one that is not.
///
/// This scenario covers superseded same-owner reads; foreign-owner completion
/// is separately gated by the unchanged `active_session` guard in
/// `apply_reload_data`.
pub fn scenario_conflict_abort_superseded_reload(cx: &mut VisualTestAppContext) {
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.detect_conflict_mode(cx));
    cx.run_until_parked();

    click_control(cx, window, "conflict-abort");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "precondition: the dashboard Abort opens the confirmation"
    );

    // Start a reload and supersede its read before the dispatcher delivers it,
    // exactly as a later read or an admitted mutation does.
    app.update(cx, |app, cx| {
        app.reload(cx);
        let session = app.active_session().expect("owner");
        let _superseding = app.reads.begin(session);
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_some()),
        "a superseded reload must not close the confirmation on screen"
    );

    // …and the next accepted reload does close it, so the assertion above is
    // about the read being refused, not about the sweep being absent.
    app.update(cx, |app, cx| app.reload(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()),
        "an accepted reload must sweep the confirmation"
    );

    assert!(repo.join(".git/MERGE_HEAD").exists());
    assert!(
        read_oplog_tail_for_repo(&repo, 100)
            .into_iter()
            .all(|entry| entry.op != "merge-abort"),
        "neither reload may run the operation the confirmation was showing"
    );
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS conflict_abort_superseded_reload");
}

/// Legacy synchronous planning preserves the failed receipt and activates its
/// required acknowledgment, instead of losing it in a preparation-error toast.
pub fn scenario_legacy_plan_recording_failure(cx: &mut VisualTestAppContext) {
    use kagi::ui::{e2e, i18n, FooterStatus};
    use kagi_git::oplog::OpOutcome;
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let previous = i18n::lang();
    for language in [i18n::Lang::En, i18n::Lang::Ja] {
        for operation in ["merge-abort", "merge-commit"] {
            let fixture = crate::app_conflict::stuck_merging_fixture();
            let repo = fixture.path().canonicalize().unwrap();
            if operation == "merge-commit" {
                std::fs::write(repo.join("file.txt"), "resolved for merge commit\n").unwrap();
                crate::macos::git(&repo, &["add", "file.txt"]);
            }
            let (app, window) = mount(cx, &repo);
            app.update(cx, |app, cx| {
                app.set_lang(language, cx);
                app.set_window_active_for_e2e(false, cx);
                app.detect_conflict_mode(cx);
            });
            cx.run_until_parked();
            if operation == "merge-commit" {
                click_control(cx, window, "conflict-continue");
                wait_idle(cx, &app);
                crate::busy_dismiss::ready(cx, &app);
                assert!(cx.read(|cx| app.read(cx).ui().conflict_merge_pending));
            }
            let before = read_oplog_tail_for_repo(&repo, 100).len();
            let toast_ids_before = cx.read(|cx| {
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
            let fingerprint = crate::macos::repo_fingerprint(&repo);
            let index = std::fs::read(repo.join(".git/index")).unwrap();
            let merge_head = std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
            let log_lock = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap())
                .join("operations.jsonl.lock");
            let append_lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(log_lock)
                .unwrap();
            append_lock.lock().unwrap();
            let owner = crate::repository_lock::RepositoryLockOwner::acquire(&repo);
            if operation == "merge-abort" {
                click_control(cx, window, "conflict-abort");
            } else {
                app.update(cx, |app, cx| {
                    let owner = app.active_session().unwrap();
                    app.open_commit_plan_modal(owner, cx);
                });
            }
            cx.run_until_parked();
            cx.read(|cx| {
                let state = app.read(cx);
                let rows = state.op_log.as_ref().unwrap().read(cx).entries();
                let attempts: Vec<_> = rows.iter().filter(|row| row.op == operation).collect();
                assert_eq!(attempts.len(), 1, "one attempted UI row for {operation}");
                let attempted = attempts[0];
                assert!(matches!(&attempted.outcome, OpOutcome::Refused { blockers }
                    if blockers == &["Another Kagi window is operating on this repository"]));
                assert_eq!(attempted.before.head, "unobserved");
                assert_eq!(attempted.before.dirty, "unobserved");
                let notice = state
                    .app_notice()
                    .expect("required recording-failure acknowledgment");
                assert!(notice.message.contains(&repo.display().to_string()));
                assert!(notice
                    .message
                    .contains(&std::fs::TryLockError::WouldBlock.to_string()));
                assert!(!e2e::queued_notice_contains(
                    state,
                    &repo.display().to_string()
                ));
                assert_eq!(
                    state
                        .toast_stack
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .toasts()
                        .iter()
                        .map(|toast| toast.id)
                        .collect::<Vec<_>>(),
                    toast_ids_before,
                    "required acknowledgment adds no ordinary toast"
                );
                assert!(!matches!(
                    state.status_footer,
                    FooterStatus::Failed(_) | FooterStatus::Success(_)
                ));
                assert!(state.conflict_abort_modal().is_none());
                assert!(!state.app_sessions.has_leases());
            });
            assert_eq!(read_oplog_tail_for_repo(&repo, 100).len(), before);
            assert_eq!(crate::macos::repo_fingerprint(&repo), fingerprint);
            assert_eq!(std::fs::read(repo.join(".git/index")).unwrap(), index);
            assert_eq!(
                std::fs::read(repo.join(".git/MERGE_HEAD")).unwrap(),
                merge_head
            );
            drop(owner);
            drop(append_lock);
            unmount(cx, app, window);
        }
    }
    i18n::set_lang(previous);
}
