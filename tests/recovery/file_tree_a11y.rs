//! #354: the Editor Workspace and Commit Panel expose real flattened file
//! trees with localized names, absolute hierarchy, selection and disclosure.
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{Entity, VisualTestAppContext};
use kagi::ui::{
    commit_panel::CommitPanelFileRef,
    e2e,
    i18n::{self, Lang, Msg},
    KagiApp,
};
use kagi_ui_core::{
    file_tree::TreeRow,
    tree_a11y::{clear_recorded_trees, recorded_tree, RecordedTree, RecordedTreeItem},
};
use std::path::Path;
use std::time::{Duration, Instant};

fn fixture_files(repo: &Path) {
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::create_dir(repo.join("docs")).unwrap();
    for file in ["src/Fix {}.txt", "src/alpha.txt", "docs/report.md"] {
        std::fs::write(repo.join(file), "base\n").unwrap();
    }
    git(repo, &["add", "src", "docs"]);
    git(repo, &["commit", "-qm", "tree baseline"]);
    for file in ["src/Fix {}.txt", "src/alpha.txt", "docs/report.md"] {
        std::fs::write(repo.join(file), "changed\n").unwrap();
    }
    git(repo, &["add", "docs/report.md"]);
    std::fs::write(repo.join("Cargo.lock"), "# generated\n").unwrap();
}

fn draw(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, window: gpui::AnyWindowHandle) {
    clear_recorded_trees();
    app.update(cx, |_, cx| cx.notify());
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .unwrap();
}

fn item<'a>(tree: &'a RecordedTree, needle: &str) -> &'a RecordedTreeItem {
    tree.rows
        .values()
        .find(|item| item.label.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}: {tree:?}"))
}

