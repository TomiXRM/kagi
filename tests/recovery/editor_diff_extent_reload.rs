//! #1121: an unrelated worktree notification re-reads a clean Editor buffer,
//! but must not restart the unchanged WIP diff's native reading position.
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_viewed::paint;
use gpui::{AnyWindowHandle, AppContext, Entity, VisualTestAppContext};
use kagi::ui::editor_workspace::EditorWorkspaceView;
use kagi::ui::{DiffRow, MainDiffView};
use kagi_ui_core::theme;

const FILE: &str = "reading.rs";

struct Restore {
    split: bool,
    zoom: f32,
    _saved: crate::gui_isolation::SavedKeys,
}
impl Drop for Restore {
    fn drop(&mut self) {
        theme::set_diff_split(self.split);
        theme::set_zoom(self.zoom);
    }
}

fn fixture() -> (tempfile::TempDir, String) {
    let fixture = build_fixture();
    let mut base = String::new();
    let mut wip = String::new();
    for line in 1..=800 {
        base.push_str(&format!("let line_{line:04} = {line};\n"));
        if line % 20 == 10 {
            wip.push_str(&format!(
                "let line_{line:04} = {}; // WIP_{line:04}\n",
                line + 1
            ));
        } else {
            wip.push_str(&format!("let line_{line:04} = {line};\n"));
        }
    }
    std::fs::write(fixture.path().join(FILE), base).unwrap();
    std::fs::write(fixture.path().join("other.txt"), "other base\n").unwrap();
    git(fixture.path(), &["add", FILE, "other.txt"]);
    git(
        fixture.path(),
        &["commit", "-q", "-m", "editor reading base"],
    );
    std::fs::write(fixture.path().join(FILE), &wip).unwrap();
    (fixture, wip)
}

// Compare semantic before/after content, not opaque source IDs or Arc pointers.
fn content(view: &MainDiffView) -> Vec<(String, String, Option<u32>, Option<u32>)> {
    view.rows
        .iter()
        .map(|row| match row {
            DiffRow::HunkHeader(text) => ("hunk".into(), text.to_string(), None, None),
            DiffRow::Line {
                kind,
                text,
                old_lineno,
                new_lineno,
                ..
            } => (
                format!("{kind:?}"),
                text.to_string(),
                *old_lineno,
                *new_lineno,
            ),
            DiffRow::Binary => ("binary".into(), String::new(), None, None),
        })
        .collect()
}

