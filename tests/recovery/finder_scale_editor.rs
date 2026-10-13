//! #1467: native Editor file-tree density, glyph clipping and navigation.
use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, repo_fingerprint, unmount};
use gpui::{px, AnyWindowHandle, AppContext, Entity, ScrollStrategy, VisualTestAppContext};
use gpui_component::scroll::ScrollbarHandle;
use kagi::ui::{
    i18n::{self, Lang, Msg},
    theme, KagiApp,
};
use kagi_ui_core::{
    file_tree::TreeRow,
    tree_a11y::{clear_recorded_trees, recorded_tree},
};
use kagi_ui_editor::{EditorTreeCellProbe, EditorWorkspaceView};
use std::path::Path;
use std::time::{Duration, Instant};

const FILE_COUNT: usize = 72;

fn fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    std::fs::create_dir(fixture.path().join("glyphs")).unwrap();
    for index in 0..FILE_COUNT {
        std::fs::write(
            fixture.path().join(format!("glyphs/glyphs-{index:03}.txt")),
            "base\n",
        )
        .unwrap();
    }
    git(fixture.path(), &["add", "glyphs"]);
    git(fixture.path(), &["commit", "-qm", "Editor scale glyphs"]);
    for index in 0..FILE_COUNT {
        std::fs::write(
            fixture.path().join(format!("glyphs/glyphs-{index:03}.txt")),
            "changed\n",
        )
        .unwrap();
    }
    fixture
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    // The new rem size and deferred uniform-list reveal need measured frames.
    for _ in 0..4 {
        cx.run_until_parked();
        clear_recorded_trees();
        cx.update_window(window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear();
        })
        .unwrap();
    }
}