pub fn scenario_file_tree_roles(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original_language = i18n::lang();
    i18n::set_lang(Lang::En);
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    fixture_files(&repo);
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("editor");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| !editor.read(cx).loading && !editor.read(cx).files.is_empty()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor files did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    let (selected, src_dir) = cx.read(|cx| {
        let editor = editor.read(cx);
        let selected = editor
            .files
            .iter()
            .position(|f| f.path == Path::new("src/Fix {}.txt"))
            .unwrap();
        let src_dir = editor
            .tree
            .iter()
            .position(|r| matches!(r, TreeRow::Dir { name, .. } if name.as_ref() == "src"))
            .unwrap();
        (selected, src_dir)
    });
    editor.update(cx, |editor, cx| editor.select(selected, cx));
    cx.run_until_parked();
    draw(cx, &app, window);
    let tree = recorded_tree("ews-file-tree").expect("editor tree drawn");
    assert_eq!(tree.label, Msg::A11yFileTree.t());
    let src = item(&tree, "Folder src");
    assert_eq!(
        (src.level, src.expanded, src.selected),
        (1, Some(true), None)
    );
    let fix = item(&tree, "Fix {}.txt");
    assert_eq!(fix.label, "File Fix {}.txt, modified");
    assert_eq!(
        (fix.level, fix.position, fix.size, fix.selected),
        (2, 1, 2, Some(true))
    );
    assert_eq!(
        (
            item(&tree, "alpha.txt").position,
            item(&tree, "alpha.txt").size
        ),
        (2, 2)
    );
    editor.update(cx, |editor, cx| editor.toggle_dir(src_dir, cx));
    cx.run_until_parked();
    draw(cx, &app, window);
    let tree = recorded_tree("ews-file-tree").unwrap();
    assert_eq!(item(&tree, "Folder src").expanded, Some(false));
    assert!(!tree
        .rows
        .values()
        .any(|row| row.label.contains("Fix {}.txt")));
    editor.update(cx, |editor, cx| editor.toggle_dir(src_dir, cx));
    i18n::set_lang(Lang::Ja);
    cx.run_until_parked();
    draw(cx, &app, window);
    let tree = recorded_tree("ews-file-tree").unwrap();
    assert_eq!(tree.label, Msg::A11yFileTree.t());
    assert_eq!(item(&tree, "Fix {}.txt").label, "ファイル Fix {}.txt、変更");
    assert_eq!(item(&tree, "Fix {}.txt").selected, Some(true));
    // The center pane is exclusive: close the editor to display the panel.
    drop(editor);
    app.update(cx, |app, cx| {
        app.close_editor_workspace();
        cx.notify();
    });
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();
    let panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .expect("panel");
    panel.update(cx, |view, cx| {
        view.state.tree_view = true;
        let selected = view
            .state
            .unstaged
            .iter()
            .position(|file| file.path == Path::new("src/Fix {}.txt"))
            .unwrap();
        view.state.selected_file = Some(CommitPanelFileRef::Unstaged { index: selected });
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, &app, window);
    let unstaged = recorded_tree("cp-unstaged-tree").expect("unstaged tree drawn");
    let staged = recorded_tree("cp-staged-tree").expect("staged tree drawn");
    assert_eq!(unstaged.label, Msg::A11yUnstagedTree.t());
    assert_eq!(staged.label, Msg::A11yStagedTree.t());
    let src = item(&unstaged, "フォルダー src");
    assert_eq!(
        (src.level, src.expanded, src.position, src.size),
        (1, None, 1, 2)
    );
    let fix = item(&unstaged, "Fix {}.txt");
    assert_eq!(fix.label, "ファイル Fix {}.txt、変更");
    assert_eq!(
        (fix.level, fix.position, fix.size, fix.selected),
        (2, 1, 2, Some(true))
    );
    let fold = item(&unstaged, Msg::GeneratedFilesSection.t());
    assert_eq!(
        (fold.level, fold.expanded, fold.position, fold.size),
        (1, Some(false), 2, 2)
    );
    assert_eq!(
        (
            item(&staged, "フォルダー docs").level,
            item(&staged, "report.md").level
        ),
        (1, 2)
    );
    assert_eq!(item(&staged, "report.md").selected, Some(false));
    // Reattach the *same* panel after its file set changes. from_repo starts
    // tree_revision at 1 again, so a cache keyed only by that revision would
    // assign the old sibling count to new rows or omit their TreeItem role.
    std::fs::write(repo.join("src/beta.txt"), "new\n").unwrap();
    std::fs::write(repo.join("docs/next.md"), "staged\n").unwrap();
    git(&repo, &["add", "docs/next.md"]);
    app.update(cx, |app, cx| {
        e2e::open_local_panel_no_inputs(app, repo.clone(), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        panel.entity_id(),
        cx.read(|cx| app.read(cx).ui().commit_panel.as_ref().unwrap().entity_id()),
        "reattach must reuse the drawn panel and its existing layout cache"
    );
    draw(cx, &app, window);
    let unstaged = recorded_tree("cp-unstaged-tree").unwrap();
    let staged = recorded_tree("cp-staged-tree").unwrap();
    assert_eq!(
        (
            item(&unstaged, "beta.txt").level,
            item(&unstaged, "beta.txt").position,
            item(&unstaged, "beta.txt").size
        ),
        (2, 3, 3)
    );
    assert_eq!(
        (
            item(&unstaged, "alpha.txt").position,
            item(&unstaged, "alpha.txt").size
        ),
        (2, 3)
    );
    assert_eq!(
        (
            item(&staged, "next.md").level,
            item(&staged, "next.md").position,
            item(&staged, "next.md").size
        ),
        (2, 1, 2)
    );
    assert_eq!(
        (
            item(&staged, "report.md").position,
            item(&staged, "report.md").size
        ),
        (2, 2)
    );
    panel.update(cx, |view, cx| {
        let selected = view
            .state
            .staged
            .iter()
            .position(|file| file.path == Path::new("docs/report.md"))
            .unwrap();
        view.state.selected_file = Some(CommitPanelFileRef::Staged { index: selected });
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, &app, window);
    assert_eq!(
        item(&recorded_tree("cp-unstaged-tree").unwrap(), "Fix {}.txt").selected,
        Some(false)
    );
    assert_eq!(
        item(&recorded_tree("cp-staged-tree").unwrap(), "report.md").selected,
        Some(true)
    );
    panel.update(cx, |view, cx| {
        view.state.generated_expanded = true;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, &app, window);
    let unstaged = recorded_tree("cp-unstaged-tree").unwrap();
    assert_eq!(
        item(&unstaged, Msg::GeneratedFilesSection.t()).expanded,
        Some(true)
    );
    let lock = item(&unstaged, "Cargo.lock");
    assert_eq!((lock.level, lock.position, lock.size), (2, 1, 1));
    i18n::set_lang(Lang::En);
    cx.run_until_parked();
    draw(cx, &app, window);
    let unstaged = recorded_tree("cp-unstaged-tree").unwrap();
    assert_eq!(unstaged.label, "Unstaged files");
    assert_eq!(
        item(&unstaged, "Fix {}.txt").label,
        "File Fix {}.txt, modified"
    );
    assert_eq!(
        item(&unstaged, "Cargo.lock").label,
        "File Cargo.lock, added"
    );
    // A status rebuild invalidates the cached layout even when disclosure
    // flags stay unchanged; all siblings, including off-screen ones, count.
    std::fs::write(repo.join("src/zeta.txt"), "new\n").unwrap();
    panel.update(cx, |view, cx| {
        view.state.reload_status(&repo);
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, &app, window);
    let unstaged = recorded_tree("cp-unstaged-tree").unwrap();
    let new_file = item(&unstaged, "zeta.txt");
    assert_eq!(
        (
            new_file.level,
            new_file.position,
            new_file.size,
            new_file.selected
        ),
        (2, 4, 4, Some(false))
    );
    panel.update(cx, |view, cx| {
        view.state.tree_view = false;
        cx.notify();
    });
    cx.run_until_parked();
    draw(cx, &app, window);
    assert!(recorded_tree("cp-unstaged-tree").is_none());
    assert!(recorded_tree("cp-staged-tree").is_none());
    drop(panel);
    i18n::set_lang(original_language);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS file_tree_roles: Editor and Commit Panel EN/JA TreeItems, hierarchy, selection, collapse, folds and status");
}