fn wait_loaded(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        paint(cx, window);
        let loaded = cx.read(|cx| {
            let view = editor.read(cx);
            !view.loading
                && !view.file_loading
                && view.editor.is_some()
                && view.content.is_some()
                && view.diff.is_some()
        });
        if loaded {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Editor content/WIP backend read did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn wheel(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    delta: f32,
) {
    let viewport = cx.read(|cx| editor.read(cx).diff_scroll.viewport_bounds());
    assert!(
        viewport.size.height > gpui::px(100.),
        "real Editor WIP viewport required"
    );
    cx.simulate_event(
        window,
        gpui::ScrollWheelEvent {
            position: viewport.center(),
            delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        },
    );
    paint(cx, window);
}

pub fn scenario_editor_diff_extent_unchanged_reload(cx: &mut VisualTestAppContext) {
    let _restore = Restore {
        split: theme::diff_split(),
        zoom: theme::zoom(),
        _saved: crate::gui_isolation::SavedKeys::keep(&["diff_split", "ui_zoom"]),
    };
    theme::set_diff_split(false);
    theme::set_zoom(1.);
    let (fixture, wip) = fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.update(cx, |app, cx| app.open_editor_workspace(cx));
        let editor = cx
            .read(|cx| app.read(cx).ui().editor_workspace.clone())
            .expect("actual Editor owner");
        editor.update(cx, |view, cx| view.open_tab(FILE.into(), cx));
        wait_loaded(cx, &editor, window);
        let (before_content, count, native) = cx.read(|cx| {
            let view = editor.read(cx);
            assert_eq!(view.repo_path, repo);
            assert_eq!(view.open_path.as_deref(), Some(std::path::Path::new(FILE)));
            assert_eq!(view.content.as_deref(), Some(wip.as_str()));
            assert!(!view.dirty, "disk-WIP text is a clean Editor buffer");
            let diff = view
                .diff
                .as_ref()
                .unwrap()
                .downcast_ref::<MainDiffView>()
                .expect("host WIP diff");
            assert!(
                diff.rows
                    .iter()
                    .filter(|row| matches!(row, DiffRow::HunkHeader(_)))
                    .count()
                    > 20,
                "actual Git multi-hunk WIP needed"
            );
            (content(diff), diff.rows.len(), view.diff_scroll.clone())
        });
        // Visit the true tail so a later unchanged reload must preserve at
        // least one actual distant measurement, not just the visible rows.
        wheel(cx, &editor, window, -20_000_000.);
        let last = count - 1;
        let tail = native.bounds_for_item(last).expect("actual tail measured");
        assert!(
            tail.top() >= native.viewport_bounds().top() - gpui::px(1.)
                && tail.bottom() <= native.viewport_bounds().bottom() + gpui::px(1.),
            "actual WIP tail in native viewport"
        );
        wheel(cx, &editor, window, 20_000_000.);
        wheel(cx, &editor, window, -900.);
        let before = native.logical_scroll_top();
        let before_bounds = native
            .bounds_for_item(before.item_ix)
            .expect("reading row drawn");
        let before_extent = native.max_offset_for_scrollbar().y;
        assert!(
            before.item_ix > 1 && before.item_ix + 20 < last,
            "native wheel must choose a real interior reading offset"
        );
        let known_tail = native
            .bounds_for_item(last)
            .expect("distant actual tail measurement retained before reload");
        let entity = editor.entity_id();
        // This is the same public boundary the real debounced WorkTree event
        // calls (#736), with an actual unrelated disk change behind it. No
        // timer-only simulated FS event or prepared diff response is injected.
        std::fs::write(
            repo.join("other.txt"),
            "other changed outside selected buffer\n",
        )
        .unwrap();
        let previous_seed =
            kagi::ui::e2e::editor_diff_seed_request(entity).expect("initial WIP seed admitted");
        editor.update(cx, |view, cx| view.on_worktree_changed(cx));
        wait_loaded(cx, &editor, window);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            paint(cx, window);
            if kagi::ui::e2e::editor_diff_seed_request(entity)
                .is_some_and(|request| request > previous_seed)
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "new actual Editor WIP seed did not arrive after worktree notification"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        paint(cx, window);
        cx.read(|cx| {
            let current = app
                .read(cx)
                .ui()
                .editor_workspace
                .as_ref()
                .expect("Editor still owned");
            assert_eq!(
                current.entity_id(),
                entity,
                "notification must keep the same Editor owner"
            );
            let view = current.read(cx);
            assert_eq!(view.repo_path, repo);
            assert_eq!(view.open_path.as_deref(), Some(std::path::Path::new(FILE)));
            assert!(!view.dirty && !view.external_changed);
            assert_eq!(view.content.as_deref(), Some(wip.as_str()));
            let diff = view
                .diff
                .as_ref()
                .unwrap()
                .downcast_ref::<MainDiffView>()
                .unwrap();
            assert_eq!(
                content(diff),
                before_content,
                "actual backend re-read must show the identical WIP content and line IDs"
            );
            let after = view.diff_scroll.logical_scroll_top();
            assert_eq!(
                after.item_ix, before.item_ix,
                "unchanged clean-buffer reload reset native Editor WIP reading row"
            );
            assert!(
                (after.offset_in_item - before.offset_in_item).abs() < gpui::px(1.),
                "unchanged reload moved within-row reading offset"
            );
            let bounds = view
                .diff_scroll
                .bounds_for_item(after.item_ix)
                .expect("same reading row remains measured");
            assert!(
                (bounds.top() - before_bounds.top()).abs() < gpui::px(1.),
                "same code row must stay at the same native viewport location"
            );
            let retained = view
                .diff_scroll
                .bounds_for_item(last)
                .expect("unchanged reload discarded distant actual WIP measurement");
            assert!((retained.size.height - known_tail.size.height).abs() < gpui::px(1.));
            assert!(
                (view.diff_scroll.max_offset_for_scrollbar().y - before_extent).abs()
                    < gpui::px(1.),
                "unchanged content must keep global native extent"
            );
            // The pre-notification native handle must still observe the same
            // live reading position, rather than being silently orphaned.
            assert_eq!(native.logical_scroll_top().item_ix, after.item_ix);
        });
        assert_eq!(std::fs::read_to_string(repo.join(FILE)).unwrap(), wip);
        drop(editor);
        drop(native);
    }));
    cx.update_window(window, |_, window, cx| {
        window.replace_root(cx, |window, cx| {
            let empty = cx.new(|_| gpui::Empty);
            gpui_component::Root::new(empty, window, cx)
        });
    })
    .expect("retire Editor reload root");
    for _ in 0..30 {
        crate::gui_evidence::present_windows();
        crate::macos::drain_native_events();
        cx.run_until_parked();
    }
    unmount(cx, app, window);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
