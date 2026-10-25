//! #1731: a real autosave I/O failure must not silently lose a live Result draft.
use crate::app_conflict::{click_control, content_fixture, wait_idle};
use crate::conflict_draft_revision::{
    conflict_read, detect, paint, replace_result, result_input, RebaseFixture, FILE,
};
use crate::host_dirty_guard::{alive, cleanup, request as request_host, Entry};
use crate::macos::mount;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{Lang, Msg},
    FooterStatus, KagiApp, ToastKind,
};
use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    resolution::ResolutionBuffer,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

pub(super) const DRAFT: &str = "MANUAL RESULT MUST SURVIVE FAILED AUTOSAVE\n";

pub(super) struct IsolatedLog {
    previous: Option<OsString>,
    directory: tempfile::TempDir,
}

impl IsolatedLog {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", directory.path());
        Self {
            previous,
            directory,
        }
    }
}

impl Drop for IsolatedLog {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => std::env::set_var("KAGI_LOG_DIR", previous),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}

pub(super) struct UnwritableDirectory {
    path: PathBuf,
    permissions: Permissions,
}

impl UnwritableDirectory {
    pub(super) fn new(path: PathBuf) -> Self {
        std::fs::create_dir_all(&path).unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let guard = Self { path, permissions };
        std::fs::set_permissions(&guard.path, Permissions::from_mode(0o000)).unwrap();
        guard
    }

    pub(super) fn restore(&self) {
        std::fs::set_permissions(&self.path, self.permissions.clone()).unwrap();
    }
}

impl Drop for UnwritableDirectory {
    fn drop(&mut self) {
        // Restore before TempDir removal, including when an assertion unwinds.
        let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
    }
}

fn failed_receipts(repo: &Path) -> usize {
    read_oplog_tail_for_repo(repo, 1000)
        .iter()
        .filter(|entry| {
            entry.op == "conflict-draft-autosave"
                && matches!(entry.outcome, OpOutcome::Failed { .. })
        })
        .count()
}

fn assert_live(cx: &VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    assert!(
        alive(cx, window),
        "unapproved draft loss must not remove the host"
    );
    cx.read(|cx| {
        assert!(!cx.global::<e2e::QuitForE2e>().requested);
        let conflict = conflict_read(app, cx);
        assert!(
            conflict.draft_persist_failed.is_some(),
            "real autosave failure is retained"
        );
        assert_eq!(
            conflict
                .mode
                .as_ref()
                .unwrap()
                .buffer
                .resolved_text(Path::new(FILE))
                .as_deref(),
            Some(DRAFT)
        );
        assert_eq!(
            conflict
                .editor_inputs
                .as_ref()
                .unwrap()
                .result
                .read(cx)
                .value()
                .as_str(),
            DRAFT
        );
    });
}

fn assert_guard(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    lang: Lang,
) {
    e2e::take_conflict_draft_guard();
    paint(cx, app, window);
    let e2e::ConflictDraftGuardPresentation {
        title,
        retry,
        discard,
        detail,
        ..
    } = e2e::take_conflict_draft_guard().expect("production renderer selected failure labels");
    assert_eq!(
        title,
        match lang {
            Lang::En => "Draft could not be saved",
            Lang::Ja => "下書きを保存できていません",
        }
    );
    assert_eq!(retry, Msg::ConflictDraftRetry.t());
    assert_eq!(discard, Msg::ConflictDraftDiscardClose.t());
    assert_eq!(detail, Msg::ConflictDraftUnsavedDetail.t());
    for id in [
        "editor-dirty-guard-save",
        "editor-dirty-guard-discard",
        "editor-dirty-guard-cancel",
    ] {
        assert!(
            e2e::control_bounds(window.window_id(), id).is_some(),
            "rendered {id}"
        );
    }
    assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_some()));
    assert_live(cx, app, window);
}

fn assert_error_toast(cx: &VisualTestAppContext, app: &Entity<KagiApp>, repo: &Path) {
    cx.read(|cx| {
        let app = app.read(cx);
        let stack = app.toast_stack.as_ref().unwrap().read(cx);
        let toast = stack
            .toasts()
            .iter()
            .rev()
            .find(|toast| toast.kind == ToastKind::Error)
            .expect("autosave failure must surface an Error toast");
        assert!(!toast.message.is_empty());
        assert!(!toast.message.contains('\n'), "preview must stay bounded");
        assert!(
            !toast.message.contains(repo.to_str().unwrap()),
            "no absolute fixture path in preview"
        );
    });
}

