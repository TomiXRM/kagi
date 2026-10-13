//! #1484: compact Changes projects the existing Inspector, not WIP staging.
use super::*;
use kagi::ui::{diff_view::MainDiffSource, list_a11y};

const DIMENSIONS: (f32, f32) = (900., 660.);
const MESSAGE: &str = "Compact inspector commit\n\nRetained commit body 日本語";

fn inspector_fixture() -> tempfile::TempDir {
    let fixture = build_fixture();
    for (path, text) in [
        ("a/file.txt", "first file\n"),
        ("b/file.txt", "second file\n"),
    ] {
        std::fs::create_dir_all(fixture.path().join(path).parent().unwrap()).unwrap();
        std::fs::write(fixture.path().join(path), text).unwrap();
    }
    git(fixture.path(), &["add", "."]);
    git(fixture.path(), &["commit", "-qm", MESSAGE]);
    git(fixture.path(), &["tag", "compact-inspector"]);
    std::fs::write(fixture.path().join("compare-only.txt"), "HEAD comparison\n").unwrap();
    git(fixture.path(), &["add", "compare-only.txt"]);
    git(fixture.path(), &["commit", "-qm", "Comparison HEAD"]);
    std::fs::write(fixture.path().join("staged.txt"), "staged WIP\n").unwrap();
    git(fixture.path(), &["add", "staged.txt"]);
    std::fs::write(fixture.path().join("unstaged.txt"), "unstaged WIP\n").unwrap();
    fixture
}

pub fn scenario_compact_inspector_columns(cx: &mut VisualTestAppContext) {
    let _restore = crate::surface_scrollbars::PresentationRestore::capture(cx);
    let fixture = inspector_fixture();
    let before = crate::macos::repo_fingerprint(fixture.path());
    matrix(|case| {
        if case.height != DIMENSIONS.1 {
            return;
        }
        e2e::inspector_metadata::clear();
        e2e::text_caps::clear();
        list_a11y::clear_recorded_lists();
        let (app, win) = mount(cx, fixture.path(), case);
        resize(cx, win, DIMENSIONS);
        let panel = changes(cx, &app, win);
        let row = cx.read(|cx| {
            app.read(cx)
                .view()
                .rows
                .iter()
                .position(|row| row.summary.as_ref() == "Compact inspector commit")
                .unwrap()
        });
        app.update(cx, |app, cx| {
            app.inspector_tree_view = false;
            app.select(row);
            cx.notify();
        });
        settle(cx, win);
        // First #1484 oracle: baseline stops on the missing columns, never on
        // a new Rust API or fabricated list/metadata state.
        inspector_columns(win);
        metadata(cx, &app, win, row);
        click(cx, win, "inspector-file-0");
        assert_diff(cx, &app, "a/file.txt", row, 0);
        for (control, tree, role) in [
            ("toggle-tree", true, gpui::Role::Tree),
            ("toggle-path", false, gpui::Role::ListBox),
        ] {
            click(cx, win, control);
            assert_eq!(cx.read(|cx| app.read(cx).inspector_tree_view), tree);
            inspector_columns(win);
            assert_eq!(
                list_a11y::recorded_list("inspector-files-scroll")
                    .unwrap()
                    .role,
                Some(role)
            );
            keyboard_file(cx, &app, win, row);
        }
        // Width restoration projects the same selection into the ordinary
        // right Inspector without recreating the WIP owner or writing Git.
        resize(cx, win, (1392., 883.));
        assert!(
            e2e::control_bounds(win.window_id(), "compact-inspector-details").is_none(),
            "compact columns unmount in wide layout"
        );
        let right = bounds(win, "right-pane-clip");
        contained(
            right,
            bounds(win, "inspector-header"),
            "wide Inspector metadata",
        );
        contained(
            right,
            bounds(win, "inspector-files-scroll"),
            "wide Inspector files",
        );
        assert_eq!(cx.read(|cx| app.read(cx).ui().selected), Some(row));
        resize(cx, win, DIMENSIONS);
        click(cx, win, "compact-tab-changes");
        inspector_columns(win);
        app.update(cx, |app, cx| {
            app.close_main_diff();
            cx.notify();
        });
        settle(cx, win);
        compare_with_head(cx, &app, win, row);
        click(cx, win, "compact-wip-row");
        settle(cx, win);
        columns(win, DIMENSIONS);
        assert!(
            e2e::control_bounds(win.window_id(), "compact-inspector-details").is_none(),
            "WIP restores staging, not commit metadata"
        );
        for id in [
            "cp-stage-all",
            "cp-unstage-all",
            "cp-view-tree",
            "cp-staged-view-tree",
        ] {
            bounds(win, id);
        }
        assert_eq!(
            cx.read(|cx| {
                let state = &panel.read(cx).state;
                (state.unstaged.len(), state.staged.len())
            }),
            (1, 1),
            "real staged/unstaged WIP stays on the original owner"
        );
        assert_eq!(
            cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
            panel.entity_id()
        );
        drop(panel);
        unmount(cx, app, win);
    });
    assert_eq!(before, crate::macos::repo_fingerprint(fixture.path()));
}

