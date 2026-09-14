//! #643 Wave 4 S1 (ADR-0197 決定 4): the first row of the leak matrix.
//!
//! Two real fixtures opened as two tabs, on the real root. What it observes is
//! **ownership of presentation intent**, not pixels:
//!
//! - a selection made in A is invisible in B, and vice versa;
//! - A→B→A restores A's selection instead of clearing it (`reset_per_repo_ui`
//!   no longer has a selection to forget);
//! - opening A again through a second locator for the same worktree (`<root>`
//!   vs `<root>/.git`) resolves to the live session and leaves its state alone;
//! - closing A removes its key from **both** stores, and reopening the same
//!   path is a fresh incarnation that starts unselected.
//!
//! `dom(ui) = attached sessions` is re-checked after every step that can add or
//! remove a key, so a future slice that adds a field to `TabUiState` inherits
//! the lifecycle assertion for free.
//!
//! Later slices add their own rows here (scroll, caches, pane entities).
use crate::macos::{build_fixture, git, mount, unmount};
use gpui::{ScrollStrategy, VisualTestAppContext};
use kagi::ui::KagiApp;
use std::path::{Path, PathBuf};

/// A three-commit `main`, which is all this scenario needs: it asserts on row
/// indices, not on graph shape.
fn build_linear_fixture(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for i in 0..3 {
        std::fs::write(repo.join("f.txt"), format!("{name} {i}\n")).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", &format!("{name} {i}")]);
    }
    if name == "alpha" {
        // Keep enough concurrent lanes to make horizontal graph scroll valid;
        // otherwise render correctly clamps any synthetic offset back to zero.
        for i in 0..40 {
            let branch = format!("side-{i}");
            git(&repo, &["checkout", "-q", "-b", &branch, "HEAD~2"]);
            std::fs::write(repo.join(format!("side-{i}.txt")), format!("{i}\n")).unwrap();
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-q", "-m", &branch]);
            git(&repo, &["checkout", "-q", "main"]);
        }
    }
    repo.canonicalize().unwrap()
}

/// `dom(ui) = attached sessions` (ADR-0197 決定 2): every open tab has a UI
/// state, and no UI state outlives the session that owned it.
fn assert_ui_domain(app: &KagiApp, where_: &str) {
    for tab in &app.tabs {
        assert!(
            app.ui.contains_key(&tab.session),
            "{where_}: tab {} is attached with no ui entry",
            tab.name,
        );
    }
    for session in app.ui.keys() {
        assert!(
            app.app_sessions.is_attached(*session),
            "{where_}: ui holds a session that is no longer attached",
        );
        assert!(
            app.tabs.iter().any(|tab| tab.session == *session),
            "{where_}: ui holds a session no tab names",
        );
    }
}

/// The commit `session` currently has selected, resolved through **its own**
/// read — the point of the assertion is that the row index alone means nothing
/// once the graph has been rebuilt.
fn selected_commit(app: &KagiApp, session: kagi::app::SessionId) -> Option<kagi_git::CommitId> {
    let row = app.ui[&session].selected?;
    Some(app.reads.get(Some(session)).rows[row].id.clone())
}

/// ADR-0197 決定 3: a retained selection is not authoritative against a read it
/// has not been revalidated by. A background tab's revalidate renumbers its rows
/// just as an active tab's does, so the selection has to be carried across the
/// accept by `CommitId` — for the **owner** of the read, not for the tab on
/// screen.
///
/// Both halves start a reload owned by B and then switch to A before it lands,
/// which is the real sequence: `reload_async` freezes its owner at spawn time.
pub fn scenario_tab_ui_state_background_reload(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_linear_fixture(&root, "alpha");
    let repo_b = build_linear_fixture(&root, "beta");

    let (kagi, window) = mount(cx, &repo_a);
    let session_b = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.tabs[1].session
    });
    cx.run_until_parked();

    // ── a reload that lands on a background owner re-anchors its selection ──
    let pinned = kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_b));
        app.select(1);
        selected_commit(app, session_b).expect("B has a selection")
    });

    // Renumber B's graph: a new commit takes row 0 and pushes everything down.
    std::fs::write(repo_b.join("f.txt"), "beta 3\n").unwrap();
    git(&repo_b, &["commit", "-q", "-am", "beta 3"]);

    kagi.update(cx, |app, cx| {
        app.reload(cx); // owned by B
        app.switch_repo(0, cx); // → A, before that read lands
    });
    cx.run_until_parked();

    kagi.update(cx, |app, _| {
        assert_eq!(app.reads.get(Some(session_b)).rows.len(), 4, "B reloaded");
        assert_eq!(
            app.ui[&session_b].selected,
            Some(2),
            "the background reload left B's selection on its pre-rebuild row",
        );
        assert_eq!(
            selected_commit(app, session_b).as_ref(),
            Some(&pinned),
            "B's selection now names a different commit than before the reload",
        );
    });

    // And the same thing is what the user sees on coming back.
    kagi.update(cx, |app, cx| app.switch_repo(1, cx));
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        let row = app.ui().selected.expect("B is still selecting something");
        assert_eq!(
            app.view().rows[row].id,
            pinned,
            "returning to B showed a different commit selected",
        );
    });

    // ── the selected commit leaving the graph clears the selection ─────────
    let dropped = kagi.update(cx, |app, _| {
        app.select(3); // the oldest commit, which a one-row page cannot hold
        selected_commit(app, session_b).expect("B has a selection")
    });
    assert_ne!(dropped, pinned);
    kagi.update(cx, |app, cx| {
        app.ui_mut().commit_limit = 1;
        app.reload(cx); // owned by B
        app.switch_repo(0, cx); // → A, before that read lands
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.reads.get(Some(session_b)).rows.len(), 1, "B was paged");
        assert_eq!(
            app.ui[&session_b].selected, None,
            "B kept a selection for a commit its read no longer has",
        );
    });

    unmount(cx, kagi, window);
}

