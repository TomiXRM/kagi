//! ADR-0197 S5: retained pane/resource ownership on real GPUI entities.

use crate::macos::{git, mount, unmount};
use gpui::{Focusable, SharedString, VisualTestAppContext};
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

/// `git diff --cached` file count for `repo` — the authoritative index state,
/// independent of any panel's in-memory view.
fn staged_count(repo: &Path) -> usize {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(["diff", "--cached", "--name-only"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git diff --cached");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .count()
}

/// #722 P1-c: a Commit Panel's Stage/Unstage is deferred to the next tick
/// (`spawn_in`). If the user switches tabs before it runs, the owner frozen at
/// the click must keep it from writing the index of the tab now on screen
/// (ADR-0197 決定 5). `do_stage_file` now takes `owner: SessionId`, so a callback
/// that omits it cannot compile.
pub fn scenario_commit_stage_deferred_owner(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "stage-a");
    let repo_b = build_fixture(&root, "stage-b");
    // Both repos carry an unstaged change so each panel lists a stageable file.
    std::fs::write(repo_a.join("f.txt"), "stage-a second\nstage-a unstaged\n").unwrap();
    std::fs::write(repo_b.join("f.txt"), "stage-b second\nstage-b unstaged\n").unwrap();

    let (app, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    let owner_a = app.update(cx, |state, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_a.clone(), cx);
        state.active_session().expect("A owner")
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app
            .read(cx)
            .ui()
            .commit_panel
            .as_ref()
            .is_some_and(|panel| { !panel.read(cx).state.unstaged.is_empty() })),
        "precondition: A's panel lists an unstaged file"
    );

    let owner_b = app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_b.clone(), cx);
        state.active_session().expect("B owner")
    });
    cx.run_until_parked();
    assert_eq!(staged_count(&repo_a), 0, "A index clean before");
    assert_eq!(staged_count(&repo_b), 0, "B index clean before");

    // A's deferred Stage lands after the switch to B.
    app.update(cx, |state, cx| state.do_stage_file(owner_a, 0, cx));
    cx.run_until_parked();
    cx.read(|cx| assert_eq!(app.read(cx).active_session(), Some(owner_b)));
    assert_eq!(
        staged_count(&repo_b),
        0,
        "stage-deferred-owner: A's Stage wrote B's index"
    );
    assert_eq!(
        staged_count(&repo_a),
        0,
        "stage-deferred-owner: A's Stage ran even though A was in the background"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_stage_deferred_owner");
}

