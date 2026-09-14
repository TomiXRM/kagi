//! ADR-0197 S5: retained pane/resource ownership on real GPUI entities.

use crate::macos::{git, mount, unmount};
use gpui::{SharedString, VisualTestAppContext};
use std::path::{Path, PathBuf};

fn build_fixture(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), format!("{name} first\n")).expect("first file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    std::fs::write(repo.join("f.txt"), format!("{name} second\n")).expect("second file");
    git(&repo, &["commit", "-q", "-am", "second"]);
    repo.canonicalize().expect("canonical repo")
}

pub fn scenario_retained_pane_resources(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "pane-a");
    let repo_b = build_fixture(&root, "pane-b");
    let (app, window) = mount(cx, &repo_a);
    let owner_a = cx.read(|cx| app.read(cx).active_session().expect("A owner"));
    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
    });
    cx.run_until_parked();
    let owner_b = cx.read(|cx| app.read(cx).active_session().expect("B owner"));
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    cx.run_until_parked();

    // File History: its real git-log completion lands in A while B is active.
    let file_history = app.update(cx, |state, cx| {
        state.open_file_history(PathBuf::from("f.txt"), None, cx);
        let pane = state.ui().file_history.clone().expect("A file history");
        state.switch_repo(1, cx);
        state.status_footer =
            kagi::ui::FooterStatus::Idle(SharedString::from("B file-history sentinel"));
        pane
    });
    assert_eq!(
        owner_b,
        cx.read(|cx| app.read(cx).active_session().expect("B owner"))
    );
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.ui().file_history.is_none(),
            "file-history-owner-isolation: A pane leaked into B"
        );
        assert_eq!(
            state.ui[&owner_a]
                .file_history
                .as_ref()
                .map(|pane| pane.entity_id()),
            Some(file_history.entity_id()),
            "file-history-owner-retained: A lost its pane in the background",
        );
        assert!(
            file_history
                .read(cx)
                .data
                .history
                .as_ref()
                .is_some_and(|history| !history.entries.is_empty()),
            "file-history-background-owner: A completion did not land in A",
        );
        assert!(
            matches!(
                &state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == "B file-history sentinel"
            ),
            "file-history-footer-isolation: A completion changed B footer"
        );
        assert!(
            !kagi::ui::e2e::active_modal_present(state),
            "file-history-modal-isolation: A completion opened a modal on B",
        );
    });

    // Editor: the file read and a git-backed background read (History) update
    // the retained entity, not B's pane / footer. The History load is requested
    // while A is active but delivered after the switch, proving the pane's own
    // reads land in their frozen owner even in the background (ADR-0197 決定 3).
    let editor = app.update(cx, |state, cx| {
        state.switch_repo(0, cx);
        state.close_file_history();
        state.open_editor_workspace(cx);
        let pane = state.ui().editor_workspace.clone().expect("A editor");
        pane.update(cx, |view, cx| view.open_tab(PathBuf::from("f.txt"), cx));
        pane
    });
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |view, cx| {
            view.set_right_tab(kagi_ui_editor::RightPaneTab::History, window, cx)
        });
        app.update(cx, |state, cx| {
            state.switch_repo(1, cx);
            state.status_footer =
                kagi::ui::FooterStatus::Idle(SharedString::from("B editor sentinel"));
        });
    })
    .expect("editor window");
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(state.active_session(), Some(owner_b));
        assert!(
            state.ui().editor_workspace.is_none(),
            "editor-owner-isolation: A editor leaked into B"
        );
        assert!(
            editor.read(cx).content.is_some(),
            "editor-background-owner: A file completion did not land in A"
        );
        assert!(
            editor
                .read(cx)
                .history
                .as_ref()
                .is_some_and(|history| !history.entries.is_empty()),
            "editor-background-read-owner: A History load did not land in A"
        );
        assert_eq!(
            state.ui[&owner_a]
                .editor_workspace
                .as_ref()
                .map(|pane| pane.entity_id()),
            Some(editor.entity_id()),
            "editor-owner-retained: A lost its editor",
        );
        assert!(
            matches!(
                &state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == "B editor sentinel"
            ),
            "editor-footer-isolation: A completion changed B footer"
        );
        assert!(
            !kagi::ui::e2e::active_modal_present(state),
            "editor-modal-isolation: A completion opened a modal on B",
        );
    });

    // Analyze: its mine and entity remain owned by A while B stays untouched.
    let ecosystem = app.update(cx, |state, cx| {
        state.switch_repo(0, cx);
        state.open_ecosystem_view(cx);
        let pane = state.ui().ecosystem.clone().expect("A Analyze pane");
        state.switch_repo(1, cx);
        state.status_footer =
            kagi::ui::FooterStatus::Idle(SharedString::from("B ecosystem sentinel"));
        pane
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state.ui().ecosystem.is_none(),
            "ecosystem-owner-isolation: A pane leaked into B"
        );
        assert!(
            state.ui[&owner_a].ecosystem_cache.is_some(),
            "ecosystem-background-owner: mine missed A"
        );
        assert_eq!(
            state.ui[&owner_a]
                .ecosystem
                .as_ref()
                .map(|pane| pane.entity_id()),
            Some(ecosystem.entity_id()),
            "ecosystem-owner-retained: A lost its pane",
        );
        assert!(
            matches!(
                &state.status_footer,
                kagi::ui::FooterStatus::Idle(text) if text.as_ref() == "B ecosystem sentinel"
            ),
            "ecosystem-footer-isolation: A completion changed B footer"
        );
        assert!(
            !kagi::ui::e2e::active_modal_present(state),
            "ecosystem-modal-isolation: A completion opened a modal on B",
        );
    });

    // Edited entity state and every pane identity return with A.
    editor.update(cx, |view, _| {
        view.content = Some("unsaved pane sentinel".to_string());
        view.dirty = true;
    });
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert_eq!(
            state
                .ui()
                .editor_workspace
                .as_ref()
                .map(|pane| pane.entity_id()),
            Some(editor.entity_id())
        );
        assert_eq!(
            editor.read(cx).content.as_deref(),
            Some("unsaved pane sentinel")
        );
        assert!(
            editor.read(cx).dirty,
            "pane-edit-restored: edited state was discarded on activation"
        );
    });

    // A live PTY and retained pane entities die with the owner. Weak handles
    // prove no detached subscription/resource sink keeps them alive.
    editor.update(cx, |view, _| view.dirty = false);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.ensure_terminal(window, cx));
    })
    .expect("terminal window");
    let (terminal_weak, editor_weak, history_weak, ecosystem_weak) = cx.read(|cx| {
        let state = app.read(cx);
        let terminal = state
            .ui()
            .terminal_session
            .as_ref()
            .and_then(|session| session.view.as_ref())
            .expect("live terminal")
            .downgrade();
        (
            terminal,
            editor.downgrade(),
            file_history.downgrade(),
            ecosystem.downgrade(),
        )
    });
    drop(editor);
    drop(file_history);
    drop(ecosystem);
    app.update(cx, |state, cx| state.close_tab(0, cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            !state.ui.contains_key(&owner_a),
            "close-resource-owner-drop: A state survived close"
        );
        assert!(
            terminal_weak.upgrade().is_none(),
            "close-terminal-drop: PTY view survived owner close"
        );
        assert!(
            editor_weak.upgrade().is_none(),
            "close-editor-drop: editor survived owner close"
        );
        assert!(
            history_weak.upgrade().is_none(),
            "close-file-history-drop: history survived owner close"
        );
        assert!(
            ecosystem_weak.upgrade().is_none(),
            "close-ecosystem-drop: Analyze survived owner close"
        );
        let _ = cx;
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS retained_pane_resources");
}
