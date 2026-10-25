//! Real index/worktree deletion and Git-parked namespace routes (#1734/#1735).
use super::{click_control, content_fixture, git, git_fixture, mount, unmount, wait_idle};
use gpui::VisualTestAppContext;
use kagi::ui::{
    e2e,
    i18n::{self, Lang},
};
use std::path::Path;

fn deletion_fixture(current: bool, incoming: bool, raw: bool) -> tempfile::TempDir {
    let fixture = content_fixture();
    let path = fixture.path();
    let repository = git2::Repository::open(path).unwrap();
    let mut index = repository.index().unwrap();
    let conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    let mut ancestor = conflict.ancestor.unwrap();
    if raw {
        ancestor.id = repository.blob(b"base\0bytes").unwrap();
    }
    index.clear().unwrap();
    index.add(&ancestor).unwrap();
    if current {
        let mut entry = conflict.our.unwrap();
        if raw {
            entry.id = repository.blob(b"current\0bytes").unwrap();
        }
        index.add(&entry).unwrap();
    }
    if incoming {
        let mut entry = conflict.their.unwrap();
        if raw {
            entry.id = repository.blob(b"incoming\0bytes").unwrap();
        }
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    if current || incoming {
        std::fs::write(
            path.join("file.txt"),
            if raw {
                &b"modified\0bytes"[..]
            } else {
                &b"modified\n"[..]
            },
        )
        .unwrap();
    } else {
        std::fs::remove_file(path.join("file.txt")).unwrap();
    }
    drop(index);
    drop(repository);
    fixture
}

fn index_identity(path: &Path) -> Vec<(Vec<u8>, git2::Oid, u32, u16)> {
    git2::Repository::open(path)
        .unwrap()
        .index()
        .unwrap()
        .iter()
        .map(|entry| (entry.path, entry.id, entry.mode, entry.flags & 0x3000))
        .collect()
}

fn delete_leg(cx: &mut VisualTestAppContext, lang: Lang, current: bool, incoming: bool, raw: bool) {
    let fixture = deletion_fixture(current, incoming, raw);
    let repo = fixture.path().canonicalize().unwrap();
    let before = index_identity(&repo);
    let bytes = std::fs::read(repo.join("file.txt")).ok();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    kagi::ui::conflict_view::take_compact_labels_for_e2e();
    click_control(cx, window, "conflict-row-0");
    assert!(e2e::control_bounds(window.window_id(), "conflict-delete").is_some());
    if current || incoming {
        assert!(e2e::control_bounds(window.window_id(), "conflict-use-modified").is_some());
    }
    let labels = kagi::ui::conflict_view::take_compact_labels_for_e2e();
    let expected_delete = match (lang, current || incoming) {
        (Lang::En, false) => "Confirm deletion (deleted on both sides)…",
        (Lang::Ja, false) => "削除を確定（両側で削除済み）…",
        (Lang::En, true) => "Use deleted side…",
        (Lang::Ja, true) => "削除側を採用…",
    };
    assert_eq!(labels.get("conflict-delete"), Some(&expected_delete));
    assert_eq!(
        labels.contains_key("conflict-use-modified"),
        current || incoming
    );
    for cancel in ["button", "enter", "escape"] {
        click_control(cx, window, "conflict-delete");
        cx.read(|cx| {
            let app = app.read(cx);
            let modal = app
                .conflict_resolution_modal()
                .expect("explicit confirmation required");
            assert_eq!(
                modal.changes.remove_index,
                vec![Path::new("file.txt").to_path_buf()]
            );
        });
        let list = e2e::control_bounds(window.window_id(), "conflict-resolution-paths")
            .expect("visible path list");
        let filename = e2e::control_bounds(window.window_id(), "modal-path-name-file.txt-file.txt")
            .expect("painted deletion target");
        assert!(
            filename.size.height >= gpui::px(12.),
            "filename must not collapse"
        );
        assert!(filename.size.width > gpui::px(0.));
        assert!(filename.origin.y >= list.origin.y && filename.bottom() <= list.bottom());
        match cancel {
            "button" => click_control(cx, window, "conflict-resolution-cancel"),
            key => {
                cx.simulate_keystrokes(window, key);
                cx.run_until_parked();
            }
        }
        assert_eq!(index_identity(&repo), before);
        assert_eq!(std::fs::read(repo.join("file.txt")).ok(), bytes);
        assert!(kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100).is_empty());
        if cancel == "enter" {
            assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_some()));
            click_control(cx, window, "conflict-resolution-cancel");
        }
        assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_none()));
    }
    click_control(cx, window, "conflict-delete");
    click_control(cx, window, "conflict-resolution-confirm");
    wait_idle(cx, &app);
    assert!(!repo.join("file.txt").exists());
    assert!(!git2::Repository::open(&repo)
        .unwrap()
        .index()
        .unwrap()
        .has_conflicts());
    assert_eq!(
        kagi_git::oplog::read_oplog_tail_for_repo(&repo, 100)
            .iter()
            .filter(|entry| entry.op == "conflict-save:merge")
            .count(),
        1
    );
    assert!(repo.join(".git/MERGE_HEAD").exists());
    click_control(cx, window, "conflict-continue");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().commit_panel_open));
    unmount(cx, app, window);
}