fn wait_for(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    content: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        draw(cx, window);
        if cx.read(|cx| {
            let view = editor.read(cx);
            !view.loading
                && !view.files.is_empty()
                && (!content
                    || (!view.file_loading && view.editor.is_some() && view.content.is_some()))
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "real Editor file read did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_glyphs(
    cx: &mut VisualTestAppContext,
    window: AnyWindowHandle,
    text: &str,
    cell: EditorTreeCellProbe,
) {
    cx.update_window(window, |_, window, _| {
        let mut style = window.text_style();
        style.font_family = theme::UI_FONT.into();
        style.font_weight = cell.font_weight;
        let text_system = window.text_system();
        let line = text_system.shape_line(
            text.to_string().into(),
            cell.font_size,
            &[style.to_run(text.len())],
            None,
        );
        assert!(
            line.width <= cell.bounds.size.width,
            "fixture label must not need ellipsis: {text}"
        );
        let baseline = cell.bounds.top()
            + (cell.bounds.size.height - line.ascent - line.descent) / 2.
            + line.ascent;
        for (index, character) in text.char_indices() {
            let font = line
                .font_id_for_index(index)
                .expect("shaped Editor tree font");
            let glyph = text_system
                .typographic_bounds(font, cell.font_size, character)
                .expect("native Editor tree glyph bounds");
            // CoreText glyph bounds are baseline-relative, with positive y up.
            let top = baseline - glyph.bottom();
            let bottom = baseline - glyph.top();
            let x = cell.bounds.left() + line.x_for_index(index);
            let left = x + glyph.left();
            let right = x + glyph.right();
            assert!(
                top >= cell.mask.top()
                    && bottom <= cell.mask.bottom()
                    && left >= cell.mask.left()
                    && right <= cell.mask.right(),
                "Editor {text:?} glyph {character:?} must fit its actual paint mask: \
                 glyph={left:?}..{right:?}/{top:?}..{bottom:?}, mask={:?}",
                cell.mask
            );
        }
    })
    .unwrap();
}

fn assert_row(
    cx: &mut VisualTestAppContext,
    editor: &Entity<EditorWorkspaceView>,
    window: AnyWindowHandle,
    index: usize,
    name: &str,
    badge: &str,
) {
    let probe = cx
        .read(|cx| editor.read(cx).tree_row_probe_for_e2e(index))
        .expect("base tree row painted in the current frame");
    let row = probe.row.expect("native row bounds");
    assert!(
        (row.bounds.size.height - theme::scaled_px(20.)).abs() <= px(0.6),
        "Editor {name}: scaled 20px row, actual {:?}",
        row.bounds
    );
    assert!(
        row.bounds.top() >= row.mask.top() && row.bounds.bottom() <= row.mask.bottom(),
        "Editor {name}: complete row revealed inside its native viewport mask"
    );
    for cell in [
        probe.badge.expect("badge painted"),
        probe.name.expect("name painted"),
    ] {
        assert!(
            (cell.font_size - theme::scaled_px(13.)).abs() <= px(0.1),
            "Editor {name}: scaled 13px tree font, actual {:?}",
            cell.font_size
        );
        assert!(
            (cell.line_height - (cell.font_size * 1.2).round()).abs() <= px(0.1),
            "Editor {name}: rounded 1.2 leading, actual {:?}",
            cell.line_height
        );
        assert!(
            cell.bounds.top() >= row.bounds.top() && cell.bounds.bottom() <= row.bounds.bottom(),
            "Editor {name}: text line fits its row"
        );
    }
    let badge_cell = probe.badge.unwrap();
    assert!(
        (badge_cell.bounds.size.width - theme::scaled_px(16.)).abs() <= px(0.6),
        "Editor {name}: scaled 16px badge column"
    );
    assert_glyphs(cx, window, name, probe.name.unwrap());
    assert_glyphs(cx, window, badge, badge_cell);
}

fn inspect_mounted(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: AnyWindowHandle) {
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("real Editor workspace");
    wait_for(cx, &editor, window, false);
    let (directory, files) = cx.read(|cx| {
        let view = editor.read(cx);
        let directory = view.tree.iter().position(|row| matches!(row, TreeRow::Dir { name, .. } if name.as_ref() == "glyphs")).unwrap();
        let files: Vec<_> = (0..FILE_COUNT).map(|index| {
            let path = format!("glyphs/glyphs-{index:03}.txt");
            let file = view.files.iter().position(|file| file.path == Path::new(&path)).unwrap();
            let base = view.tree.iter().position(|row| matches!(row, TreeRow::File { file_index, .. } if *file_index == file)).unwrap();
            (file, base)
        }).collect();
        (directory, files)
    });
    editor.update(cx, |view, cx| {
        view.expand_all(cx);
        view.select(files[0].0, cx);
        view.tree_scroll.scroll_to_item(0, ScrollStrategy::Top);
    });
    wait_for(cx, &editor, window, true);
    assert_row(cx, &editor, window, directory, "glyphs", "▾");
    assert_row(cx, &editor, window, files[0].1, "glyphs-000.txt", "M");
    let tree = recorded_tree("ews-file-tree").expect("rendered tree AX assignments");
    assert_eq!(tree.label, Msg::A11yFileTree.t());
    let folder = tree
        .rows
        .values()
        .find(|row| row.label == "Folder glyphs")
        .unwrap();
    assert_eq!(
        (folder.level, folder.expanded, folder.selected),
        (1, Some(true), None)
    );
    let file = tree
        .rows
        .values()
        .find(|row| row.label == "File glyphs-000.txt, modified")
        .unwrap();
    assert_eq!(
        (file.level, file.position, file.size, file.selected),
        (2, 1, FILE_COUNT, Some(true))
    );

    editor.update(cx, |view, cx| view.toggle_dir(directory, cx));
    draw(cx, window);
    assert_row(cx, &editor, window, directory, "glyphs", "▸");
    let tree = recorded_tree("ews-file-tree").unwrap();
    assert_eq!(
        tree.rows
            .values()
            .find(|row| row.label == "Folder glyphs")
            .unwrap()
            .expanded,
        Some(false)
    );
    assert!(
        !tree
            .rows
            .values()
            .any(|row| row.label.contains("glyphs-000.txt")),
        "collapsed files leave the rendered AX tree"
    );
    assert!(
        cx.read(|cx| editor.read(cx).tree_row_probe_for_e2e(files[0].1).is_none()),
        "collapsed files leave the current paint probes"
    );
    editor.update(cx, |view, cx| {
        view.toggle_dir(directory, cx);
        view.select(files[FILE_COUNT - 2].0, cx);
        view.tree_scroll.scroll_to_item(0, ScrollStrategy::Top);
    });
    wait_for(cx, &editor, window, true);
    let before_scroll = cx.read(|cx| editor.read(cx).tree_scroll.offset());
    cx.update_window(window, |_, window, cx| {
        let focus = app
            .read(cx)
            .root_focus
            .clone()
            .expect("production root keyboard focus");
        focus.focus(window, cx);
    })
    .unwrap();
    keys(cx, window, "down");
    wait_for(cx, &editor, window, true);
    assert!(
        cx.read(|cx| editor.read(cx).tree_scroll.offset().y < before_scroll.y),
        "root Down must scroll the scaled virtual tree to its tail"
    );
    assert_row(
        cx,
        &editor,
        window,
        files[FILE_COUNT - 1].1,
        "glyphs-071.txt",
        "M",
    );
    cx.read(|cx| {
        let view = editor.read(cx);
        assert_eq!(view.selected, Some(files[FILE_COUNT - 1].0));
        assert_eq!(
            view.open_path.as_deref(),
            Some(Path::new("glyphs/glyphs-071.txt")),
            "keyboard activation opens the exact selected file"
        );
        assert!(!view.dirty);
    });
    let tree = recorded_tree("ews-file-tree").unwrap();
    let last = tree
        .rows
        .values()
        .find(|row| row.label == "File glyphs-071.txt, modified")
        .unwrap();
    assert_eq!(
        (last.level, last.position, last.size, last.selected),
        (2, FILE_COUNT, FILE_COUNT, Some(true))
    );
    keys(cx, window, "up");
    wait_for(cx, &editor, window, true);
    assert_row(
        cx,
        &editor,
        window,
        files[FILE_COUNT - 2].1,
        "glyphs-070.txt",
        "M",
    );
    assert_eq!(
        cx.read(|cx| editor.read(cx).selected),
        Some(files[FILE_COUNT - 2].0)
    );
    drop(editor);
}

/// Called by `finder_scale_rest::scenario`; owns and retires every mounted window.
pub fn inspect(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let _saved = crate::gui_isolation::SavedKeys::keep(&["ui_zoom", "lang"]);
    i18n::set_lang(Lang::En);
    let fixture = fixture();
    let before = repo_fingerprint(fixture.path());
    for preset in ["apple-light", "apple-dark"] {
        for zoom in [1., 1.67] {
            assert!(theme::set_active(preset));
            theme::set_zoom(zoom);
            cx.update(theme::sync_gpui_component_theme);
            let (app, window) = mount(cx, fixture.path());
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                inspect_mounted(cx, &app, window)
            }));
            // Retire Input owners before unmount drains their deferred tasks.
            cx.update_window(window, |_, window, cx| {
                window.replace_root(cx, |window, cx| {
                    let empty = cx.new(|_| gpui::Empty);
                    gpui_component::Root::new(empty, window, cx)
                });
            })
            .ok();
            let cleanup =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unmount(cx, app, window)));
            if let Err(payload) = result {
                std::panic::resume_unwind(payload);
            }
            if let Err(payload) = cleanup {
                std::panic::resume_unwind(payload);
            }
        }
    }
    assert_eq!(
        before,
        repo_fingerprint(fixture.path()),
        "scale and keyboard navigation must not mutate the fixture"
    );
}