fn inspector_columns(win: AnyWindowHandle) {
    let panel = bounds(win, "bottom-panel");
    let details = bounds(win, "compact-inspector-details");
    let files = bounds(win, "compact-inspector-files");
    for (name, column) in [("commit details", details), ("changed files", files)] {
        contained(viewport(DIMENSIONS), column, name);
        contained(panel, column, name);
        assert!(column.size.width > px(100.) && column.size.height > px(0.));
    }
    near(details.top(), files.top(), "Inspector columns share top");
    near(
        details.bottom(),
        files.bottom(),
        "Inspector columns share bottom",
    );
    assert!(
        details.right() <= files.left() + px(1.),
        "metadata left, files right; never stacked"
    );
    assert!((details.size.width - files.size.width).abs() <= theme::scaled_px(16.) + px(1.));
    contained(
        files,
        bounds(win, "inspector-files-scroll"),
        "existing RowList viewport",
    );
    for id in ["toggle-path", "toggle-tree"] {
        contained(files, bounds(win, id), "shared Path/Tree control");
    }
    let row = bounds(win, "inspector-file-0");
    near(
        row.size.height,
        theme::scaled_px(20.),
        "shared dense file row",
    );
    near(
        e2e::control_paint(win.window_id(), "inspector-file-name-0")
            .unwrap()
            .font_size,
        theme::scaled_px(13.),
        "shared dense file font",
    );
}

fn metadata(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    row: usize,
) {
    let column = bounds(win, "compact-inspector-details");
    let header = bounds(win, "inspector-header");
    assert!(
        header.left() >= column.left() - px(1.)
            && header.right() <= column.right() + px(1.)
            && header.top() < column.bottom()
            && header.bottom() > column.top(),
        "shared header belongs to the left scroll viewport without shrinking its contents"
    );
    for id in ["inspector-author", "inspector-hash-chip"] {
        contained(column, bounds(win, id), "shared Inspector metadata");
    }
    let rendered = e2e::inspector_metadata::last().expect("real metadata renderer");
    let (author, short) = cx.read(|cx| {
        let row = &app.read(cx).view().rows[row];
        (row.author.clone(), row.short_id.clone())
    });
    assert_eq!(rendered.author, author);
    assert_eq!(rendered.hash_role, Some(gpui::Role::Button));
    assert_eq!(
        rendered.hash_name.as_deref(),
        Some(i18n::inspector_copy_hash(&short).as_str())
    );
    assert_eq!(
        e2e::text_caps::message().source.as_ref().trim_end(),
        MESSAGE
    );
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    assert!(ax
        .rows
        .values()
        .any(|(name, _)| name.ends_with("a/file.txt")));
    assert!(ax
        .rows
        .values()
        .any(|(name, _)| name.ends_with("b/file.txt")));
    // At 167% the unchanged Inspector header can be taller than the panel.
    // Its real wheel owner must reveal the actions, not clip or shrink them.
    wheel(cx, win, column.center(), -120.);
    for id in ["create-branch-btn", "cherry-pick-btn"] {
        contained(column, bounds(win, id), "reachable shared commit action");
    }
}