fn parked_leg(cx: &mut VisualTestAppContext, lang: Lang, keep_file: bool) {
    let fixture = tempfile::tempdir().unwrap();
    let repo = fixture.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("base"), "base\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "base"]);
    git(repo, &["checkout", "-qb", "file-side"]);
    std::fs::write(repo.join("thing"), "file side\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "file"]);
    git(repo, &["checkout", "-q", "main"]);
    std::fs::create_dir(repo.join("thing")).unwrap();
    std::fs::write(repo.join("thing/child"), "child\n").unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "directory"]);
    assert!(!git_fixture::git_succeeds(repo, &["merge", "file-side"]));
    let parked = std::fs::read_dir(repo)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("thing~")
        })
        .expect("Git parked the file");
    let before = index_identity(repo);
    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| {
        app.set_lang(lang, cx);
        app.detect_conflict_mode(cx);
    });
    cx.run_until_parked();
    click_control(cx, window, "conflict-row-0");
    let choice = if keep_file {
        "conflict-keep-file"
    } else {
        "conflict-keep-directory"
    };
    click_control(cx, window, choice);
    cx.read(|cx| {
        let app = app.read(cx);
        let changes = &app.conflict_resolution_modal().unwrap().changes;
        if keep_file {
            assert!(changes.restore.contains(&"thing".into()));
        } else {
            assert!(changes.keep.contains(&"thing/child".into()));
        }
    });
    click_control(cx, window, "conflict-resolution-cancel");
    assert_eq!(index_identity(repo), before);
    assert!(parked.exists());
    for key in ["enter", "escape"] {
        click_control(cx, window, choice);
        cx.simulate_keystrokes(window, key);
        cx.run_until_parked();
        if key == "enter" {
            assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_some()));
            click_control(cx, window, "conflict-resolution-cancel");
        }
        assert!(cx.read(|cx| app.read(cx).conflict_resolution_modal().is_none()));
        assert_eq!(index_identity(repo), before);
        assert_eq!(std::fs::read(&parked).unwrap(), b"file side\n");
        assert_eq!(std::fs::read(repo.join("thing/child")).unwrap(), b"child\n");
    }
    click_control(cx, window, choice);
    click_control(cx, window, "conflict-resolution-confirm");
    wait_idle(cx, &app);
    assert!(!parked.exists(), "renamed source must be absent");
    if keep_file {
        assert_eq!(std::fs::read(repo.join("thing")).unwrap(), b"file side\n");
    } else {
        assert_eq!(std::fs::read(repo.join("thing/child")).unwrap(), b"child\n");
    }
    assert!(!git2::Repository::open(repo)
        .unwrap()
        .index()
        .unwrap()
        .has_conflicts());
    click_control(cx, window, "conflict-continue");
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).ui().commit_panel_open));
    unmount(cx, app, window);
}

pub(super) fn run(cx: &mut VisualTestAppContext, lang: Lang) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["lang"]);
    let original = i18n::lang();
    for raw in [false, true] {
        for (current, incoming) in [(false, false), (true, false), (false, true)] {
            delete_leg(cx, lang, current, incoming, raw);
        }
    }
    for keep_file in [false, true] {
        parked_leg(cx, lang, keep_file);
    }
    i18n::set_lang(original);
}
