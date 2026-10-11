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
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    let active_fill = paint(cx, window, 0);
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    assert_eq!(&paint(cx, window, 0)[..3], &[211, 211, 211]);
    click_control(cx, window, "conflict-row-1");
    // Keep the pointer on notes.txt: hover must not replace the selected fill.
    assert_eq!(
        &paint(cx, window, 1)[..3],
        &[211, 211, 211],
        "clicked notes.txt stays visibly selected under the pointer"
    );
    selected_row("conflict-files", "notes.txt");
    app.update(cx, |app, cx| app.set_window_active_for_e2e(true, cx));
    assert_eq!(
        paint(cx, window, 1),
        active_fill,
        "active selection also wins over hover"
    );
    app.update(cx, |app, cx| app.set_window_active_for_e2e(false, cx));
    click_control(cx, window, "conflict-next");
    assert_eq!(&paint(cx, window, 2)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "zeta.txt");
    cx.read(|cx| {
        let view = conflict.read(cx);
        assert_eq!(view.editing.as_deref(), Some(Path::new("zeta.txt")));
        assert_eq!(
            view.editor_inputs.as_ref().unwrap().path,
            Path::new("zeta.txt")
        );
    });
    click_control(cx, window, "conflict-editor-prev");
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "notes.txt");
    click_control(cx, window, "conflict-editor-next");
    assert_eq!(&paint(cx, window, 2)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "zeta.txt");
    // Enter/Space on the same navigation control must use the same selection.
    // gpui-kit intentionally does not focus buttons on pointer down. Reverse
    // Tab from a blurred window reaches the dashboard's final enabled button.
    cx.update_window(window, |_, window, cx| {
        window.blur();
        window.focus_prev(cx);
        window.draw(cx).clear();
    })
    .unwrap();
    crate::keyboard_nav::keys(cx, window, "space");
    assert_eq!(&paint(cx, window, 0)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "blob.bin");
    cx.read(|cx| {
        assert!(
            conflict.read(cx).editing.is_none(),
            "binary preview leaves no stale content editor"
        );
    });
    click_control(cx, window, "conflict-row-1");
    conflict.update(cx, |view, cx| {
        view.conflict_editor_set_file_side(
            Path::new("notes.txt"),
            kagi_git::resolution::SelectionSide::Current,
            true,
        );
        cx.notify();
    });
    app.update(cx, |_, cx| cx.notify());
    assert_eq!(&paint(cx, window, 1)[..3], &[211, 211, 211]);
    selected_row("conflict-resolved-files", "notes.txt");
    click_control(cx, window, "conflict-next");
    assert_eq!(&paint(cx, window, 2)[..3], &[211, 211, 211]);
    selected_row("conflict-files", "zeta.txt");
    assert_eq!(
        repo_fingerprint(fixture.path()),
        before,
        "selection/draft resolution must not write Git state"
    );
    app.update(cx, |app, cx| app.set_theme(&old_theme, cx));
    drop(conflict);
    unmount(cx, app, window);
}
