//! #1721: preserved drafts are inspectable independently of receipts, never applied.
use crate::app_conflict::{click_control, content_fixture};
use crate::conflict_draft_revision::{
    advance_first, assert_unresolved, detect, RebaseFixture, FILE, FIRST,
};
use crate::macos::{mount, unmount};
use crate::repository_state::RepositoryState;
use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, oplog_panel::entry_clipboard_text, BottomTab, KagiApp};
use kagi_git::resolution::ResolutionBuffer;
use kagi_ui_core::i18n::Lang;
use std::path::{Path, PathBuf};

#[path = "../support/git_fixture.rs"]
mod git_fixture;

#[path = "conflict_draft_inspection_hold.rs"]
mod inspection_hold;

#[path = "conflict_draft_continue_admission.rs"]
mod continue_admission;
pub use continue_admission::scenario_conflict_draft_continue_restore_admission;

#[path = "conflict_draft_stale_failure.rs"]
mod stale_failure;
pub use stale_failure::{
    scenario_conflict_draft_fresh_restore_failure, scenario_conflict_draft_stale_restore_async,
    scenario_conflict_draft_stale_restore_failure, scenario_conflict_draft_stale_restore_success,
};

#[path = "conflict_draft_restore_io.rs"]
mod restore_io;
pub use restore_io::scenario_conflict_draft_restore_io_failure;

const OTHER: &str = "OTHER REPOSITORY PRESERVED RESULT\n";

// Like the existing theme_custom sandbox: restore the runner-owned environment
// only after every scenario window/task has been unmounted and drained.
struct IsolatedLog {
    previous: Option<std::ffi::OsString>,
    _directory: tempfile::TempDir,
}

impl IsolatedLog {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", directory.path());
        Self {
            previous,
            _directory: directory,
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

fn paint(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
    cx.run_until_parked();
}

fn panel(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> Entity<kagi::ui::oplog_panel::OpLogPanel> {
    cx.read(|cx| app.read(cx).op_log.clone().unwrap())
}

pub(super) fn show_log(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    if cx.read(|cx| app.read(cx).bottom_panel_open) {
        cx.simulate_keystrokes(window, "cmd-j");
        paint(cx, app, window);
        assert!(!cx.read(|cx| app.read(cx).bottom_panel_open));
    }
    app.update(cx, |app, cx| {
        // Only arrange panel selection/size; opening uses the real root action.
        app.bottom_tab = BottomTab::OperationLog;
        app.bottom_panel_height = 520.;
        cx.notify();
    });
    cx.simulate_keystrokes(window, "cmd-j");
    paint(cx, app, window);
    assert!(cx.read(|cx| app.read(cx).bottom_panel_open));
    assert!(
        e2e::control_bounds(window.window_id(), "bottom-panel").is_some(),
        "the window-wide panel is actually laid out while Conflict Mode is active"
    );
}

fn draft_text(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<String> {
    cx.read(|cx| {
        app.read(cx)
            .op_log
            .as_ref()
            .unwrap()
            .read(cx)
            .preserved_conflict_draft_text()
            .map(|text| text.to_string())
    })
}

pub(super) fn open_drafts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &str,
) -> String {
    click_control(cx, window, "oplog-preserved-drafts-open");
    wait_drafts(cx, app, window, expected)
}

fn wait_drafts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &str,
) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if draft_text(cx, app).is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "explicit preserved-draft inspection completes"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    paint(cx, app, window);
    let text = draft_text(cx, app).expect("the explicit draft surface loaded");
    assert!(
        text.contains(expected),
        "preserved body is inspectable: {text}"
    );
    text
}

fn copy_drafts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    text: &str,
) {
    let before = draft_text(cx, app);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before-draft-copy".into()));
    click_control(cx, window, "oplog-preserved-drafts-copy");
    cx.run_until_parked();
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap();
    assert_eq!(
        copied, text,
        "only explicit draft Copy copies the preservation snapshot"
    );
    assert_eq!(draft_text(cx, app), before);
}

fn back(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    click_control(cx, window, "oplog-preserved-drafts-back");
    paint(cx, app, window);
    assert_eq!(draft_text(cx, app), None);
}

