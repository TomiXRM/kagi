//! Deterministic editor → host admission wiring; real remove contention is G/M.
use crate::macos::{build_fixture, mount, unmount};
use gpui::{Focusable, VisualTestAppContext};
use kagi::ui::{i18n::Msg, FooterStatus};
use std::time::{Duration, Instant};

pub fn scenario_editor_save_admission(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let before = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    // Same Sessions API as the bridge; no child process or pending remove
    // future for the real-platform dispatcher to wait on.
    let guard = app.update(cx, |app, _| app.app_sessions.write_lease(&repo).unwrap());
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));
    let edited = cx.read(|cx| {
        editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()
    });
    assert_ne!(edited.as_bytes(), before.as_slice());

    // Exercise save_editor_file → SaveRequested → reserve_write, not a direct
    // invocation of the pane executor or a manually raised Busy notification.
    app.update(cx, |app, cx| app.save_editor_file(cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| app.read(cx).app_sessions.has_leases()));
    assert_eq!(std::fs::read(repo.join("README.md")).unwrap(), before);
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "Busy must retain the buffer"
    );
    assert!(cx.read(|cx| matches!(&app.read(cx).status_footer,
        FooterStatus::Failed(message) if message.as_ref() == Msg::OpInProgress.t())));
    assert!(cx.read(|cx| app
        .read(cx)
        .toast_stack
        .as_ref()
        .unwrap()
        .read(cx)
        .toasts()
        .iter()
        .any(|toast| toast.message.as_ref() == Msg::OpInProgress.t())));
    assert_eq!(
        cx.read(|cx| editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        edited
    );

    guard.complete();
    assert!(cx.read(|cx| !app.read(cx).app_sessions.has_leases()));
    app.update(cx, |app, cx| app.save_editor_file(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !editor.read(cx).dirty && !app.read(cx).app_sessions.has_leases()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "admitted editor save did not settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        std::fs::read(repo.join("README.md")).unwrap(),
        edited.as_bytes()
    );
    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor admission → Busy preserves bytes/buffer → release → save writes edited bytes");
}

/// #486: a background save is bound to the buffer it was issued from. Start a
/// save on README.md, switch to a dirty second buffer before the write lands,
/// and the completion must leave the *visible* buffer alone while still
/// settling the buffer it actually belongs to.
pub fn scenario_editor_save_buffer_identity(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let other_before = "other file\n";
    std::fs::write(repo.join("other.txt"), other_before).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();

    // Open a tab, wait for its buffer, focus it, and type one character so it
    // is dirty. Returns the buffer's edited text.
    let mut open_and_edit = |cx: &mut VisualTestAppContext, path: &str, key: &str| -> String {
        let target = std::path::PathBuf::from(path);
        editor.update(cx, |view, cx| view.open_tab(target.clone(), cx));
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            cx.run_until_parked();
            cx.update_window(window, |_, window, cx| window.draw(cx).clear())
                .unwrap();
            if cx.read(|cx| {
                let v = editor.read(cx);
                v.open_path.as_deref() == Some(target.as_path())
                    && v.editor.is_some()
                    && v.content.is_some()
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "{path} did not load");
            std::thread::sleep(Duration::from_millis(2));
        }
        cx.update_window(window, |_, window, cx| {
            let input = editor.read(cx).editor.clone().unwrap();
            window.focus(&input.read(cx).focus_handle(cx), cx);
            window.draw(cx).clear();
        })
        .unwrap();
        cx.simulate_keystrokes(window, key);
        cx.run_until_parked();
        assert!(cx.read(|cx| editor.read(cx).dirty), "{path} must be dirty");
        cx.read(|cx| {
            editor
                .read(cx)
                .editor
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string()
        })
    };

    // Two dirty buffers; README.md is the active one, so it is the buffer the
    // save gets issued from.
    let other_edited = open_and_edit(cx, "other.txt", "y");
    let readme_edited = open_and_edit(cx, "README.md", "x");

    // Issue the save, then switch away before its background write lands — the
    // completion is delivered while the pane holds a different buffer.
    app.update(cx, |app, cx| app.save_editor_file(cx));
    editor.update(cx, |view, cx| view.open_tab("other.txt".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !app.read(cx).app_sessions.has_leases())
            && std::fs::read(repo.join("README.md")).unwrap() == readme_edited.as_bytes()
        {
            break;
        }
        assert!(Instant::now() < deadline, "editor save did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }

    // The visible buffer belongs to another file: untouched and still dirty.
    assert_eq!(
        cx.read(|cx| editor.read(cx).open_path.clone()),
        Some(std::path::PathBuf::from("other.txt"))
    );
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "the save must not clean the buffer it did not come from"
    );
    assert_eq!(
        cx.read(|cx| editor
            .read(cx)
            .editor
            .as_ref()
            .unwrap()
            .read(cx)
            .value()
            .to_string()),
        other_edited
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("other.txt")).unwrap(),
        other_before
    );
    // …while the buffer the save DID come from settled clean in its own tab.
    assert!(
        cx.read(|cx| !editor.read(cx).tab_dirty(std::path::Path::new("README.md"))),
        "the saved buffer must settle clean in its own tab"
    );

    // Reactivating it restores the saved snapshot, with no false conflict.
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    cx.run_until_parked();
    assert!(cx.read(|cx| !editor.read(cx).dirty));
    assert!(cx.read(|cx| !editor.read(cx).external_changed));
    assert_eq!(
        cx.read(|cx| editor.read(cx).content.clone()),
        Some(readme_edited)
    );

    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor save binds to its own buffer → other tab keeps its edits");
}

/// #736: the watcher only says "something under the working tree changed", and
/// kagi's own fetch or save fires it. A dirty buffer must not be told its file
/// changed on disk unless the bytes really differ — the banner it raises offers
/// Reload, which discards the edit.
pub fn scenario_editor_external_change_banner(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let original = std::fs::read(repo.join("README.md")).unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));

    // An unrelated worktree event — exactly what an auto-fetch or kagi's own
    // save produces. README.md itself is untouched, so no banner.
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| !editor.read(cx).external_changed),
        "an unrelated worktree event must not claim the open file changed on disk"
    );
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "the edit itself must survive the probe"
    );

    // Now change the file for real: the banner is exactly what should appear.
    // A different length is the cheap half of that check — the probe must not
    // read a file that an external process swapped for a huge one.
    std::fs::write(
        repo.join("README.md"),
        "changed by someone else, and at a different length\n",
    )
    .unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| editor.read(cx).external_changed) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a real external change must raise the banner"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    // Put the file back. The banner has to come down with it: the disk matches
    // the buffer's snapshot again, and while it is up `save_impl` refuses an
    // ordinary save over a conflict that no longer exists.
    std::fs::write(repo.join("README.md"), &original).unwrap();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !editor.read(cx).external_changed) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "restoring the file must lower the banner"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "lowering the banner must not touch the edit"
    );

    drop(editor);
    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS editor banner follows the file's bytes, not the watcher's coarseness"
    );
}