/// #722 P1 (round 2): `close_tab(index)` can target a background tab, so its
/// dirty-editor check and disposal must look at `tabs[index].session`, not the
/// tab on screen (ADR-0197 決定 5). Reading the active editor would drop a dirty
/// background tab without prompting, or discard the active tab when a *clean*
/// background tab is closed.
pub fn scenario_close_tab_editor_dirty_owner(cx: &mut VisualTestAppContext) {
    // Part A — a DIRTY background tab must prompt before it is dropped.
    {
        let root_dir = tempfile::tempdir().expect("tempdir");
        let root = root_dir.path().canonicalize().unwrap();
        let repo_a = build_fixture(&root, "closeA-a");
        let repo_b = build_fixture(&root, "closeA-b");
        let (app, window) = mount(cx, &repo_a);
        cx.run_until_parked();
        app.update(cx, |state, cx| state.open_editor_workspace(cx));
        let owner_b = app.update(cx, |state, cx| {
            assert!(state.open_repository(repo_b.clone(), cx), "open B");
            state.open_editor_workspace(cx);
            state.active_session().expect("B owner")
        });
        cx.run_until_parked();
        app.update(cx, |state, cx| {
            state
                .ui()
                .editor_workspace
                .clone()
                .expect("B editor")
                .update(cx, |v, _| v.dirty = true);
        });
        app.update(cx, |state, cx| state.switch_repo(0, cx));
        cx.run_until_parked();
        let b_index = cx.read(|cx| {
            app.read(cx)
                .tabs
                .iter()
                .position(|t| t.session == owner_b)
                .expect("B tab")
        });
        app.update(cx, |state, cx| state.close_tab(b_index, cx));
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                state.editor_dirty_guard_modal().is_some(),
                "close-bg-dirty-prompts: a dirty background tab closed without a prompt"
            );
            assert!(
                state.tabs.iter().any(|t| t.session == owner_b),
                "close-bg-dirty-prompts: B was dropped before confirmation"
            );
        });
        unmount(cx, app, window);
    }

    // Part B — with only the ACTIVE tab dirty, closing a CLEAN background tab
    // must not prompt and must not discard the active tab's workspace.
    {
        let root_dir = tempfile::tempdir().expect("tempdir");
        let root = root_dir.path().canonicalize().unwrap();
        let repo_a = build_fixture(&root, "closeB-a");
        let repo_b = build_fixture(&root, "closeB-b");
        let (app, window) = mount(cx, &repo_a);
        cx.run_until_parked();
        let owner_a = cx.read(|cx| app.read(cx).active_session().expect("A owner"));
        app.update(cx, |state, cx| state.open_editor_workspace(cx));
        app.update(cx, |state, cx| {
            state
                .ui()
                .editor_workspace
                .clone()
                .expect("A editor")
                .update(cx, |v, _| v.dirty = true);
        });
        let owner_b = app.update(cx, |state, cx| {
            assert!(state.open_repository(repo_b.clone(), cx), "open B");
            state.open_editor_workspace(cx);
            state.active_session().expect("B owner")
        });
        cx.run_until_parked();
        app.update(cx, |state, cx| state.switch_repo(0, cx));
        cx.run_until_parked();
        let b_index = cx.read(|cx| {
            app.read(cx)
                .tabs
                .iter()
                .position(|t| t.session == owner_b)
                .expect("B tab")
        });
        app.update(cx, |state, cx| state.close_tab(b_index, cx));
        cx.run_until_parked();
        cx.read(|cx| {
            let state = app.read(cx);
            assert!(
                state.editor_dirty_guard_modal().is_none(),
                "close-clean-bg-no-prompt: closing a clean background tab read the active dirty editor"
            );
            assert!(
                !state.tabs.iter().any(|t| t.session == owner_b),
                "close-clean-bg-no-prompt: B did not close"
            );
            assert!(
                state.ui[&owner_a].editor_workspace.is_some(),
                "close-clean-bg-no-prompt: the active tab's editor was discarded"
            );
        });
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS close_tab_editor_dirty_owner");
}

/// #722 P2 (round 2): returning to a tab whose Commit Panel was retained must
/// revalidate the panel against the snapshot the activation read installs — the
/// reload path already does, so both must (ADR-0197 決定 3). Otherwise the
/// staged/unstaged lists stay frozen at the pre-departure state.
pub fn scenario_commit_panel_revalidates_on_activation(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "reval-a");
    let repo_b = build_fixture(&root, "reval-b");
    std::fs::write(repo_a.join("f.txt"), "reval-a second\nreval-a unstaged\n").unwrap();

    let (app, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    let owner_a = app.update(cx, |state, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_a.clone(), cx);
        state.active_session().expect("A owner")
    });
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui[&owner_a]
            .commit_panel
            .as_ref()
            .is_some_and(|e| {
                let st = &e.read(cx).state;
                !st.unstaged.is_empty() && st.staged.is_empty()
            })),
        "precondition: A's panel shows the unstaged file"
    );

    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
    });
    cx.run_until_parked();
    // External stage while A is in the background: f.txt moves unstaged → staged.
    git(&repo_a, &["add", "f.txt"]);
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui[&owner_a].commit_panel.as_ref().is_some_and(|e| {
            let st = &e.read(cx).state;
            !st.staged.is_empty() && st.unstaged.is_empty()
        })),
        "panel-revalidates-on-activation: returning to A did not refresh the Commit Panel against the external `git add`"
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_panel_revalidates_on_activation");
}

