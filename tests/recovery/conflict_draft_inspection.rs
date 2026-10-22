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
use std::path::{Path, PathBuf};

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
        Self { previous, _directory: directory }
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
    }).unwrap();
    cx.run_until_parked();
}

fn panel(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Entity<kagi::ui::oplog_panel::OpLogPanel> {
    cx.read(|cx| app.read(cx).op_log.clone().unwrap())
}

fn show_log(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |app, cx| {
        app.bottom_panel_open = true;
        app.bottom_tab = BottomTab::OperationLog;
        app.bottom_panel_height = 520.;
        cx.notify();
    });
    paint(cx, app, window);
}

fn draft_text(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<String> {
    cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx)
        .preserved_conflict_draft_text().map(str::to_owned))
}

fn open_drafts(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    expected: &str,
) -> String {
    click_control(cx, window, "oplog-preserved-drafts-open");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if draft_text(cx, app).is_some() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "explicit preserved-draft inspection completes");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    paint(cx, app, window);
    let text = draft_text(cx, app).expect("the explicit draft surface loaded");
    assert!(text.contains(expected), "preserved body is inspectable: {text}");
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
    let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();
    assert_eq!(copied, text, "only explicit draft Copy copies the preservation snapshot");
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
    let row = cx.read(|cx| panel.read(cx).entries().iter().position(|entry| {
        entry.op == "rebase-continue"
            && kagi::ui::oplog_panel::entry_worktree(entry) == repo.to_string_lossy()
    }));
    let Some(row) = row else { return None; };
    let expected = cx.read(|cx| entry_clipboard_text(&panel.read(cx).entries()[row]));
    cx.read(|cx| panel.read(cx).scroll_handle().scroll_to_reveal_item(row));
    paint(cx, app, window);
    if cx.read(|cx| panel.read(cx).expanded()) != Some(row) {
        let summary = cx.read(|cx| panel.read(cx).scroll_handle().bounds_for_item(row))
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
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("before-receipt-copy".into()));
    cx.simulate_mouse_move(window, copy.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(window, copy.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();
    assert_eq!(copied, expected, "ordinary receipt Copy keeps its exact existing contract");
    assert!(!copied.contains(FIRST));
    assert!(!copied.contains(OTHER));
    Some(copied)
}

pub(super) fn inspect_saved_recovery(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    window: AnyWindowHandle,
    repo: &Path,
    expected: &str,
) {
    let before = RepositoryState::read(repo);
    let presentation = cx.read(|cx| {
        let state = app.read(cx);
        (state.bottom_panel_open, state.bottom_tab, state.bottom_panel_height)
    });
    show_log(cx, app, window);
    let receipt = copy_receipt(cx, app, window, repo);
    let selected = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded());
    let scroll = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx)
        .scroll_handle().logical_scroll_top());
    let text = open_drafts(cx, app, window, expected);
    copy_drafts(cx, app, window, &text);
    back(cx, app, window);
    assert_eq!(cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded()), selected,
        "Back preserves the ordinary receipt selection");
    let after_scroll = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx)
        .scroll_handle().logical_scroll_top());
    assert_eq!(after_scroll.item_ix, scroll.item_ix);
    assert_eq!(after_scroll.offset_in_item, scroll.offset_in_item);
    assert_eq!(copy_receipt(cx, app, window, repo), receipt);
    assert_eq!(before, RepositoryState::read(repo), "inspection and both Copy actions are read-only");
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
    let mut document: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for file in document["files"].as_array_mut().unwrap() {
        file.as_object_mut().unwrap().remove("identity");
    }
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    path
}

fn assert_empty_log(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    assert_eq!(cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).len()), 0);
    assert!(kagi_git::oplog::read_oplog_tail(100).is_empty(), "there is no fabricated durable receipt");
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
    cx.read(|cx| app.read(cx).ui().conflict.as_ref().unwrap().read(cx)
        .mode.as_ref().unwrap().buffer.autosave().unwrap());
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

fn assert_scope_cleared(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    e2e::clear_control_bounds(window.window_id(), "oplog-preserved-drafts-copy");
    paint(cx, app, window);
    assert_eq!(draft_text(cx, app), None);
    assert!(e2e::control_bounds(window.window_id(), "oplog-preserved-drafts-copy").is_none());
}

pub fn scenario_conflict_draft_inspection_scope_change(cx: &mut VisualTestAppContext) {
    let _isolated = IsolatedLog::new();
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
    show_log(cx, &app, window);
    let receipt = copy_receipt(cx, &app, window, &repo_a).unwrap();
    let selected = cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded());
    // Change Attachment without waiting for A's requested snapshot.
    click_control(cx, window, "oplog-preserved-drafts-open");
    app.update(cx, |app, cx| assert!(app.open_repository(repo_b.clone(), cx)));
    crate::busy_dismiss::ready(cx, &app);
    assert_scope_cleared(cx, &app, window);
    let text_b = open_drafts(cx, &app, window, OTHER);
    assert!(!text_b.contains(FIRST));
    copy_drafts(cx, &app, window, &text_b);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    crate::busy_dismiss::ready(cx, &app);
    assert_scope_cleared(cx, &app, window);
    let text_a = open_drafts(cx, &app, window, FIRST);
    assert!(!text_a.contains(OTHER));
    copy_drafts(cx, &app, window, &text_a);
    back(cx, &app, window);
    assert_eq!(cx.read(|cx| app.read(cx).op_log.as_ref().unwrap().read(cx).expanded()), selected);
    assert_eq!(copy_receipt(cx, &app, window, &repo_a), Some(receipt));
    assert_unresolved(cx, &app);
    assert_eq!(before_a, RepositoryState::read(&repo_a));
    assert_eq!(before_b, RepositoryState::read(&repo_b));
    unmount(cx, app, window);
}