fn keyboard_file(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    row: usize,
) {
    focus_root(cx, app, win);
    let focused = |cx: &mut VisualTestAppContext| {
        cx.update_window(win, |_, window, cx| {
            app.read(cx)
                .list_row_focused_for_e2e("inspector-files-scroll", window)
        })
        .unwrap()
    };
    for _ in 0..80 {
        press_key(cx, win, "tab");
        if focused(cx).is_some() {
            break;
        }
    }
    assert!(
        focused(cx).is_some(),
        "Tab enters the shared compact file RowList"
    );
    for key in ["home", "down", "enter"] {
        press_key(cx, win, key);
    }
    assert_eq!(focused(cx).as_deref(), Some("b/file.txt"));
    assert_diff(cx, app, "b/file.txt", row, 1);
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    assert_eq!(
        ax.rows.values().filter(|(_, selected)| *selected).count(),
        1
    );
}

fn compare_with_head(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
    win: AnyWindowHandle,
    row: usize,
) {
    // The AX recorder retains row positions across frames; discard the
    // historical two-row list before observing the shorter Compare list.
    list_a11y::clear_recorded_lists();
    app.update(cx, |app, cx| {
        let base = app.view().rows[row].id.clone();
        app.open_compare_with_head(base, Some(cx));
        cx.notify();
    });
    settle(cx, win);
    let (base, files) = cx.read(|cx| {
        let app = app.read(cx);
        let compare = app.ui().compare_view.as_ref().unwrap().read(cx).view();
        assert_eq!(compare.target, kagi::ui::diff_view::CompareTarget::Head);
        (compare.base.clone(), compare.files.clone())
    });
    assert_eq!(files.len(), 1, "fixture Compare differs from commit files");
    assert_eq!(files[0].path.to_str(), Some("compare-only.txt"));
    let ax = list_a11y::recorded_list("inspector-files-scroll").unwrap();
    assert_eq!(
        ax.rows.len(),
        files.len(),
        "compact Changes renders Compare files"
    );
    for (index, file) in files.iter().enumerate() {
        let path = file.path.to_str().unwrap();
        assert!(
            ax.rows.values().any(|(name, _)| name.ends_with(path)),
            "compact Changes must show actual Compare path {path}"
        );
        click(cx, win, &format!("inspector-file-{index}"));
        cx.read(|cx| {
            let app = app.read(cx);
            let diff = app.ui().main_diff.as_ref().unwrap().read(cx);
            assert_eq!(diff.view.title.as_ref(), path);
            assert!(matches!(
                &diff.view.source,
                MainDiffSource::Compare { base: actual, target, file_index }
                    if actual == &base
                        && *target == kagi::ui::diff_view::CompareTarget::Head
                        && *file_index == index
            ));
            assert_eq!(app.ui().selected, Some(row));
        });
    }
    app.update(cx, |app, cx| {
        app.close_main_diff();
        cx.notify();
    });
    settle(cx, win);
}

fn assert_diff(
    cx: &VisualTestAppContext,
    app: &Entity<KagiApp>,
    path: &str,
    row: usize,
    file: usize,
) {
    cx.read(|cx| {
        let app = app.read(cx);
        let diff = app
            .ui()
            .main_diff
            .as_ref()
            .expect("file activation opens central diff")
            .read(cx);
        assert_eq!(diff.view.title.as_ref(), path);
        assert!(matches!(
            &diff.view.source,
            MainDiffSource::Commit { row_index, file_index, commit: Some(commit) }
                if *row_index == row && *file_index == file && commit == &app.view().rows[row].id
        ));
        assert_eq!(
            app.ui().selected,
            Some(row),
            "diff retains graph commit selection"
        );
    });
}