/// Render one frame, driving the `scans_stale` arm that calls
/// `start_wip_diffstat_scan` (`render.rs:186`).
fn draw_frame(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    cx.update_window(window, |_, window, cx| window.draw(cx).clear())
        .expect("draw frame");
    cx.run_until_parked();
}

/// #722 P1 (round 3): **starting with no tab must not panic.**
///
/// A normal launch with no saved session (`main.rs:170`) and a failed
/// repository open (`main.rs:200`) both build `KagiApp::with_error`, which has
/// no tab and therefore no session owning the screen. The first render
/// processes `scans_stale` and reaches `start_wip_diffstat_scan`; while that
/// wrote a default through the active writer it panicked *before Welcome was
/// ever drawn*, so a new install could not start the app at all.
///
/// Every scenario before this one mounted an open repository, which is exactly
/// why 23/23 green CI and this crash were not a contradiction.
pub fn scenario_welcome_startup_renders(cx: &mut VisualTestAppContext) {
    for (label, state) in [
        ("welcome-no-session", kagi::ui::KagiApp::with_error("")),
        (
            "welcome-open-failed",
            kagi::ui::KagiApp::with_error("could not open repository"),
        ),
    ] {
        let (app, window) = crate::macos::mount_state(cx, state);
        assert!(
            cx.read(|cx| app.read(cx).active_session().is_none()),
            "{label}: precondition — Welcome must have no owning session",
        );
        // The mount's own first frame is the one that used to panic: reaching
        // this line at all means Welcome rendered. `scans_stale` being consumed
        // proves that frame really ran the arm, rather than skipping it.
        assert!(
            !cx.read(|cx| app.read(cx).scans_stale),
            "{label}: the first frame never reached the scan arm, so the crash \
             path was not exercised",
        );
        // Re-arm and draw again: the repeat render must stay owner-free too.
        app.update(cx, |state, _| state.scans_stale = true);
        draw_frame(cx, window);
        assert!(
            !cx.read(|cx| app.read(cx).scans_stale),
            "{label}: the re-armed frame never reached the scan arm",
        );
        unmount(cx, app, window);
    }
    eprintln!("[gui-e2e] PASS welcome_startup_renders");
}

/// #722 P1 (round 3): closing the **last** tab returns to Welcome, so every
/// render after it runs with no owning session too.
pub fn scenario_close_last_tab_welcome_renders(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo = build_fixture(&root, "welcome-close");
    let (app, window) = mount(cx, &repo);
    assert!(
        cx.read(|cx| app.read(cx).active_session().is_some()),
        "precondition: the mounted repo owns the screen"
    );

    app.update(cx, |state, cx| state.close_tab(0, cx));
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).active_session().is_none()),
        "close-last-tab-welcome: closing the last tab must release its session",
    );

    // The post-close frame is the one that used to panic.
    app.update(cx, |state, _| state.scans_stale = true);
    draw_frame(cx, window);
    assert!(
        !cx.read(|cx| app.read(cx).scans_stale),
        "close-last-tab-welcome: the post-close frame never reached the scan arm",
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS close_last_tab_welcome_renders");
}

