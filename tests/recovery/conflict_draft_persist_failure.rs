//! #1731: a real autosave I/O failure must not silently lose a live Result draft.
use crate::app_conflict::{click_control, content_fixture};
use crate::conflict_draft_revision::{
    conflict_read, detect, paint, replace_result, result_input, FILE,
};
use crate::host_dirty_guard::{alive, cleanup, request, Entry};
use crate::macos::mount;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{
    e2e,
    i18n::{Lang, Msg},
    KagiApp, ToastKind,
};
use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    resolution::ResolutionBuffer,
};
use std::{
    ffi::OsString,
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const DRAFT: &str = "MANUAL RESULT MUST SURVIVE FAILED AUTOSAVE\n";

struct IsolatedLog {
    previous: Option<OsString>,
    directory: tempfile::TempDir,
}

impl IsolatedLog {
    fn new() -> Self {
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

struct UnwritableDirectory {
    path: PathBuf,
    permissions: Permissions,
}

impl UnwritableDirectory {
    fn new(path: PathBuf) -> Self {
        std::fs::create_dir_all(&path).unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let guard = Self { path, permissions };
        std::fs::set_permissions(&guard.path, Permissions::from_mode(0o000)).unwrap();
        guard
    }

    fn restore(&self) {
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
    let (title, retry, discard) =
        e2e::take_conflict_draft_guard().expect("production renderer selected failure labels");
    assert_eq!(
        title,
        match lang {
            Lang::En => "Draft could not be saved",
            Lang::Ja => "下書きを保存できていません",
        }
    );
    assert_eq!(retry, Msg::ConflictDraftRetry.t());
    assert_eq!(discard, Msg::ConflictDraftDiscardClose.t());
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

fn run_case(cx: &mut VisualTestAppContext, lang: Lang, entry: Entry, discard: bool) {
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
    replace_result(cx, &app, window, &input, DRAFT);
    assert_live(cx, &app, window);
    assert!(
        failed_receipts(&repo) > before,
        "autosave failure must be durable in oplog"
    );
    assert_error_toast(cx, &app, &repo);

    request(cx, &app, window, entry);
    assert_guard(cx, &app, window, lang);
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone().unwrap());
    click_control(cx, window, "editor-dirty-guard-cancel");
    assert!(cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()));
    assert_live(cx, &app, window);
    request(cx, &app, window, entry);
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
        click_control(cx, window, "editor-dirty-guard-save");
        assert_guard(cx, &app, window, lang);
        assert!(
            failed_receipts(&repo) > before_retry,
            "Retry must attempt real persistence again"
        );
        assert_error_toast(cx, &app, &repo);
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
        Entry::Quit => assert!(cx.read(|cx| cx.global::<e2e::QuitForE2e>().requested)),
        Entry::Close => assert!(!alive(cx, window), "approved close removes the window"),
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

pub fn scenario_conflict_draft_persist_failure(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    cx.update(|cx| cx.set_global(e2e::QuitForE2e::default()));
    for lang in [Lang::En, Lang::Ja] {
        for entry in [Entry::Quit, Entry::Close] {
            for discard in [false, true] {
                cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
                run_case(cx, lang, entry, discard);
            }
        }
    }
    cx.update(|cx| cx.global_mut::<e2e::QuitForE2e>().requested = false);
}
