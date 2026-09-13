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
use gpui::VisualTestAppContext;
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

pub fn scenario_tab_ui_state_ownership(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_linear_fixture(&root, "alpha");
    let repo_b = build_linear_fixture(&root, "beta");
    // `build_fixture` is what every other scenario mounts against; keeping the
    // helper referenced here documents that this one deliberately does not.
    let _ = build_fixture;

    let (kagi, window) = mount(cx, &repo_a);

    let (session_a, session_b) = kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
        (app.tabs[0].session, app.tabs[1].session)
    });
    assert_ne!(session_a, session_b, "each tab is its own owner");
    cx.run_until_parked();

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
        assert_eq!(
            app.ui().selected,
            None,
            "B's selection leaked into A across the switch",
        );
        app.select(2);
        assert_eq!(app.ui().selected, Some(2));

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
}