/// #722 P2 (round 3): **Connect Remote must not discard an unsaved buffer.**
///
/// `enter_remote_view` keeps the local session attached and merely moves to
/// another tab, so it is not a path that destroys the editor's owner. It used
/// to open the dirty guard anyway, whose `EnterRemoteView` arm then called
/// `close_editor_workspace()` — leaving the user with "abandon the connection"
/// or "throw away the edit" as the only two choices.
pub fn scenario_remote_connect_keeps_dirty_editor(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo = build_fixture(&root, "remote-dirty");
    let (app, window) = mount(cx, &repo);
    let local = cx.read(|cx| app.read(cx).active_session().expect("local owner"));

    // A dirty editor buffer on the local tab.
    app.update(cx, |state, cx| state.open_editor_workspace(cx));
    let editor = cx
        .read(|cx| app.read(cx).ui().editor_workspace.clone())
        .expect("editor workspace");
    let target = PathBuf::from("f.txt");
    editor.update(cx, |view, cx| view.open_tab(target.clone(), cx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        draw_frame(cx, window);
        if cx.read(|cx| {
            let v = editor.read(cx);
            v.open_path.as_deref() == Some(target.as_path()) && v.editor.is_some()
        }) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "f.txt did not load");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    cx.update_window(window, |_, window, cx| {
        let input = editor.read(cx).editor.clone().unwrap();
        window.focus(&input.read(cx).focus_handle(cx), cx);
        window.draw(cx).clear();
    })
    .unwrap();
    cx.simulate_keystrokes(window, "x");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "precondition: the local editor must be dirty"
    );
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

    // Connect Remote. Owner-preserving navigation: no prompt, no discard.
    let host = kagi_domain::remote::RemoteHost::parse("example.test").unwrap();
    let snap = kagi_git::Backend::open(&repo)
        .unwrap()
        .snapshot(100)
        .unwrap();
    app.update(cx, |state, cx| {
        state.enter_remote_view(host, "/srv/repo".into(), snap, cx);
    });
    cx.run_until_parked();

    assert!(
        cx.read(|cx| app.read(cx).editor_dirty_guard_modal().is_none()),
        "remote-connect-no-prompt: owner-preserving navigation must not ask to \
         discard the buffer",
    );
    assert!(
        cx.read(|cx| app.read(cx).remote_view.is_some()),
        "remote-connect-no-prompt: the remote view must actually be entered",
    );
    assert_ne!(
        Some(local),
        cx.read(|cx| app.read(cx).active_session()),
        "remote-connect-no-prompt: the remote tab must own the screen now",
    );
    assert_eq!(
        cx.read(|cx| app.read(cx).ui[&local]
            .editor_workspace
            .as_ref()
            .map(|e| e.entity_id())),
        Some(editor.entity_id()),
        "remote-connect-keeps-editor: the departing owner lost its editor",
    );
    assert!(
        cx.read(|cx| editor.read(cx).dirty),
        "remote-connect-keeps-buffer: the unsaved buffer was discarded",
    );
    assert_eq!(
        edited,
        cx.read(|cx| {
            editor
                .read(cx)
                .editor
                .as_ref()
                .unwrap()
                .read(cx)
                .value()
                .to_string()
        }),
        "remote-connect-keeps-buffer: the edited text changed",
    );

    drop(editor);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS remote_connect_keeps_dirty_editor");
}

/// A Smart Commit generation the test releases by hand, so a tab can be closed
/// while the "LLM" is still thinking.
#[allow(clippy::type_complexity)]
fn pending_generation(
    cx: &mut VisualTestAppContext,
) -> (
    gpui::Task<Option<(String, bool)>>,
    impl FnOnce(Option<(String, bool)>),
) {
    type Slot = (Option<Option<(String, bool)>>, Option<std::task::Waker>);
    let state: std::sync::Arc<std::sync::Mutex<Slot>> =
        std::sync::Arc::new(std::sync::Mutex::new((None, None)));
    let polled = state.clone();
    let task = cx
        .background_executor
        .spawn(std::future::poll_fn(move |task_cx| {
            let mut slot = polled.lock().expect("generation slot");
            match slot.0.take() {
                Some(reply) => std::task::Poll::Ready(reply),
                None => {
                    slot.1 = Some(task_cx.waker().clone());
                    std::task::Poll::Pending
                }
            }
        }));
    (task, move |reply| {
        let mut slot = state.lock().expect("generation slot");
        slot.0 = Some(reply);
        if let Some(waker) = slot.1.take() {
            waker.wake();
        }
    })
}