fn request_close(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    entry: Option<Entry>,
) {
    if let Some(entry) = entry {
        request_host(cx, app, window, entry);
    } else {
        // Exercise the real root shortcut, including the last-tab → Welcome path.
        cx.simulate_keystrokes(window, "cmd-w");
        cx.run_until_parked();
    }
}

fn run_case(cx: &mut VisualTestAppContext, lang: Lang, entry: Option<Entry>, discard: bool) {
    let isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let original = std::fs::read(repo.join(FILE)).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        // Autosave failures must announce even in the foreground Graph view,
        // where ordinary short operation-result toasts are intentionally quiet.
        app.set_window_active_for_e2e(true, cx);
    });
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    // Deny actual filesystem access, not an injected view flag or mocked backend.
    let denied = UnwritableDirectory::new(isolated.directory.path().join("conflicts"));
    let before = failed_receipts(&repo);
    let failure_log_prefix = "[kagi] conflict-mode: resolution draft autosave failed: ";
    let failure_logs = || {
        kagi_ui_core::klog::tail()
            .iter()
            .filter(|line| {
                line.starts_with(failure_log_prefix) && line.contains(denied.path.to_str().unwrap())
            })
            .count()
    };
    let before_logs = failure_logs();
    replace_result(cx, &app, window, &input, DRAFT);
    assert_live(cx, &app, window);
    assert_eq!(
        failure_logs(),
        before_logs + 1,
        "one line per failed generation"
    );
    assert!(
        failed_receipts(&repo) > before,
        "autosave failure must be durable in oplog"
    );
    assert_error_toast(cx, &app, &repo);

    request_close(cx, &app, window, entry);
    assert_guard(cx, &app, window, lang);
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    click_control(cx, window, "editor-dirty-guard-cancel");
    assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
    assert_live(cx, &app, window);
    request_close(cx, &app, window, entry);
    assert_guard(cx, &app, window, lang);

    if discard {
        click_control(cx, window, "editor-dirty-guard-discard");
        assert_live(cx, &app, window);
        assert!(cx.read(|cx| app
            .read(cx)
            .editor_dirty_guard_modal()
            .unwrap()
            .conflict_discard_armed));
        click_control(cx, window, "editor-dirty-guard-discard");
    } else {
        let before_retry = failed_receipts(&repo);
        let before_retry_logs = failure_logs();
        click_control(cx, window, "editor-dirty-guard-save");
        assert_guard(cx, &app, window, lang);
        assert!(
            failed_receipts(&repo) > before_retry,
            "Retry must attempt real persistence again"
        );
        assert_error_toast(cx, &app, &repo);
        assert_eq!(
            failure_logs(),
            before_retry_logs + 1,
            "Retry logs its new failed generation once despite wait/flush observers"
        );
        denied.restore();
        click_control(cx, window, "editor-dirty-guard-save");
        cx.read(|cx| {
            assert!(
                conflict.read(cx).draft_persist_failed.is_none(),
                "successful retry clears failure"
            );
            assert!(app.read(cx).editor_dirty_guard_modal().is_none());
        });
        assert_eq!(
            ResolutionBuffer::load(&repo)
                .unwrap()
                .resolved_text(Path::new(FILE))
                .as_deref(),
            Some(DRAFT),
            "safe closure requires durable Result text"
        );
    }
    match entry {
        Some(Entry::Quit) => assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested)),
        Some(Entry::Close) => assert!(!alive(cx, window), "approved close removes the window"),
        None => {
            assert!(alive(cx, window), "Cmd-W closes the tab, not the host");
            cx.read(|cx| {
                assert!(app.read(cx).tabs.is_empty(), "last tab closes to Welcome");
                assert!(app.read(cx).ui().conflict.is_none());
                assert!(!cx.global::<e2e::QuitForE2e>().requested);
            });
        }
        _ => unreachable!(),
    }
    assert_eq!(
        std::fs::read(repo.join(FILE)).unwrap(),
        original,
        "autosave/close never writes Result into the worktree"
    );
    drop(input);
    drop(conflict);
    cleanup(cx, app, window);
}

fn repository_bytes(repo: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, bytes: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, bytes);
            } else {
                bytes.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut bytes = BTreeMap::new();
    visit(repo, repo, &mut bytes);
    bytes
}