pub fn scenario_tab_ui_state_ownership(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_linear_fixture(&root, "alpha");
    let repo_b = build_linear_fixture(&root, "beta");
    // `build_fixture` is what every other scenario mounts against; keeping the
    // helper referenced here documents that this one deliberately does not.
    let _ = build_fixture;

    let (kagi, window) = mount(cx, &repo_a);

    let session_a = kagi.read_with(cx, |app, _| app.active_session().expect("A session"));
    let a_limit = kagi.update(cx, |app, cx| {
        app.load_more_commits(cx);
        let ui = app.ui_mut();
        ui.graph_scroll_x = 42.0;
        ui.branch_groups_collapsed.insert("local:feature".into());
        ui.commit_limit
    });

    let session_b = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        app.tabs[1].session
    });
    // Seed the retained owner's list positions while it is off-screen. This
    // isolates session ownership from each list's viewport clamping behavior.
    let a_cleanup_name = "feature/old".to_owned();
    let b_initial_limit = kagi.read_with(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_b));
        assert_eq!(
            app.ui().graph_scroll_x,
            0.0,
            "A graph position leaked into B"
        );
        assert_eq!(
            app.ui().commit_scroll_handle.logical_scroll_top_index(),
            0,
            "A commit scroll leaked into B",
        );
        assert_eq!(
            app.ui().cleanup_scroll.logical_scroll_top_index(),
            0,
            "A cleanup scroll leaked into B",
        );
        assert!(
            !app.ui().branch_groups_collapsed.contains("local:feature"),
            "A branch fold leaked into B",
        );
        assert!(
            app.ui().cleanup_selected.is_empty(),
            "A cleanup selection leaked into B",
        );
        app.ui().commit_limit
    });
    assert_ne!(
        a_limit, b_initial_limit,
        "B inherited A's paged commit limit"
    );
    assert_eq!(
        kagi::ui::e2e::tab_load_commit_limit(session_b),
        Some(b_initial_limit),
        "tab-load-commit-limit-owner: B's first read did not use B's default limit",
    );
    assert_ne!(session_a, session_b, "each tab is its own owner");
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        let ui = app.ui.get_mut(&session_a).expect("A ui");
        ui.commit_scroll_handle
            .scroll_to_item(2, ScrollStrategy::Center);
        ui.cleanup_scroll.scroll_to_item(1, ScrollStrategy::Center);
        ui.cleanup_selected.insert(a_cleanup_name.clone());
    });

    // ── the selection belongs to the tab that made it ─────────────────────
    kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_b));
        assert_ui_domain(app, "both tabs open");
        assert!(app.view().rows.len() >= 3, "B loaded");
        assert_eq!(app.ui().selected, None, "a fresh tab starts unselected");
        app.select(1);
        assert_eq!(app.ui().selected, Some(1), "B recorded its own selection");
    });

    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx); // → A
        assert_eq!(app.active_session(), Some(session_a));
        assert!(
            app.ui().cleanup_selected.contains(&a_cleanup_name),
            "A cleanup selection was lost on the first restore",
        );
        assert_eq!(
            app.ui().selected,
            None,
            "B's selection leaked into A across the switch",
        );
        app.select(2);
        assert_eq!(app.ui().selected, Some(2));
        assert!(
            app.ui().cleanup_selected.contains(&a_cleanup_name),
            "selecting a commit cleared A cleanup selection",
        );

        app.switch_repo(1, cx); // → B
        assert_eq!(
            app.ui().selected,
            Some(1),
            "A's selection leaked into B across the switch",
        );

        app.switch_repo(0, cx); // → A again
        assert_eq!(
            app.ui().selected,
            Some(2),
            "A→B→A did not restore A's selection",
        );
        assert_eq!(
            app.ui().commit_limit,
            a_limit,
            "A's commit limit was not restored"
        );
        assert_eq!(
            app.ui().graph_scroll_x,
            42.0,
            "A's graph position was not restored"
        );
        assert_eq!(
            app.ui().commit_scroll_handle.logical_scroll_top_index(),
            2,
            "A's commit scroll was not restored",
        );
        assert_eq!(
            app.ui().cleanup_scroll.logical_scroll_top_index(),
            1,
            "A's cleanup scroll was not restored",
        );
        assert!(
            app.ui().branch_groups_collapsed.contains("local:feature"),
            "A's branch fold was not restored",
        );
        assert!(
            app.ui().cleanup_selected.contains(&a_cleanup_name),
            "A's cleanup selection was not restored",
        );
    });

    // The switches armed background revalidates. A landing read publishes a
    // whole new `TabViewState`, and must not drag the intent along with it.
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(
            app.ui().selected,
            Some(2),
            "the revalidate that followed the switch cleared A's selection",
        );
        assert_ui_domain(app, "after the round trip");
    });

    // ── a second locator for one worktree is the same session ─────────────
    //
    // `Sessions::attach` unifies by the resolved `WorktreeId`, so this comes
    // back as the session tab 0 already holds. Initializing its UI state here
    // would clear a live tab's selection from under the user.
    kagi.update(cx, |app, cx| {
        assert!(
            app.open_repository(repo_a.join(".git"), cx),
            "open A through its .git locator",
        );
        assert_eq!(app.tabs.len(), 2, "the alias opened a second tab for A");
        assert_eq!(app.active_session(), Some(session_a));
        assert_eq!(
            app.ui().selected,
            Some(2),
            "an alias open re-initialized the live session's ui state",
        );
        assert_ui_domain(app, "after the alias open");
    });
    cx.run_until_parked();

    // ── close A: both stores forget it ────────────────────────────────────
    kagi.update(cx, |app, cx| {
        let index = app
            .tabs
            .iter()
            .position(|tab| tab.session == session_a)
            .expect("A is still open");
        app.close_tab(index, cx);
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert!(
            !app.ui.contains_key(&session_a),
            "the closed session's ui state outlived its tab",
        );
        assert!(
            !app.app_sessions.is_attached(session_a),
            "the closed session is still attached",
        );
        assert_eq!(
            app.reads.revision(session_a),
            0,
            "the closed session's read model outlived its tab",
        );
        assert_ui_domain(app, "after closing A");
    });

    // ── reopening the same path is a fresh incarnation ────────────────────
    let session_a2 = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_a.clone(), cx), "reopen A");
        let session = app.active_session().expect("A is on screen again");
        assert_ne!(
            session, session_a,
            "reopening reused the closed session's identity",
        );
        assert_eq!(
            app.ui().selected,
            None,
            "the reopened tab inherited the previous incarnation's selection",
        );
        assert_ui_domain(app, "after reopening A");
        session
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_eq!(app.active_session(), Some(session_a2));
        assert_eq!(
            app.ui().selected,
            None,
            "the reopened tab's first read brought a selection with it",
        );
        assert_ui_domain(app, "after the reopened tab's first read");
    });

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS tab_ui_state_ownership");
}

pub fn scenario_tab_ui_state_rejects_detached_writer(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.run_until_parked();
    app.update(cx, |app, cx| {
        assert!(
            kagi::ui::e2e::active_ui_writer_available(app),
            "attached-ui-writer-is-available: active session lost its writer"
        );
        app.close_tab(0, cx);
        assert!(app.tabs.is_empty(), "fixture did not reach Welcome");
        assert!(
            !kagi::ui::e2e::active_ui_writer_available(app),
            "detached-ui-writer-is-rejected: Welcome exposed a resource sink"
        );
        assert!(
            app.ui().selected.is_none(),
            "detached-ui-read-default: Welcome cannot read default state"
        );
    });

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS tab_ui_state_rejects_detached_writer");
}