/// #722 P1 (codex): **an in-flight task must not keep a closed tab's pane
/// alive.** The Smart Commit completion captured a strong
/// `Entity<CommitPanelView>`, so closing the tab mid-generation left the panel
/// and its `InputState` allocated until a slow (or hung) LLM answered — one
/// leaked pane per closed tab. ADR-0197 決定 2 requires `release_session` to be
/// the moment the resource dies, so the task holds a weak handle and upgrades
/// only after the owner entry proves the tab is still open.
pub fn scenario_smart_generation_close_drops_panel(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "smart-a");
    let repo_b = build_fixture(&root, "smart-b");
    let (app, window) = mount(cx, &repo_a);
    let session_a = cx.read(|cx| app.read(cx).active_session().expect("A session"));
    // A second tab so closing A does not fall back to Welcome.
    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
        state.switch_repo(0, cx);
    });
    cx.run_until_parked();

    app.update(cx, |state, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_a.clone(), cx);
        state.smart_commit.llm_enabled = true;
        state.smart_commit.provider =
            kagi::ui::smart_commit::SmartProvider::Cli(kagi_git::message_gen::CliProvider::Codex);
    });
    let (task, finish) = pending_generation(cx);
    kagi::ui::e2e::queue_smart_generation(task);
    cx.update_window(window, |_, window, cx| {
        app.update(cx, |state, cx| state.smart_generate(session_a, window, cx));
    })
    .expect("start A generation");
    cx.run_until_parked();
    assert!(
        cx.read(|cx| app.read(cx).ui().smart_commit_generating),
        "precondition: A's generation must still be in flight",
    );
    let weak_panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .expect("A commit panel")
        .downgrade();

    // Close A while the generation is still pending.
    app.update(cx, |state, cx| {
        let index = state
            .tabs
            .iter()
            .position(|tab| tab.session == session_a)
            .expect("A tab");
        state.close_tab(index, cx);
    });
    cx.run_until_parked();
    assert!(
        weak_panel.upgrade().is_none(),
        "smart-generation-close-drops-panel: the in-flight generation kept the \
         closed tab's CommitPanelView alive",
    );

    // Completing it afterwards is a silent no-op, not a panic or a write.
    finish(Some(("late result".to_string(), true)));
    cx.run_until_parked();
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            !state.ui().smart_commit_generating && state.ui().smart_commit_status.is_none(),
            "smart-generation-close-drops-panel: a departed completion wrote to \
             the surviving tab",
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS smart_generation_close_drops_panel");
}

/// #722 P2: while the activation read is in flight the retained Commit Panel
/// is still on screen showing the lists it had *before* the tab was left, so
/// its writes are refused until the read is accepted. On a large repository or
/// a slow disk that window is long enough to Stage against a list that no
/// longer describes the index.
pub fn scenario_commit_panel_refuses_during_activation(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "reval-a");
    let repo_b = build_fixture(&root, "reval-b");
    std::fs::write(repo_a.join("f.txt"), "reval-a second\nreval-a unstaged\n").unwrap();

    let (app, window) = mount(cx, &repo_a);
    let owner_a = app.update(cx, |state, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_a.clone(), cx);
        state.active_session().expect("A owner")
    });
    cx.run_until_parked();
    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
    });
    cx.run_until_parked();
    assert_eq!(staged_count(&repo_a), 0, "A index clean before");

    // Return to A but do NOT settle the activation read: this is the window
    // the user sees the retained panel in.
    app.update(cx, |state, cx| state.switch_repo(0, cx));
    assert!(
        cx.read(|cx| app.read(cx).ui().panes_revalidating),
        "precondition: A's panes must be awaiting their activation read",
    );
    app.update(cx, |state, cx| state.do_stage_file(owner_a, 0, cx));
    // The row menu's single-file Discard plans against the same stale list.
    app.update(cx, |state, cx| {
        state.open_discard_modal_for_path(
            owner_a,
            PathBuf::from("f.txt"),
            kagi::ui::worktree_wip::WriteOrigin::CommitPanel,
            cx,
        );
    });
    assert!(
        !cx.read(|cx| kagi::ui::e2e::active_modal_present(app.read(cx))),
        "panel-refused-while-revalidating: the row menu planned a Discard \
         against the pre-switch list",
    );
    cx.run_until_parked();
    assert_eq!(
        staged_count(&repo_a),
        0,
        "panel-refused-while-revalidating: a Stage ran against the pre-switch list",
    );

    // The read has now been accepted, so the same click works.
    assert!(
        !cx.read(|cx| app.read(cx).ui().panes_revalidating),
        "the accepted read must re-enable the panel",
    );
    app.update(cx, |state, cx| state.do_stage_file(owner_a, 0, cx));
    cx.run_until_parked();
    assert_eq!(
        staged_count(&repo_a),
        1,
        "panel-reenabled-after-read: Stage stayed refused after revalidation",
    );

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_panel_refuses_during_activation");
}

