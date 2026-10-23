//! #1187: selection survives a row click, hover, navigation and resolution.
use crate::app_conflict::click_control;
use crate::macos::{git, mount, repo_fingerprint, unmount};
use gpui::{AnyWindowHandle, VisualTestAppContext};
use kagi::ui::{e2e, list_a11y};
use kagi_ui_core::theme;
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    for (branch, content) in [
        ("main", "base"),
        ("feature", "incoming"),
        ("main", "current"),
    ] {
        if content == "incoming" {
            git(repo, &["checkout", "-qb", branch]);
        } else if content == "current" {
            git(repo, &["checkout", "-q", branch]);
        }
        std::fs::write(repo.join("blob.bin"), format!("\0{content}\n")).unwrap();
        for file in ["notes.txt", "zeta.txt"] {
            std::fs::write(repo.join(file), format!("{content}\n")).unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-qm", content]);
    }
    let result = std::process::Command::new("git")
        .args(["merge", "feature"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(!result.status.success());
    fixture
}

fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle, row: usize) -> [u8; 4] {
    cx.run_until_parked();
    list_a11y::clear_recorded_lists();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
        let bounds = e2e::control_bounds(
            window.window_handle().window_id(),
            &format!("conflict-row-{row}"),
        )
        .expect("file row measured");
        let image = window.render_to_image().expect("native row fill capture");
        let scale = window.scale_factor();
        // Window-local logical coordinates: the left padding at x+4 is before
        // the disclosure/document icons; mid-height avoids the rounded corners.
        // Convert once to native image pixels. The old right-5/top+5 hit badges.
        image
            .get_pixel(
                ((f32::from(bounds.left()) + 4.) * scale) as u32,
                (f32::from(bounds.center().y) * scale) as u32,
            )
            .0
    })
    .unwrap()
}

fn selected_row(list: &str, path: &str) {
    let list = list_a11y::recorded_list(list).expect("named conflict file list");
    assert_eq!(list.role, Some(gpui::Role::ListBox));
    assert_eq!(
        list.rows.values().filter(|(_, selected)| *selected).count(),
        1
    );
    assert!(list
        .rows
        .values()
        .any(|(label, selected)| label == path && *selected));
}

fn chrome_contract(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    last_pending: usize,
    saved: bool,
) {
    let ids = [
        "conflict-pending-header",
        "conflict-saved-header",
        "conflict-save",
        "conflict-next-file",
        "conflict-file-header",
    ];
    for id in ids {
        e2e::clear_control_bounds(window.window_id(), id);
    }
    paint(cx, window, 0);
    let bounds = |id: &str| e2e::control_bounds(window.window_id(), id).unwrap();
    let pending = bounds("conflict-pending-header");
    let last = bounds(&format!("conflict-row-{last_pending}"));
    let saved_header = bounds("conflict-saved-header");
    assert!(pending.bottom() <= last.top());
    assert!(
        saved_header.top() >= last.bottom()
            && saved_header.top() - last.bottom() <= theme::scaled_px(20.),
        "Saved follows pending rows, not a bottom-pinned section"
    );
    let action = if saved {
        "conflict-next-file"
    } else {
        "conflict-save"
    };
    let absent = if saved {
        "conflict-save"
    } else {
        "conflict-next-file"
    };
    let header = bounds("conflict-file-header");
    let action_bounds = bounds(action);
    assert!(action_bounds.left() >= header.left() && action_bounds.right() <= header.right());
    assert!(action_bounds.top() >= header.top() && action_bounds.bottom() <= header.bottom());
    assert!(
        e2e::control_bounds(window.window_id(), absent).is_none(),
        "one file action only"
    );
}