fn copy_receipt(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
) -> Option<String> {
    let panel = panel(cx, app);
    let row = cx.read(|cx| {
        panel.read(cx).entries().iter().position(|entry| {
            entry.op == "rebase-continue"
                && kagi::ui::oplog_panel::entry_worktree(entry) == repo.to_string_lossy()
        })
    });
    let Some(row) = row else {
        return None;
    };
    let expected = cx.read(|cx| entry_clipboard_text(&panel.read(cx).entries()[row]));
    cx.read(|cx| panel.read(cx).scroll_handle().scroll_to_reveal_item(row));
    paint(cx, app, window);
    if cx.read(|cx| panel.read(cx).expanded()) != Some(row) {
        let summary = cx
            .read(|cx| panel.read(cx).scroll_handle().bounds_for_item(row))
            .expect("the real receipt summary is laid out");
        let position = summary.origin + gpui::point(gpui::px(40.), gpui::px(8.));
        cx.simulate_mouse_move(window, position, None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(window, position, gpui::Modifiers::none());
        paint(cx, app, window);
    }
    assert_eq!(cx.read(|cx| panel.read(cx).expanded()), Some(row));
    let copy = e2e::control_bounds(window.window_id(), &format!("oplog-copy-{row}"))
        .expect("the ordinary receipt Copy control is laid out");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
        "before-receipt-copy".into(),
    ));
    cx.simulate_mouse_move(window, copy.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap();
    assert_eq!(
        copied, expected,
        "ordinary receipt Copy keeps its exact existing contract"
    );
    assert!(!copied.contains(FIRST));
    assert!(!copied.contains(OTHER));
    Some(copied)
}

fn inspection_labels(text: &str, path: &Path, identity: Option<&str>, body: &str) -> Vec<String> {
    let (heading, document) = text.split_once("\n\n").unwrap();
    let mut fields = document.splitn(3, '\n');
    let (file_label, displayed_path) = fields.next().unwrap().split_once(": ").unwrap();
    let (identity_label, displayed_identity) = fields.next().unwrap().split_once(": ").unwrap();
    assert_eq!(displayed_path, path.to_string_lossy());
    assert_eq!(
        fields.next().unwrap(),
        body,
        "raw draft body stays byte-exact"
    );
    let mut labels = vec![
        heading.to_owned(),
        file_label.to_owned(),
        identity_label.to_owned(),
    ];
    if let Some(identity) = identity {
        assert_eq!(
            displayed_identity, identity,
            "bound draft identity stays exact"
        );
    } else {
        labels.push(displayed_identity.to_owned());
    }
    labels
}

fn switch_inspection_language(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    language: Lang,
) {
    // This is the same command used by View → Language, not a translation probe.
    app.update(cx, |app, cx| app.set_lang(language, cx));
    paint(cx, app, window);
    assert!(e2e::control_bounds(window.window_id(), "oplog-preserved-drafts-back").is_some());
    assert!(
        draft_text(cx, app).is_some(),
        "language changes keep the loaded inspection open"
    );
}

fn copy_inspection_languages(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    expected: &str,
) {
    let buffer = kagi_git::Backend::open(repo)
        .unwrap()
        .resolution_buffer_from_repo_with_autosave()
        .unwrap();
    let drafts: Vec<_> = buffer.recovery_drafts().collect();
    assert_eq!(drafts.len(), 1, "this fixture has one preserved draft");
    let draft = drafts[0];
    assert_eq!(
        draft
            .result
            .map(kagi_git::resolution::lines_to_text)
            .as_deref(),
        Some(expected)
    );
    assert!(draft.raw_result.is_none());
    let identity = draft.identity.map(|identity| identity.as_str());
    // The accepted real read completed in EN but was held until the UI is JA.
    let japanese_text = inspection_hold::pending_language_drafts(cx, app, window, expected);
    copy_drafts(cx, app, window, &japanese_text);
    let japanese = inspection_labels(&japanese_text, draft.path, identity, expected);
    switch_inspection_language(cx, app, window, Lang::En);
    let text = draft_text(cx, app).unwrap();
    let english = inspection_labels(&text, draft.path, identity, expected);
    assert_ne!(japanese_text, text);
    assert_eq!(japanese.len(), english.len());
    for (translated, original) in japanese.iter().zip(&english) {
        assert_ne!(
            translated, original,
            "each user-facing label follows the new language"
        );
    }
    copy_drafts(cx, app, window, &text);
    switch_inspection_language(cx, app, window, Lang::Ja);
    assert_eq!(draft_text(cx, app).as_deref(), Some(japanese_text.as_str()));
    copy_drafts(cx, app, window, &japanese_text);
    switch_inspection_language(cx, app, window, Lang::En);
    assert_eq!(draft_text(cx, app).as_deref(), Some(text.as_str()));
    copy_drafts(cx, app, window, &text);
}