fn assert_refused(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    repo: &Path,
    op: &str,
    announced: bool,
) {
    crate::busy_dismiss::wait(cx, app, "draft flush refuses the operation", |_| {
        read_oplog_tail_for_repo(repo, 1000)
            .iter()
            .any(|entry| entry.op == op && matches!(entry.outcome, OpOutcome::Refused { .. }))
    });
    let entries = read_oplog_tail_for_repo(repo, 1000);
    let receipt = entries.iter().rev().find(|entry| entry.op == op).unwrap();
    let reason = kagi_domain::plan_note::PlanNote::Conflicts(
        kagi_domain::plan_note::ConflictsNote::DraftPreservationFailed,
    );
    assert!(matches!(&receipt.outcome, OpOutcome::Refused { blockers }
        if blockers == &[reason.message_en()]));
    cx.read(|cx| {
        let app = app.read(cx);
        let reason = Msg::AdviceConflictsDraftPreservationFailed.t();
        if announced {
            // The existing result policy deliberately clears the footer when
            // its single presentation owner is a notification.
            assert!(
                matches!(&app.status_footer, FooterStatus::Idle(message) if message.is_empty())
            );
            let stack = app.toast_stack.as_ref().unwrap().read(cx);
            let toast = stack.toasts().last().unwrap();
            assert_eq!(toast.kind, ToastKind::Error);
            let first_sentence = reason.split(['。', '.']).next().unwrap();
            assert!(toast.message.contains(first_sentence), "{}", toast.message);
            assert!(!toast.message.contains('\n'));
            assert!(!toast.message.contains(repo.to_str().unwrap()));
        } else {
            assert!(
                matches!(&app.status_footer, FooterStatus::Failed(message)
                if message.contains(reason)),
                "localized typed blocker must reach the foreground footer"
            );
        }
    });
}

fn run_abort_case(cx: &mut VisualTestAppContext, lang: Lang, at_confirm: bool) {
    let isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    replace_result(cx, &app, window, &input, "OLDER DURABLE DRAFT\n");
    let frozen = if at_confirm {
        click_control(cx, window, "conflict-abort");
        Some(app.update(cx, |app, _| {
            let frozen = app
                .conflict_abort_modal()
                .cloned()
                .expect("real abort plan");
            app.cancel_conflict_abort();
            frozen
        }))
    } else {
        None
    };
    // The permission trap must prevent Abort from substituting this older
    // durable body for the newer live Result, at both plan and confirmation.
    assert_eq!(
        ResolutionBuffer::load(&repo)
            .unwrap()
            .resolved_text(Path::new(FILE))
            .as_deref(),
        Some("OLDER DURABLE DRAFT\n")
    );
    let _denied = UnwritableDirectory::new(isolated.directory.path().join("conflicts"));
    replace_result(cx, &app, window, &input, DRAFT);
    assert_live(cx, &app, window);
    let before = repository_bytes(&repo);
    if let Some(frozen) = frozen {
        // Replay a genuinely computed confirmation at the app boundary after
        // real native editing failed; a visible modal correctly traps typing.
        app.update(cx, |app, cx| {
            app.set_conflict_abort_modal(frozen);
            app.confirm_conflict_abort(cx);
        });
    } else {
        click_control(cx, window, "conflict-abort");
    }
    wait_idle(cx, &app);
    assert_refused(cx, &app, &repo, "merge-abort", true);
    assert_eq!(
        repository_bytes(&repo),
        before,
        "refused Abort is byte-exact no-write"
    );
    assert_live(cx, &app, window);
    assert!(cx.read(|cx| app.read(cx).conflict_abort_modal().is_none()));
    assert!(repo.join(".git/MERGE_HEAD").exists());
    drop(input);
    cleanup(cx, app, window);
}

fn run_skip_case(cx: &mut VisualTestAppContext, lang: Lang, active: bool) {
    let isolated = IsolatedLog::new();
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.set_window_active_for_e2e(active, cx);
    });
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    let _denied = UnwritableDirectory::new(isolated.directory.path().join("conflicts"));
    replace_result(cx, &app, window, &input, DRAFT);
    let before = repository_bytes(&repo);
    app.update(cx, |app, cx| {
        let owner = app
            .app_sessions
            .attachment(app.active_session().unwrap())
            .unwrap();
        app.conflict_skip(owner, cx);
    });
    wait_idle(cx, &app);
    assert_refused(cx, &app, &repo, "rebase-skip", !active);
    assert_eq!(repository_bytes(&repo), before);
    assert_live(cx, &app, window);
    drop(input);
    cleanup(cx, app, window);
}

pub(super) fn queue_result(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    input: &Entity<gpui_component::input::InputState>,
    text: &str,
) {
    cx.update_window(window, |_, window, cx| {
        input.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            cx.emit(gpui_component::input::InputEvent::Change);
        });
    })
    .unwrap();
}

