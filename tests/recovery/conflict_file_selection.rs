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
        image
            .get_pixel(
                ((f32::from(bounds.left()) + 10.) * scale) as u32,
                ((f32::from(bounds.top()) + 5.) * scale) as u32,
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
    assert_eq!(cx.read(|cx| conflict.read(cx).page), ConflictPage::Overview);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    let active_fill = paint(cx, window, 0);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    assert_eq!(&paint(cx, window, 0)[..3], &[211, 211, 211]);
    click_control(cx, window, "conflict-next");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.mode.as_ref().unwrap().selected_file, Some(0));
        assert!(
            view.editing.is_none(),
            "Open must not skip the first raw file"
        );
    });
    click_control(cx, window, "conflict-overview");
    click_control(cx, window, "conflict-row-1");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.page, ConflictPage::Hunk);
        assert_eq!(view.editing.as_deref(), Some(Path::new("notes.txt")));
    });
    click_control(cx, window, "conflict-review");
    assert_eq!(cx.read(|cx| conflict.read(cx).page), ConflictPage::Review);
    click_control(cx, window, "conflict-overview");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "notes.txt");
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    assert_eq!(paint(cx, window, 1), active_fill);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    click_control(cx, window, "conflict-next");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.editing.as_deref(), Some(Path::new("notes.txt")));
        assert_eq!(
            view.selected_hunk, 0,
            "file navigation is not hunk navigation"
        );
    });
    click_control(cx, window, "conflict-overview");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "notes.txt");
    click_control(cx, window, "conflict-row-0");
    click_control(cx, window, "conflict-overview");
    cx.update_window(window, |_, window, cx| {
        window.draw(cx).clear();
    })
    .unwrap();
    let mut reached_next = false;
    for _ in 0..80 {
        kagi::ui::conflict_view::take_next_conflict_focus_probe();
        cx.simulate_keystrokes(window, "shift-tab");
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        cx.simulate_keystrokes(window, "f19");
        cx.run_until_parked();
        if kagi::ui::conflict_view::take_next_conflict_focus_probe() {
            reached_next = true;
            break;
        }
    }
    assert!(
        reached_next,
        "Open must be keyboard reachable in the overview"
    );
    crate::keyboard_nav::keys(cx, window, "space");
    cx.read(|cx| {
        assert!(
            conflict.read(cx).editing.is_none(),
            "raw file has no stale text editor"
        )
    });
    click_control(cx, window, "conflict-overview");
    assert_eq!(&paint(cx, window, 0)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "blob.bin");
    click_control(cx, window, "conflict-row-1");
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_file_side(
            Path::new("notes.txt"),
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        cx.notify();
    });
    click_control(cx, window, "conflict-overview");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "notes.txt");
    click_control(cx, window, "conflict-next");
    click_control(cx, window, "conflict-overview");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "notes.txt");
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "page changes and draft choices never stage"
    );
    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
    drop(conflict);
    unmount(cx, app, window);
}