pub(super) fn inspect_saved_recovery(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    expected: &str,
) {
    let before = RepositoryState::read(repo);
    let _presentation = crate::presentation_support::Presentation::keep();
    let draft_directory =
        PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap()).join("conflicts");
    let saved_drafts = git_files(&draft_directory);
    app.update(cx, |app, cx| app.set_lang(Lang::En, cx));
    let presentation = cx.read(|cx| {
        let state = app.read(cx);
        (
            state.bottom_panel_open,
            state.bottom_tab,
            state.bottom_panel_height,
        )
    });
    show_log(cx, app, window);
    let receipt = copy_receipt(cx, app, window, repo);
    let selected = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded());
    let scroll = cx.read(|cx| {
        app.read(cx)
            .op_log
            .as_ref()
            .unwrap()
            .read(cx)
            .scroll_handle()
            .logical_scroll_top()
    });
    copy_inspection_languages(cx, app, window, repo, expected);
    back(cx, app, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded()),
        selected,
        "Back preserves the ordinary receipt selection"
    );
    let after_scroll = cx.read(|cx| {
        app.read(cx)
            .op_log
            .as_ref()
            .unwrap()
            .read(cx)
            .scroll_handle()
            .logical_scroll_top()
    });
    assert_eq!(after_scroll.item_ix, scroll.item_ix);
    assert_eq!(after_scroll.offset_in_item, scroll.offset_in_item);
    assert_eq!(copy_receipt(cx, app, window, repo), receipt);
    app.update(cx, |app, cx| app.set_lang(Lang::Ja, cx));
    paint(cx, app, window);
    assert_eq!(
        copy_receipt(cx, app, window, repo).is_some(),
        receipt.is_some()
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded()),
        selected
    );
    app.update(cx, |app, cx| app.set_lang(Lang::En, cx));
    paint(cx, app, window);
    assert_eq!(copy_receipt(cx, app, window, repo), receipt);
    assert_eq!(
        git_files(&draft_directory),
        saved_drafts,
        "inspection, language changes and Copy never rewrite the durable draft"
    );
    assert_eq!(
        before,
        RepositoryState::read(repo),
        "inspection and both Copy actions are read-only"
    );
    app.update(cx, |app, cx| {
        app.bottom_panel_open = presentation.0;
        app.bottom_tab = presentation.1;
        app.bottom_panel_height = presentation.2;
        cx.notify();
    });
    paint(cx, app, window);
    assert_unresolved(cx, app);
}

fn legacy_document(repo: &Path, text: &str) -> PathBuf {
    let repository = git2::Repository::open(repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repository).unwrap();
    buffer.set_manual_text(Path::new(FILE), text).unwrap();
    let path = buffer.autosave().unwrap();
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for file in document["files"].as_array_mut().unwrap() {
        file.as_object_mut().unwrap().remove("identity");
    }
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    path
}

fn assert_empty_log(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    assert_eq!(
        cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).len()),
        0
    );
    assert!(
        kagi_git::oplog::read_oplog_tail(100).is_empty(),
        "there is no fabricated durable receipt"
    );
}