fn hidden_count(cx: &mut VisualTestAppContext, window: AnyWindowHandle, expected: Option<usize>) {
    let id = "conflict-sidebar-inline-count";
    e2e::clear_control_bounds(window.window_id(), id);
    crate::native_ax::activate(cx);
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .unwrap();
    let count = e2e::control_bounds(window.window_id(), id);
    assert_eq!(
        count.is_some(),
        expected.is_some(),
        "count is inline only while the sidebar is hidden"
    );
    if let Some(count) = count {
        let toggle = e2e::control_bounds(window.window_id(), "conflict-sidebar-toggle").unwrap();
        assert!(count.left() >= toggle.left() && count.right() <= toggle.right());
        assert!(count.top() >= toggle.top() && count.bottom() <= toggle.bottom());
        let tree = crate::native_ax::tree(cx, window);
        let expected = expected.unwrap().to_string();
        assert!(
            tree["nodes"].as_object().unwrap().values().any(|node| {
                node["aria"]["role"] == "Status" && node["aria"]["label"] == expected
            }),
            "inline count reports actual remaining files: {expected}"
        );
    }
}

pub fn scenario_conflict_file_selection(cx: &mut VisualTestAppContext) {
    use kagi::ui::conflict_view::ConflictPage;
    let _saved = crate::gui_isolation::SavedKeys::keep(&["theme"]);
    let old_theme = theme::theme().slug.to_string();
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    let (app, window) = mount(cx, fixture.path());
    app.update(cx, |app, cx| {
        app.set_theme("apple-light", cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    let conflict = cx.read(|cx| app.read(cx).ui().conflict.clone()).unwrap();
    assert_eq!(cx.read(|cx| conflict.read(cx).page), ConflictPage::Hunk);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    let active_fill = paint(cx, window, 0);
    assert_eq!(
        kagi::ui::conflict_view::continue_primary_for_e2e(),
        Some(false)
    );
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    assert_eq!(&paint(cx, window, 0)[..3], &[211, 211, 211]);
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.mode.as_ref().unwrap().selected_file, Some(0));
        assert!(
            view.editing.is_none(),
            "initial selection must not skip a raw file"
        );
    });
    click_control(cx, window, "conflict-row-1");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.page, ConflictPage::Hunk);
        assert_eq!(view.editing.as_deref(), Some(Path::new("notes.txt")));
    });
    click_control(cx, window, "conflict-result-editor");
    chrome_contract(cx, window, 2, false);
    hidden_count(cx, window, None);
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-tree", "notes.txt");
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    assert_eq!(paint(cx, window, 1), active_fill);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    click_control(cx, window, "conflict-sidebar-toggle");
    hidden_count(cx, window, Some(3));
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert!(view.sidebar_hidden);
        assert_eq!(view.editing.as_deref(), Some(Path::new("notes.txt")));
    });
    click_control(cx, window, "conflict-sidebar-toggle");
    hidden_count(cx, window, None);
    assert!(!cx.read(|cx| conflict.read(cx).sidebar_hidden));
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-tree", "notes.txt");
    click_control(cx, window, "conflict-row-0");
    tab_to_tree(cx, window);
    crate::keyboard_nav::keys(cx, window, "tab");
    assert!(!tree_focused(cx, window), "the tree has only one Tab stop");
    crate::keyboard_nav::keys(cx, window, "shift-tab");
    assert!(tree_focused(cx, window));
    crate::keyboard_nav::keys(cx, window, "down");
    paint(cx, window, 0);
    selected_row("conflict-tree", "blob.bin");
    assert!(cx.read(|cx| conflict.read(cx).editing.is_none()));
    crate::keyboard_nav::keys(cx, window, "enter");
    assert_eq!(
        cx.read(|cx| conflict.read(cx).editing.clone()).as_deref(),
        Some(Path::new("notes.txt"))
    );
    assert_eq!(cx.read(|cx| conflict.read(cx).selected_hunk), 0);
    click_control(cx, window, "conflict-hunk-0-left");
    click_control(cx, window, "conflict-result-editor");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-tree", "notes.txt");
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "navigation and choices never stage"
    );
    click_control(cx, window, "conflict-save");
    crate::app_conflict::wait_idle(cx, &app);
    crate::busy_dismiss::ready(cx, &app);
    cx.run_until_parked();
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.selected_saved.as_deref(), Some(Path::new("notes.txt")));
        assert!(view.editing.is_none());
        assert!(view.editor_inputs.is_none());
        assert_eq!(view.mode.as_ref().unwrap().session.files.len(), 2);
    });
    let repository = git2::Repository::open(fixture.path()).unwrap();
    let entry = repository
        .index()
        .unwrap()
        .get_path(Path::new("notes.txt"), 0)
        .unwrap();
    assert_eq!(
        repository.find_blob(entry.id).unwrap().content(),
        b"current\n"
    );
    cx.read(|cx| {
        let view = conflict.read(cx);
        let preview = view
            .saved_preview
            .as_ref()
            .expect("accepted saved byte preview");
        assert_eq!(preview.path, Path::new("notes.txt"));
        assert_eq!(preview.oid, entry.id.to_string());
        assert_eq!(
            preview.bytes.as_ref(),
            repository.find_blob(entry.id).unwrap().content()
        );
        assert!(!view.result_editing);
    });
    chrome_contract(cx, window, 1, true);
    assert_eq!(
        kagi::ui::conflict_view::continue_primary_for_e2e(),
        Some(false)
    );
    let saved_before = repo_fingerprint(fixture.path());
    let receipts_before: Vec<_> = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 20)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    click_control(cx, window, "conflict-sidebar-toggle");
    hidden_count(cx, window, Some(2));
    click_control(cx, window, "conflict-sidebar-toggle");
    hidden_count(cx, window, None);
    assert_eq!(
        cx.read(|cx| conflict.read(cx).selected_saved.clone())
            .as_deref(),
        Some(Path::new("notes.txt"))
    );
    click_control(cx, window, "conflict-next-file");
    assert_eq!(
        cx.read(|cx| conflict.read(cx).mode.as_ref().unwrap().selected_file),
        Some(0)
    );
    assert!(cx.read(|cx| conflict.read(cx).selected_saved.is_none()));
    tab_to_tree(cx, window);
    crate::keyboard_nav::keys(cx, window, "down down");
    paint(cx, window, 0);
    selected_row("conflict-tree", "blob.bin");
    assert!(
        cx.read(|cx| conflict.read(cx).selected_saved.is_none()),
        "arrow focus is not selection"
    );
    crate::keyboard_nav::keys(cx, window, "enter");
    assert_eq!(
        cx.read(|cx| conflict.read(cx).selected_saved.clone())
            .as_deref(),
        Some(Path::new("notes.txt")),
        "arrows cross from pending to saved rows"
    );
    crate::keyboard_nav::keys(cx, window, "up");
    assert!(cx.read(|cx| conflict.read(cx).selected_saved.is_some()));
    crate::keyboard_nav::keys(cx, window, "space");
    assert_eq!(
        cx.read(|cx| conflict.read(cx).editing.clone()).as_deref(),
        Some(Path::new("zeta.txt")),
        "Space opens a pending row across the saved boundary"
    );
    click_control(cx, window, "conflict-saved-row-0");
    paint(cx, window, 0);
    selected_row("conflict-tree", "notes.txt");
    assert!(cx.read(|cx| conflict.read(cx).editor_inputs.is_none()));
    assert_eq!(
        repo_fingerprint(fixture.path()),
        saved_before,
        "saved inspection never writes Git"
    );
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 20)
            .into_iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        receipts_before,
        "navigation emits no operation receipt"
    );
    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
    drop(conflict);
    unmount(cx, app, window);
}

fn tree_focused(cx: &mut VisualTestAppContext, window: AnyWindowHandle) -> bool {
    kagi::ui::conflict_view::take_next_conflict_focus_probe();
    cx.simulate_keystrokes(window, "f19");
    cx.run_until_parked();
    kagi::ui::conflict_view::take_next_conflict_focus_probe()
}

fn tab_to_tree(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    for _ in 0..80 {
        if tree_focused(cx, window) {
            return;
        }
        crate::keyboard_nav::keys(cx, window, "shift-tab");
    }
    panic!("the persistent file tree must be keyboard reachable");
}
