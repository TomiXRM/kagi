//! Branch-only copy follows the selected commit, never unrelated checked-out HEAD.
use gpui::{AnyWindowHandle, ClipboardItem, Entity, VisualTestAppContext};
use kagi::ui::{commands, settings, KagiApp};

use crate::keyboard_nav::keys;
use crate::macos::{build_fixture, git, mount, mount_state, repo_fingerprint, unmount};

fn focus_root(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, win: AnyWindowHandle) {
    cx.update_window(win, |_, window, cx| {
        window.draw(cx).clear();
        app.read(cx)
            .root_focus
            .clone()
            .expect("root focus")
            .focus(window, cx);
    })
    .unwrap();
}

fn expect_unchanged(cx: &mut VisualTestAppContext, win: AnyWindowHandle) {
    cx.write_to_clipboard(ClipboardItem::new_string("clipboard sentinel".into()));
    keys(cx, win, "cmd-shift-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("clipboard sentinel")
    );
}

pub fn scenario_graph_copy_selected_branch(cx: &mut VisualTestAppContext) {
    let _saved = crate::gui_isolation::SavedKeys::keep(&["graph_copy_target", "recent_repos"]);
    let _ports = crate::gui_isolation::PortStore::keep();
    let _shell = crate::gui_isolation::StandInShell::install();
    let _gh = crate::pr_fields_focus::OfflineGh::with_script(
        "#!/bin/sh\ncase \"$1 $2\" in 'repo list') echo '[]' ;; *) exit 1 ;; esac\n",
    );
    settings::write_setting("graph_copy_target", Some("hash"));
    let fixture = build_fixture();
    git(
        fixture.path(),
        &["commit", "--allow-empty", "-qm", "copy selection tip"],
    );
    let branch = "feature/日本語-copy";
    git(fixture.path(), &["branch", "-m", branch]);
    // The selected row's local branch differs from checked-out HEAD.
    let selected_branch = "selected/別の-branch";
    git(fixture.path(), &["branch", selected_branch, "HEAD~1"]);
    git(fixture.path(), &["tag", "selected-tag", "HEAD~2"]);
    git(
        fixture.path(),
        &["update-ref", "refs/remotes/origin/only", "HEAD~2"],
    );
    let before = repo_fingerprint(fixture.path());
    let (app, win) = mount(cx, fixture.path());
    let selected_sha = app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = Some(1);
        cx.notify();
        app.view().rows[1].id.0.clone()
    });
    focus_root(cx, &app, win);
    keys(cx, win, "cmd-shift-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(selected_branch)
    );
    keys(cx, win, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(selected_sha.as_str())
    );
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = None;
        cx.notify();
    });
    focus_root(cx, &app, win);
    expect_unchanged(cx, win);
    cx.dispatch_action(win, commands::CopySelectedBranchName);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("clipboard sentinel"),
        "no selection must not fall back to the checked-out branch"
    );
    let unbranched_sha = app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = Some(2);
        cx.notify();
        app.view().rows[2].id.0.clone()
    });
    focus_root(cx, &app, win);
    settings::write_setting("graph_copy_target", Some("branch"));
    expect_unchanged(cx, win);
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Disabled(_)
        ))
    });
    keys(cx, win, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(unbranched_sha.as_str()),
        "ordinary Cmd+C retains its configured hash fallback; branch-only copy does not"
    );
    settings::write_setting("graph_copy_target", Some("hash"));
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = Some(1);
        cx.notify();
    });
    focus_root(cx, &app, win);

    // The palette's real Input, a context menu, and a modal own keyboard focus.
    keys(cx, win, "cmd-p");
    cx.read(|cx| {
        assert!(
            app.read(cx).menu_overlay.is_some(),
            "palette Input must be open"
        )
    });
    expect_unchanged(cx, win);
    keys(cx, win, "escape");
    keys(cx, win, "cmd-shift-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(selected_branch),
        "shortcut must work after palette returns focus without an explicit redraw"
    );
    app.update(cx, |app, cx| {
        app.select(0);
        cx.notify();
    });
    focus_root(cx, &app, win);
    keys(cx, win, "shift-f10");
    cx.read(|cx| {
        assert!(
            app.read(cx).commit_menu.is_some(),
            "context menu must own focus"
        )
    });
    expect_unchanged(cx, win);
    keys(cx, win, "escape");
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("app.about", window, cx)
        });
    })
    .unwrap();
    expect_unchanged(cx, win);
    keys(cx, win, "escape");

    // Editor mode hides Graph even if root focus is explicitly restored.
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.toggleEditorWorkspace", window, cx)
        });
    })
    .unwrap();
    focus_root(cx, &app, win);
    expect_unchanged(cx, win);
    // An enabled native Branch menu action must still work from Editor.
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Enabled
        ))
    });
    cx.dispatch_action(win, commands::CopySelectedBranchName);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(branch)
    );
    cx.update_window(win, |_, window, cx| {
        app.update(cx, |app, cx| {
            app.handle_menu_command("view.showGraph", window, cx)
        });
    })
    .unwrap();

    focus_root(cx, &app, win);
    if cx.read(|cx| app.read(cx).bottom_panel_open) {
        keys(cx, win, "cmd-j");
    }
    keys(cx, win, "cmd-j");
    cx.read(|cx| {
        assert!(app.read(cx).bottom_panel_open);
        assert_eq!(app.read(cx).bottom_tab, kagi::ui::BottomTab::Terminal);
    });
    crate::app_conflict::click_control(cx, win, "tab-terminal");
    assert!(
        cx.update_window(win, |_, window, cx| {
            app.read(cx).terminal_focused_for_e2e(window, cx)
        })
        .unwrap(),
        "Terminal must own focus before testing its clipboard shortcut"
    );
    expect_unchanged(cx, win);
    keys(cx, win, "cmd-j");

    // Home may retain repository tabs behind it, but none is on screen.
    focus_root(cx, &app, win);
    keys(cx, win, "cmd-t");
    cx.read(|cx| assert!(app.read(cx).home_in_front()));
    expect_unchanged(cx, win);
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Disabled(_)
        ));
    });
    cx.dispatch_action(win, commands::CopySelectedBranchName);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("clipboard sentinel"),
        "an explicit Branch menu action must not copy from behind Home"
    );
    keys(cx, win, "cmd-w");

    // The next owner has a symbolic branch but no commit/ref enumeration yet.
    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), &["init", "-q", "-b", "new/未作成"]);
    app.update(cx, |app, cx| {
        assert!(app.open_repository(unborn.path().to_path_buf(), cx))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !cx.read(|cx| app.read(cx).view().current_branch.as_deref() == Some("new/未作成")) {
        assert!(
            std::time::Instant::now() < deadline,
            "new owner's branch read did not settle"
        );
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    focus_root(cx, &app, win);
    expect_unchanged(cx, win);
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Disabled(_)
        ))
    });
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    focus_root(cx, &app, win);
    keys(cx, win, "cmd-shift-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(branch),
        "returning to the first owner must not copy the unborn tab's name"
    );
    assert_eq!(
        before,
        repo_fingerprint(fixture.path()),
        "copy commands must not mutate Git"
    );
    unmount(cx, app, win);

    let detached = build_fixture();
    git(detached.path(), &["checkout", "-q", "--detach"]);
    let before = repo_fingerprint(detached.path());
    let (app, win) = mount(cx, detached.path());
    focus_root(cx, &app, win);
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Disabled(_)
        ))
    });
    expect_unchanged(cx, win);
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = Some(0);
        cx.notify();
    });
    focus_root(cx, &app, win);
    keys(cx, win, "cmd-shift-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("main"),
        "detached HEAD must not hide a local branch on the selected commit"
    );
    app.update(cx, |app, cx| {
        app.ui_mut().expect("active session").selected = Some(1);
        cx.notify();
    });
    focus_root(cx, &app, win);
    expect_unchanged(cx, win);
    assert_eq!(before, repo_fingerprint(detached.path()));
    unmount(cx, app, win);

    let (app, win) = mount_state(cx, KagiApp::with_error(""));
    cx.read(|cx| {
        assert!(matches!(
            commands::command_state(app.read(cx), "branch.copySelectedName"),
            commands::CommandState::Disabled(_)
        ))
    });
    expect_unchanged(cx, win);
    unmount(cx, app, win);
    eprintln!("[gui-e2e] PASS graph_copy_selected_branch selection/refs/unborn/detached/contexts");
}