pub fn scenario_conflict_draft_corrupt_restore(cx: &mut VisualTestAppContext) {
    let _presentation = crate::presentation_support::Presentation::keep();
    for language in [Lang::En, Lang::Ja] {
        let _isolated = IsolatedLog::new();
        let fixture = content_fixture();
        let repo = fixture.path().canonicalize().unwrap();
        let (app, window) = mount(cx, &repo);
        app.update(cx, |app, cx| app.set_lang(language, cx));
        let backend = kagi_git::Backend::open(&repo).unwrap();
        let store = backend
            .resolution_buffer_from_repo()
            .unwrap()
            .autosave()
            .unwrap();
        std::fs::remove_file(store.with_file_name("buffer.json.prev")).unwrap();
        let torn = b"{\"files\":[damaged legacy draft";
        std::fs::write(&store, torn).unwrap();
        let before = RepositoryState::read(&repo);
        detect(cx, &app, window);
        let aside = store.with_file_name("buffer.json.corrupt");
        assert_eq!(std::fs::read(&aside).unwrap(), torn);
        assert_unresolved(cx, &app);
        cx.read(|cx| {
            let state = app.read(cx);
            let toasts = state.toast_stack.as_ref().unwrap().read(cx).toasts();
            let message = kagi_ui_core::i18n::Msg::conflict_draft_rescued("buffer.json.corrupt");
            assert!(
                toasts.iter().any(|toast| {
                    toast.kind == kagi::ui::ToastKind::Error && toast.message.as_ref() == message
                }),
                "a damaged draft must announce its preserved file in {language:?}"
            );
        });
        let entries = kagi_git::oplog::read_oplog_tail(100);
        let rescues: Vec<_> = entries
            .iter()
            .filter(|entry| entry.op == "conflict-draft-restore")
            .collect();
        assert_eq!(rescues.len(), 1);
        assert!(
            matches!(&rescues[0].outcome, kagi_git::oplog::OpOutcome::Failed { error }
            if error == &format!("Draft could not be read; the damaged file was moved aside as {}", aside.display()))
        );
        let input = crate::conflict_draft_revision::result_input(cx, &app, window);
        crate::conflict_draft_revision::replace_result(cx, &app, window, &input, FIRST);
        assert_eq!(
            ResolutionBuffer::load(&repo)
                .unwrap()
                .resolved_text(Path::new(FILE))
                .as_deref(),
            Some(FIRST)
        );
        assert_eq!(
            before.with_appended_oplog(rescues[0]),
            RepositoryState::read(&repo),
            "quarantine may append exactly its asserted receipt, but may not mutate Git"
        );
        drop(input);
        unmount(cx, app, window);
    }
}
pub fn scenario_conflict_draft_inspection_empty_legacy(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
    let fixture = content_fixture(); // Real CLI merge: no Kagi operation or receipt.
    let repo = fixture.path().canonicalize().unwrap();
    legacy_document(&repo, FIRST);
    let before = RepositoryState::read(&repo);
    for _ in 0..2 {
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        assert_empty_log(cx, &app);
        assert_unresolved(cx, &app);
        inspect_saved_recovery(cx, &app, window, &repo, FIRST);
        assert_empty_log(cx, &app);
        assert_eq!(before, RepositoryState::read(&repo));
        unmount(cx, app, window);
    }
}

fn save_live_buffer(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    cx.read(|cx| {
        app.read(cx)
            .ui()
            .conflict
            .as_ref()
            .unwrap()
            .read(cx)
            .mode
            .as_ref()
            .unwrap()
            .buffer
            .autosave()
            .unwrap()
    });
}

pub fn scenario_conflict_draft_inspection_unrecorded_restart(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
    let fixture = RebaseFixture::new();
    let repo = fixture.path();
    let (app, window) = mount(cx, &repo);
    detect(cx, &app, window);
    advance_first(cx, &app, window, &fixture, true);
    save_live_buffer(cx, &app);
    assert!(kagi_git::oplog::read_oplog_tail(100).is_empty());
    unmount(cx, app, window);
    let before = RepositoryState::read(&repo);
    for _ in 0..2 {
        let (app, window) = mount(cx, &repo);
        detect(cx, &app, window);
        assert_empty_log(cx, &app);
        inspect_saved_recovery(cx, &app, window, &repo, FIRST);
        assert_empty_log(cx, &app);
        assert_eq!(before, RepositoryState::read(&repo));
        unmount(cx, app, window);
    }
}

fn assert_scope_cleared(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
) {
    e2e::clear_control_bounds(window.window_id(), "oplog-preserved-drafts-copy");
    paint(cx, app, window);
    assert_eq!(draft_text(cx, app), None);
    assert!(e2e::control_bounds(window.window_id(), "oplog-preserved-drafts-copy").is_none());
}