fn run_unflushed_close(
    cx: &mut VisualTestAppContext,
    lang: Lang,
    entry: Option<Entry>,
    concurrent: bool,
) {
    let _isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
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
    let lock = kagi_git::advisory_lock::lock_exclusive(
        &store.with_file_name("buffer.json.lock"),
        std::time::Duration::ZERO,
    )
    .unwrap();
    queue_result(cx, window, &input, DRAFT);
    cx.read(|cx| {
        assert!(conflict_read(&app, cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .has_unflushed_drafts());
    });
    if !concurrent && entry.is_none() {
        app.update(cx, |app, cx| {
            assert!(view.read(cx).draft_persist_failed.is_none());
            let session = app.active_session().unwrap();
            app.open_editor_dirty_guard(kagi::ui::EditorPendingIntent::CloseRepoTab(session), cx);
            app.confirm_editor_dirty_guard(cx);
            assert!(
                app.editor_dirty_guard_modal()
                    .is_some_and(|modal| modal.conflict_discard_armed),
                "unacknowledged healthy drafts also require the second Discard action"
            );
            assert_eq!(
                app.tabs.len(),
                1,
                "first Discard cannot remove the live pane"
            );
            app.cancel_editor_dirty_guard();
        });
    }
    if concurrent {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                let owner = app
                    .app_sessions
                    .attachment(app.active_session().unwrap())
                    .unwrap();
                app.conflict_continue(owner, window, cx);
                // Begin close in the same UI turn, while Continue's barrier
                // is still outstanding. The close awaits its real result.
                app.close_tab(app.active_tab, cx);
            });
        })
        .unwrap();
    } else {
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                if entry.is_some() {
                    app.handle_menu_command("window.close", window, cx);
                } else {
                    app.close_tab(app.active_tab, cx);
                }
            });
        })
        .unwrap();
    }
    e2e::take_conflict_draft_guard();
    // Draw before driving the test executor's blocking background tasks.
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    let progress = e2e::take_conflict_draft_guard().expect("pending flush presents progress");
    assert!(progress.saving);
    assert_eq!(progress.detail, Msg::ConflictDraftSaving.t());
    assert_eq!(progress.title, Msg::ConflictDraftSavingTitle.t());
    assert_eq!(progress.editor_count, 0);
    drop(lock);
    crate::busy_dismiss::wait_ui(cx, &app, "unflushed close completes", |app, cx| {
        app.editor_dirty_guard_modal().is_none() && view.read(cx).draft_autosave_settled_for_e2e()
    });
    let restored = ResolutionBuffer::load(&repo).expect("close must await the real durable writer");
    assert_eq!(
        restored.resolved_text(Path::new(FILE)).as_deref(),
        Some(DRAFT)
    );
    assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
    if entry.is_some() {
        assert!(!alive(cx, window), "successful flush resumes window close");
    } else {
        assert!(
            cx.read(|cx| app.read(cx).tabs.is_empty()),
            "successful flush resumes tab close"
        );
    }
    drop(input);
    cleanup(cx, app, window);
}

pub fn scenario_conflict_draft_flush_boundaries(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for lang in [Lang::En, Lang::Ja] {
        run_unflushed_close(cx, lang, Some(Entry::Close), false);
        run_unflushed_close(cx, lang, None, false);
        run_unflushed_close(cx, lang, None, true);
        let _isolated = IsolatedLog::new();
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.set_lang(lang, cx));
        detect(cx, &app, window);
        let input = result_input(cx, &app, window);
        replace_result(cx, &app, window, &input, DRAFT);
        let before = repository_bytes(&repo);
        cx.update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                let owner = app
                    .app_sessions
                    .attachment(app.active_session().unwrap())
                    .unwrap();
                app.conflict_continue(owner, window, cx);
            });
            input.update(cx, |state, cx| {
                state.set_value("NEWER GENERATION MUST NOT INHERIT APPROVAL\n", window, cx);
                cx.emit(gpui_component::input::InputEvent::Change);
            });
        })
        .unwrap();
        crate::busy_dismiss::wait_ui(cx, &app, "superseded barrier settles", |app, cx| {
            app.ui()
                .conflict
                .as_ref()
                .unwrap()
                .read(cx)
                .draft_autosave_settled_for_e2e()
        });
        paint(cx, &app, window);
        assert!(cx.read(|cx| app.read(cx).conflict_continue_modal().is_none()));
        assert_eq!(
            repository_bytes(&repo),
            before,
            "stale flush cannot stage or continue"
        );
        assert!(
            !read_oplog_tail_for_repo(&repo, 1000)
                .iter()
                .any(|entry| { matches!(entry.outcome, OpOutcome::Refused { .. }) }),
            "a superseded successful flush is not a storage-permission refusal"
        );
        cx.read(|cx| {
            assert_eq!(
                conflict_read(&app, cx)
                    .mode
                    .as_ref()
                    .unwrap()
                    .buffer
                    .resolved_text(Path::new(FILE))
                    .as_deref(),
                Some("NEWER GENERATION MUST NOT INHERIT APPROVAL\n")
            );
        });
        drop(input);
        cleanup(cx, app, window);
    }
}