/// #736 review: the two ways the probe can lie about a buffer that nobody
/// changed — a rename moves the path out from under it, and a save lands while
/// it is in flight. Both used to end in the same false Reload offer.
pub fn scenario_editor_banner_rename_and_save(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let (app, window) = mount(cx, &repo);
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .unwrap();
    editor.update(cx, |view, cx| view.open_tab("README.md".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())
            .unwrap();
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(cx.read(|cx| editor.read(cx).dirty));

    // Rename the file under the dirty buffer, exactly as the tree's rename
    // does, and move the bytes with it. The buffer's own path changes; its
    // content does not. A probe keyed on a path-dependent hash would call that
    // a change on every future event.
    std::fs::rename(repo.join("README.md"), repo.join("RENAMED.md")).unwrap();
    editor.update(cx, |view, cx| {
        view.remap_renamed_path(
            std::path::Path::new("README.md"),
            std::path::Path::new("RENAMED.md"),
            cx,
        )
    });
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| !editor.read(cx).external_changed),
        "a rename that moved the same bytes must not read as an external change"
    );

    // Saving and editing again must also leave the banner down. This does NOT
    // exercise the in-flight case the identity re-check exists for: the
    // dispatcher drains the probe before the save can land, so the ordering
    // that makes a stale result observable cannot be produced here. Removing
    // that re-check does not fail this scenario — it is carried by review, not
    // by this test.
    app.update(cx, |app, cx| app.save_editor_file(cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| !editor.read(cx).dirty && !app.read(cx).app_sessions.has_leases()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor save did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    cx.simulate_keystrokes(window, "y");
    cx.run_until_parked();
    editor.update(cx, |view, cx| view.on_worktree_changed(cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| !editor.read(cx).external_changed),
        "an edit on top of a just-saved buffer must not read as an external change"
    );

    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS editor banner survives a rename and a save-then-edit");
}
