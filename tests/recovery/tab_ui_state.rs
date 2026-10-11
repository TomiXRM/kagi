//! #643 Wave 4 S1 (ADR-0197 決定 4): the first row of the leak matrix.
//!
//! Two real fixtures opened as two tabs, on the real root. What it observes is
//! **ownership of presentation intent**, not pixels:
//!
//! - a selection made in A is invisible in B, and vice versa;
//! - A→B→A restores A's selection instead of clearing it — and, since S6 deleted
//!   the switch-time reset, A's row menu and Branch Cleanup takeover too,
//!   while B first appears `is_pristine`; A's PR predicate, pane and draft
//!   survive activation and its accepted revalidation (ADR-0200 retention);
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

fn seed_pr_refs(repo: &Path) {
    for branch in ["main", "feat"] {
        git(
            repo,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                "main",
            ],
        );
    }
    git(repo, &["branch", "feat", "main"]);
    prepare_pr_remote(repo);
}

fn prepare_pr_remote(repo: &Path) {
    let remote = repo.with_extension("bare");
    git(
        repo,
        &[
            "clone",
            "-q",
            "--bare",
            repo.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    git(
        &remote,
        &["update-ref", "refs/pull/7/head", "refs/heads/feat"],
    );
    let url = "https://github.test/o/r.git";
    git(repo, &["remote", "add", "origin", url]);
    git(
        repo,
        &[
            "config",
            &format!("url.file://{}.insteadOf", remote.display()),
            url,
        ],
    );
}

fn fixture_pr(repo: &Path) -> kagi_domain::github::PullRequest {
    let mut pr = crate::cleanup_publish_owner::pr(7, "feat");
    pr.state = kagi_domain::github::IssueState::Open;
    pr.head_sha = kagi_git::Backend::open(repo)
        .unwrap()
        .snapshot(10_000)
        .unwrap()
        .branches
        .iter()
        .find(|branch| branch.name == "feat")
        .unwrap()
        .target
        .0
        .clone();
    pr
}

fn assert_retained_pr(app: &KagiApp, where_: &str) {
    let mode = app.pr_mode().expect("owner-retained PR mode");
    assert_eq!(mode.tabs.len(), 1, "{where_}: opened pane membership");
    assert_eq!(mode.active, Some(0), "{where_}: active PR pane");
    assert_eq!(
        mode.tabs[0].pr.key(),
        crate::cleanup_publish_owner::pr(7, "feat").key()
    );
    assert_eq!(
        mode.tabs[0].comment_draft, "A's unsent review 日本語",
        "{where_}: owner draft"
    );
    assert_eq!(
        app.ui().github_pr_filter.common.text,
        "A predicate",
        "{where_}: owner predicate"
    );
}

fn seed_owned_pr(app: &mut KagiApp, cx: &mut gpui::Context<KagiApp>) {
    app.pr_mode_open(&fixture_pr(app.repo_path.as_ref().unwrap()), cx);
    app.ui_mut().unwrap().pr_mode.as_mut().unwrap().tabs[0].comment_draft =
        "A's unsent review 日本語".into();
    app.ui_mut().unwrap().github_pr_filter.common.text = "A predicate".into();
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
        app.ui_mut().expect("active session").commit_limit = 1;
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
    seed_pr_refs(&repo_a);
    // `build_fixture` is what every other scenario mounts against; keeping the
    // helper referenced here documents that this one deliberately does not.
    let _ = build_fixture;

    let (kagi, window) = mount(cx, &repo_a);

    let session_a = kagi.read_with(cx, |app, _| app.active_session().expect("A session"));
    let a_limit = kagi.update(cx, |app, cx| {
        app.load_more_commits(cx);
        seed_owned_pr(app, cx);
        let ui = app.ui_mut().expect("active session");
        ui.graph_scroll_x = 42.0;
        ui.branch_groups_collapsed.insert("local:feature".into());
        // S6: the workspace mode used to be reset on every switch.
        ui.branch_cleanup_open = true;
        ui.pr_menu = Some((
            crate::cleanup_publish_owner::pr(7, "a-head"),
            Default::default(),
        ));
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
            app.ui().is_pristine(),
            Ok(()),
            "tab-ui-pristine: A's UI surface leaked into the freshly opened B",
        );
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
        assert_eq!(
            app.ui().is_pristine(),
            Ok(()),
            "B's accepted read stays pristine"
        );
        assert!(
            app.pr_mode().is_none(),
            "B's accepted read never shows A's PR mode"
        );
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
        assert_retained_pr(app, "immediate first activation");
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
        assert!(app.pr_mode().is_none(), "B never displays A's PR pane");
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
        assert!(
            app.ui().branch_cleanup_open,
            "A's cleanup takeover was not restored"
        );
        assert!(
            app.pr_mode().is_some(),
            "repository activation must retain A's session-owned PR mode",
        );
        assert_eq!(
            app.ui().pr_menu.as_ref().map(|(pr, _)| pr.number),
            Some(7),
            "A's PR row menu was not restored",
        );
        assert_retained_pr(app, "immediate round-trip activation");
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
        assert_retained_pr(app, "accepted activation revalidation");
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
        assert_eq!(
            app.ui().is_pristine(),
            Ok(()),
            "the reopened tab inherited the previous incarnation's UI surface",
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

/// ADR-0200 retention: activation's newer GitHub L1 request supersedes the
/// departed request without disposing of the owner's PR pane or draft.
/// Its accepted list answer triggers the retained pane's local PR-ref fetch.
pub fn scenario_pr_mode_retained_on_activation(cx: &mut VisualTestAppContext) {
    let root_dir = tempfile::tempdir().expect("tempdir");
    let root = root_dir.path().canonicalize().unwrap();
    let repo_a = build_linear_fixture(&root, "gamma");
    let repo_b = build_linear_fixture(&root, "beta");
    // The retained pane loads through a real synthetic PR ref fetch against
    // a local bare remote; only the GitHub list response is injected.
    git(&repo_a, &["checkout", "-q", "-b", "feat", "main"]);
    std::fs::write(repo_a.join("feat.txt"), "feat one\n").unwrap();
    git(&repo_a, &["add", "."]);
    git(&repo_a, &["commit", "-q", "-m", "feat one"]);
    git(&repo_a, &["checkout", "-q", "main"]);
    for branch in ["feat", "main"] {
        git(
            &repo_a,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                &format!("refs/heads/{branch}"),
            ],
        );
    }
    prepare_pr_remote(&repo_a);

    let (kagi, window) = mount(cx, &repo_a);
    cx.run_until_parked();
    kagi.update(cx, |app, cx| seed_owned_pr(app, cx));
    cx.run_until_parked();
    let old_head = kagi.read_with(cx, |app, _| app.pr_mode().unwrap().tabs[0].head.clone());
    assert!(
        !old_head.0.is_empty(),
        "initial pane has real fetched head evidence"
    );
    let pane = kagi.read_with(cx, |app, _| {
        let tab = &app.pr_mode().unwrap().tabs[0];
        assert_eq!(tab.selected_file, Some(0), "initial PR file pane is opened");
        tab.feed_entries.clone()
    });
    let (task, departed) = crate::evidence_support::deferred(cx);
    kagi::ui::e2e::queue_github_pr_fetch(task);
    kagi.update(cx, |app, cx| {
        app.refresh_github_prs(cx);
        assert!(
            app.ui().github_prs_loading,
            "old visit owns an outstanding request"
        );
    });

    kagi.update(cx, |app, cx| {
        assert!(app.open_repository(repo_b.clone(), cx), "open B");
    });
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert!(
            app.pr_mode().is_none(),
            "pr-mode-is-session-owned: A's PR mode was visible in B",
        );
    });

    git(&repo_a, &["checkout", "-q", "feat"]);
    std::fs::write(repo_a.join("feat.txt"), "feat two\n").unwrap();
    git(&repo_a, &["commit", "-q", "-am", "feat two"]);
    git(&repo_a, &["update-ref", "refs/remotes/origin/feat", "feat"]);
    git(&repo_a, &["checkout", "-q", "main"]);
    git(&repo_a, &["push", "-q", "origin", "+feat:refs/pull/7/head"]);
    let refreshed = fixture_pr(&repo_a);
    let (activation_task, activation) = crate::evidence_support::deferred(cx);
    kagi::ui::e2e::queue_github_pr_fetch(activation_task);
    let fresh_head = kagi_git::Backend::open(&repo_a)
        .unwrap()
        .snapshot(10_000)
        .unwrap()
        .branches
        .iter()
        .find(|branch| branch.name == "feat")
        .unwrap()
        .target
        .clone();

    let retained_rows = kagi.read_with(cx, |app, _| {
        app.ui[&app.tabs[0].session]
            .github_prs
            .iter()
            .map(|pr| pr.key())
            .collect::<Vec<_>>()
    });
    kagi.update(cx, |app, cx| {
        app.switch_repo(0, cx);
        assert_retained_pr(app, "immediate activation");
        assert!(
            std::rc::Rc::ptr_eq(&app.pr_mode().unwrap().tabs[0].feed_entries, &pane),
            "immediate activation retains the same conversation pane owner"
        );
        assert_eq!(app.pr_mode().unwrap().tabs[0].selected_file, Some(0));
    });
    let mut stale = crate::cleanup_publish_owner::pr(999, "feat");
    stale.state = kagi_domain::github::IssueState::Open;
    stale.title = "superseded departed request must not publish".into();
    departed.send(Ok(kagi_git::github::PrListSnapshot {
        base_repo: stale.base_repo.clone(),
        prs: vec![stale],
        next_cursor: None,
    }));
    cx.run_until_parked();
    kagi.read_with(cx, |app, _| {
        assert!(
            app.ui().github_prs_loading,
            "superseded completion must not clear the outstanding activation request's latch"
        );
        assert_eq!(
            app.ui()
                .github_prs
                .iter()
                .map(|pr| pr.key())
                .collect::<Vec<_>>(),
            retained_rows,
            "superseded departed request must not replace retained rows"
        );
        assert_retained_pr(app, "rejected superseded publication");
        assert!(
            std::rc::Rc::ptr_eq(&app.pr_mode().unwrap().tabs[0].feed_entries, &pane),
            "superseded request cannot replace the retained conversation pane owner"
        );
    });
    activation.send(Ok(kagi_git::github::PrListSnapshot {
        base_repo: refreshed.base_repo.clone(),
        prs: vec![refreshed],
        next_cursor: None,
    }));
    cx.run_until_parked();
    kagi.update(cx, |app, _| {
        assert_retained_pr(app, "accepted activation L1 refresh");
        assert!(
            std::rc::Rc::ptr_eq(&app.pr_mode().unwrap().tabs[0].feed_entries, &pane),
            "head revalidation retains the same conversation pane owner"
        );
        assert_eq!(app.pr_mode().unwrap().tabs[0].selected_file, Some(0));
        let head = &app.pr_mode().unwrap().tabs[0].head;
        assert_ne!(
            head, &old_head,
            "activation's accepted GitHub L1 refresh must fetch changed local head evidence"
        );
        assert_eq!(
            head, &fresh_head,
            "pane head matches the PR-ref fetch triggered by the activation L1 answer"
        );
        assert_eq!(app.pr_mode().unwrap().tabs[0].pr.head_sha, fresh_head.0);
    });
    let accepted = kagi.read_with(cx, |app, _| app.ui().github_prs.clone());
    assert_eq!(
        accepted.len(),
        1,
        "activation accepted the refreshed Open row"
    );
    assert_eq!(accepted[0].head_sha, fresh_head.0);

    unmount(cx, kagi, window);
    eprintln!("[gui-e2e] PASS pr_mode_retained_on_activation");
}

/// A missing local PR ref is a loading state, not a navigation blocker. The
/// page must exist before the background fetch can finish (or fail).
pub fn scenario_pr_open_enters_before_ref_fetch(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let (app, window) = mount(cx, fixture.path());
    cx.run_until_parked();

    app.update(cx, |app, cx| {
        app.pr_mode_open(&crate::cleanup_publish_owner::pr(71, "never-fetched"), cx);
        let mode = app.pr_mode().expect("PR mode is open");
        assert_eq!(mode.tabs.len(), 1, "the click must create the PR page");
        assert_eq!(mode.active, Some(0), "the new PR page must be visible");
        assert!(
            mode.tabs[0].local_refs_loading,
            "the page must expose its local-ref loading state"
        );
        app.pr_mode_show(kagi::ui::pr_mode::PrView::Conflicts, cx);
        assert!(
            app.pr_mode().unwrap().tabs[0].conflicts.is_none(),
            "conflict loading must wait for non-empty local refs"
        );
    });

    cx.run_until_parked();
    app.update(cx, |app, _| {
        let tab = &app.pr_mode().unwrap().tabs[0];
        assert!(
            !tab.local_refs_loading,
            "a failed fetch must settle instead of spinning forever"
        );
        assert!(
            tab.head.0.is_empty(),
            "failure must not invent a local head"
        );
    });
    // #885: the failed PR-ref fetch is observed and moved no local ref, so
    // its receipt says "nothing moved" (Unknown is never that).
    let receipt = kagi_git::oplog::read_oplog_tail_for_repo(fixture.path(), 100)
        .into_iter()
        .rev()
        .find(|entry| entry.op == "fetch-pr")
        .expect("the failed PR fetch is recorded");
    let expected = match receipt.outcome {
        kagi_git::oplog::OpOutcome::Unknown { .. } => None,
        _ => Some(Vec::new()),
    };
    assert_eq!(receipt.ref_moves, expected, "fetch-pr receipt");

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_open_enters_before_ref_fetch");
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