fn run_mixed_guard(cx: &mut VisualTestAppContext, lang: Lang, entry: Option<Entry>, discard: bool) {
    let isolated = IsolatedLog::new();
    let fixture = content_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    crate::macos::git(&repo, &["merge", "--abort"]);
    let ordinary = repo.join("ordinary.txt");
    std::fs::write(&ordinary, "ordinary original\n").unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.set_lang(lang, cx));
    let editor =
        crate::host_dirty_guard::edit(cx, &app, window, "ordinary.txt", "ordinary unsaved\n");
    assert!(!git_fixture::git_succeeds(&repo, &["merge", "feature"]));
    detect(cx, &app, window);
    let input = result_input(cx, &app, window);
    let denied = UnwritableDirectory::new(isolated.directory.path().join("conflicts"));
    replace_result(cx, &app, window, &input, DRAFT);
    request_close(cx, &app, window, entry);
    assert_guard(cx, &app, window, lang);
    paint(cx, &app, window);
    let presented = e2e::take_conflict_draft_guard().unwrap();
    assert_eq!(
        presented.editor_count, 1,
        "do not count the conflict pane as a file"
    );
    click_control(cx, window, "editor-dirty-guard-discard");
    paint(cx, &app, window);
    let armed = e2e::take_conflict_draft_guard().unwrap();
    assert_eq!(armed.detail, Msg::ConflictDraftMixedDiscardDetail.t());
    assert_eq!(
        armed.editor_count, 1,
        "loss confirmation must still disclose ordinary files"
    );
    assert!(cx.read(|cx| editor.read(cx).dirty));
    assert_eq!(
        std::fs::read_to_string(&ordinary).unwrap(),
        "ordinary original\n"
    );
    if discard {
        click_control(cx, window, "editor-dirty-guard-discard");
    } else {
        denied.restore();
        click_control(cx, window, "editor-dirty-guard-save");
        crate::busy_dismiss::wait_ui(
            cx,
            &app,
            "Retry flushes only conflict decisions",
            |state, cx| {
                state
                    .editor_dirty_guard_modal()
                    .is_some_and(|modal| modal.saving.is_none())
                    && conflict_read(&app, cx).draft_persist_failed.is_none()
                    && !conflict_read(&app, cx)
                        .mode
                        .as_ref()
                        .unwrap()
                        .buffer
                        .has_unflushed_drafts()
            },
        );
        assert!(cx.read(|cx| editor.read(cx).dirty));
        assert_eq!(
            std::fs::read_to_string(&ordinary).unwrap(),
            "ordinary original\n"
        );
        assert!(
            alive(cx, window),
            "Retry must not implicitly save ordinary files and close"
        );
        paint(cx, &app, window);
        assert_eq!(
            kagi::ui::button_style::recorded_modal_button("editor-dirty-guard-save")
                .unwrap()
                .label,
            Msg::EditorHostSaveAll.t()
        );
        click_control(cx, window, "editor-dirty-guard-save");
    }
    crate::busy_dismiss::wait(cx, &app, "mixed guard finishes approved close", |app| {
        app.editor_dirty_guard_modal().is_none()
    });
    if entry.is_some() {
        assert!(!alive(cx, window));
    } else {
        assert!(cx.read(|cx| app.read(cx).tabs.is_empty()));
    }
    assert_eq!(
        std::fs::read_to_string(&ordinary).unwrap(),
        if discard {
            "ordinary original\n"
        } else {
            "ordinary unsaved\n"
        },
        "Ordinary changes require explicit Save All or explicit two-step Discard"
    );
    drop(editor);
    drop(input);
    cleanup(cx, app, window);
}

pub fn scenario_conflict_draft_persist_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for lang in [Lang::En, Lang::Ja] {
        for entry in [Some(Entry::Quit), Some(Entry::Close), None] {
            for discard in [false, true] {
                cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
                run_case(cx, lang, entry, discard);
            }
        }
        for at_confirm in [false, true] {
            run_abort_case(cx, lang, at_confirm);
        }
        for active in [false, true] {
            run_skip_case(cx, lang, active);
        }
        run_mixed_guard(cx, lang, Some(Entry::Close), false);
        run_mixed_guard(cx, lang, Some(Entry::Close), true);
        run_mixed_guard(cx, lang, None, true);
    }
    cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
}