/// #722 P1 (codex re-review): the same lifetime bug as the Smart Commit one,
/// reached through a **helper**. `run_commit` handed a strong
/// `Entity<CommitPanelView>` to the `on_done` callback it passes to
/// `finish_run`, and `finish_run` does the `cx.spawn` internally — so the
/// handle lived for the whole background write. Closing the tab during a
/// large Commit left the panel and its `InputState` allocated until the
/// commit finished. `CommitPanelFailure::expected` is a `WeakEntity` now, so
/// the strong path no longer type-checks.
pub fn scenario_commit_close_drops_panel(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_fixture(&root, "commit-a");
    let repo_b = build_fixture(&root, "commit-b");
    std::fs::write(repo_a.join("f.txt"), "commit-a third\n").unwrap();
    git(&repo_a, &["add", "."]);

    let (app, window) = mount(cx, &repo_a);
    let session_a = cx.read(|cx| app.read(cx).active_session().expect("A session"));
    app.update(cx, |state, cx| {
        assert!(state.open_repository(repo_b.clone(), cx), "open B");
        state.switch_repo(0, cx);
    });
    cx.run_until_parked();
    app.update(cx, |state, cx| {
        kagi::ui::e2e::open_local_panel_no_inputs(state, repo_a.clone(), cx);
    });
    cx.run_until_parked();
    let weak_panel = cx
        .read(|cx| app.read(cx).ui().commit_panel.clone())
        .expect("A commit panel")
        .downgrade();
    app.update(cx, |state, cx| {
        if let Some(panel) = state.ui().commit_panel.clone() {
            panel.update(cx, |v, _| v.state.commit_msg = "from A".to_string());
        }
        state.start_commit(cx);
    });

    // The write is still queued on the background executor. Close A now.
    app.update(cx, |state, cx| {
        let index = state
            .tabs
            .iter()
            .position(|tab| tab.session == session_a)
            .expect("A tab");
        state.close_tab(index, cx);
    });
    assert!(
        weak_panel.upgrade().is_none(),
        "commit-close-drops-panel: the in-flight Commit kept the closed tab's \
         CommitPanelView alive",
    );

    // Letting it finish must not panic, and must not touch B's panel.
    cx.run_until_parked();
    assert!(
        weak_panel.upgrade().is_none(),
        "commit-close-drops-panel: the completion resurrected the closed panel",
    );
    cx.read(|cx| {
        let state = app.read(cx);
        assert!(
            state
                .ui()
                .commit_panel
                .as_ref()
                .is_none_or(|panel| panel.read(cx).repo_path == repo_b),
            "commit-close-drops-panel: A's completion reached B's panel",
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS commit_close_drops_panel");
}