pub fn scenario_conflict_draft_inspection_scope_change(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
    let _presentation = crate::presentation_support::Presentation::keep();
    let first = RebaseFixture::new();
    let repo_a = first.path();
    let second = content_fixture();
    let repo_b = second.path().canonicalize().unwrap();
    legacy_document(&repo_b, OTHER);
    let (app, window) = mount(cx, &repo_a);
    detect(cx, &app, window);
    advance_first(cx, &app, window, &first, false);
    save_live_buffer(cx, &app);
    let before_a = RepositoryState::read(&repo_a);
    let before_b = RepositoryState::read(&repo_b);
    app.update(cx, |app, cx| app.set_lang(Lang::En, cx));
    show_log(cx, &app, window);
    let receipt = copy_receipt(cx, &app, window, &repo_a).unwrap();
    let selected = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded());
    let old = inspection_hold::LoadHold::new(cx);
    // Change Attachment without waiting for A's requested snapshot.
    click_control(cx, window, "oplog-preserved-drafts-open");
    old.wait(cx, &app);
    for language in [Lang::Ja, Lang::En] {
        app.update(cx, |app, cx| app.set_lang(language, cx));
        paint(cx, &app, window);
        assert_eq!(draft_text(cx, &app), None);
    }
    app.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx))
    });
    crate::busy_dismiss::ready(cx, &app);
    assert_scope_cleared(cx, &app, window);
    let text_b = open_drafts(cx, &app, window, OTHER);
    assert!(!text_b.contains(FIRST));
    copy_drafts(cx, &app, window, &text_b);
    drop(old);
    cx.run_until_parked();
    paint(cx, &app, window);
    assert_eq!(draft_text(cx, &app).as_deref(), Some(text_b.as_str()));
    copy_drafts(cx, &app, window, &text_b);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    crate::busy_dismiss::ready(cx, &app);
    assert_scope_cleared(cx, &app, window);
    let text_a = open_drafts(cx, &app, window, FIRST);
    assert!(!text_a.contains(OTHER));
    copy_drafts(cx, &app, window, &text_a);
    back(cx, &app, window);
    assert_eq!(
        cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded()),
        selected
    );
    assert_eq!(copy_receipt(cx, &app, window, &repo_a), Some(receipt));
    assert_unresolved(cx, &app);
    assert_eq!(before_a, RepositoryState::read(&repo_a));
    assert_eq!(before_b, RepositoryState::read(&repo_b));
    unmount(cx, app, window);
}

fn attachment(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> kagi::app::Attachment {
    cx.read(|cx| {
        let app = app.read(cx);
        app.app_sessions
            .attachment(app.active_session().unwrap())
            .unwrap()
    })
}

fn git_files(directory: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    git_fixture::worktree_files(
        directory,
        git_fixture::WorktreeFilesOptions {
            skip_nested_git: false,
            follow_directory_symlinks: false,
        },
    )
}

/// Only test-owned temporary repositories are redirected; restore A even on panic.
struct GitdirSwap {
    locator: PathBuf,
    saved: PathBuf,
    _directory: tempfile::TempDir,
}

impl GitdirSwap {
    fn new(repo_a: &Path, repo_b: &Path) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let locator = repo_a.join(".git");
        let saved = directory.path().join("original.git");
        std::fs::rename(&locator, &saved).unwrap();
        let swap = Self {
            locator,
            saved,
            _directory: directory,
        };
        std::fs::write(
            &swap.locator,
            format!("gitdir: {}\n", repo_b.join(".git").display()),
        )
        .unwrap();
        swap
    }
}

impl Drop for GitdirSwap {
    fn drop(&mut self) {
        std::fs::remove_file(&self.locator).unwrap();
        std::fs::rename(&self.saved, &self.locator).unwrap();
    }
}

fn inspect_refused(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    click_control(cx, window, "oplog-preserved-drafts-open");
    let panel = panel(cx, app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        assert_eq!(
            draft_text(cx, app),
            None,
            "the substituted repository body is never admitted"
        );
        if cx.read(|cx| panel.read(cx).preserved_conflict_draft_unavailable()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "inspection settles as Unavailable, not Loading"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    e2e::clear_control_bounds(window.window_id(), "oplog-preserved-drafts-copy");
    paint(cx, app, window);
    assert!(e2e::control_bounds(window.window_id(), "oplog-preserved-drafts-copy").is_some());
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
        "identity-refusal-sentinel".into(),
    ));
    // Exercise the rendered disabled Button, not the root's Copy method.
    click_control(cx, window, "oplog-preserved-drafts-copy");
    cx.run_until_parked();
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("identity-refusal-sentinel")
    );
    assert_eq!(draft_text(cx, app), None);
    assert!(cx.read(|cx| panel.read(cx).preserved_conflict_draft_unavailable()));
}

pub fn scenario_conflict_draft_inspection_gitdir_replaced(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
    let first = content_fixture();
    let second = content_fixture();
    let repo_a = first.path().canonicalize().unwrap();
    let repo_b = second.path().canonicalize().unwrap();
    // B's actual gitdir owns B's worktree and therefore B's autosave key,
    // even when reached through the replacement gitfile at A's old locator.
    git_fixture::git(
        &repo_b,
        &["config", "core.worktree", repo_b.to_str().unwrap()],
    );
    let draft_a = legacy_document(&repo_a, FIRST);
    let draft_b = legacy_document(&repo_b, OTHER);
    let (app, window) = mount(cx, &repo_a);
    detect(cx, &app, window);
    show_log(cx, &app, window);
    let owner = attachment(cx, &app);
    assert_eq!(owner.path, repo_a);
    assert_eq!(
        owner.worktree,
        Some(
            kagi_git::Backend::open(&repo_a)
                .unwrap()
                .write_worktree_id()
                .unwrap()
        )
    );
    let valid = open_drafts(cx, &app, window, FIRST);
    assert!(!valid.contains(OTHER));
    copy_drafts(cx, &app, window, &valid);
    back(cx, &app, window);
    let before_a = RepositoryState::read(&repo_a);
    let before_b = RepositoryState::read(&repo_b);
    let git_a = git_files(&repo_a.join(".git"));
    let git_b = git_files(&repo_b.join(".git"));
    let saved_drafts = [draft_a, draft_b].map(|path| {
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    });

    let swap = GitdirSwap::new(&repo_a, &repo_b);
    let pointer = std::fs::read(&swap.locator).unwrap();
    let actual = kagi_git::Backend::open(&owner.path).unwrap();
    let identity_b = kagi_git::Backend::open(&repo_b)
        .unwrap()
        .write_worktree_id()
        .unwrap();
    assert_eq!(actual.write_worktree_id().unwrap(), identity_b);
    assert_ne!(owner.worktree.as_ref(), Some(&identity_b));
    // Without the load-time identity gate this exact backend would expose B.
    let buffer = actual.resolution_buffer_from_repo_with_autosave().unwrap();
    let bodies: Vec<_> = buffer
        .recovery_drafts()
        .filter_map(|draft| draft.result.map(kagi_git::resolution::lines_to_text))
        .collect();
    assert_eq!(bodies, vec![OTHER.to_string()]);
    drop(buffer);
    drop(actual);
    assert_eq!(
        attachment(cx, &app),
        owner,
        "the root's frozen owner is deliberately untouched"
    );

    inspect_refused(cx, &app, window);
    assert_eq!(
        attachment(cx, &app),
        owner,
        "refusal does not silently reattach A to B"
    );
    unmount(cx, app, window);
    assert_eq!(std::fs::read(&swap.locator).unwrap(), pointer);
    assert_eq!(
        git_files(&swap.saved),
        git_a,
        "A's original admin bytes remain intact"
    );
    assert_eq!(
        git_files(&repo_b.join(".git")),
        git_b,
        "B's admin bytes remain intact"
    );
    assert_eq!(RepositoryState::read(&repo_b), before_b);
    drop(swap);
    assert_eq!(git_files(&repo_a.join(".git")), git_a);
    assert_eq!(RepositoryState::read(&repo_a), before_a);
    for (path, bytes) in saved_drafts {
        assert_eq!(
            std::fs::read(path).unwrap(),
            bytes,
            "preserved draft bytes remain unchanged"
        );
    }
}
