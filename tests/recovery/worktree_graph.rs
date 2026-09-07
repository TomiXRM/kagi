//! Issue #591 graph-to-worktree navigation regression.
//! Included by gui_e2e_runner; never executed without the explicit scenario
//! filter because native GUI E2E must keep its window budget bounded.

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::VisualTestAppContext;
use kagi::ui::e2e;

use crate::macos::{git, mount, unmount};

fn rev_parse(dir: &Path, rev: &str) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", rev])
        .output()
        .expect("git rev-parse");
    assert!(output.status.success(), "git rev-parse {rev} failed");
    String::from_utf8(output.stdout)
        .expect("utf8 sha")
        .trim()
        .to_string()
}

struct Fixture {
    _root: tempfile::TempDir,
    main: PathBuf,
    linked: PathBuf,
    detached: Vec<PathBuf>,
}

/// One attached linked worktree and three clean detached worktrees at the same
/// root commit, which is unreachable from every ref and the open HEAD.
fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("tempdir");
    let repo = root.path().join("repo");
    let linked = root.path().join("linked");
    let detached_a = root.path().join("detached-a");
    let detached_b = root.path().join("detached-b");
    let detached_c = root.path().join("detached-c");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("f.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    let base = rev_parse(&repo, "HEAD");
    git(&repo, &["branch", "feature", &base]);
    std::fs::write(repo.join("f.txt"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main advances"]);
    git(
        &repo,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "feature"],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            detached_a.to_str().unwrap(),
            &base,
        ],
    );
    git(
        &detached_a,
        &["checkout", "-q", "--orphan", "detached-only"],
    );
    std::fs::write(detached_a.join("f.txt"), "unreachable\n").unwrap();
    git(&detached_a, &["add", "."]);
    git(
        &detached_a,
        &["commit", "-q", "-m", "unreachable detached root"],
    );
    let detached_head = rev_parse(&detached_a, "HEAD");
    git(&detached_a, &["checkout", "-q", "--detach", "HEAD"]);
    git(&repo, &["branch", "-D", "detached-only"]);
    for path in [&detached_b, &detached_c] {
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                path.to_str().unwrap(),
                &detached_head,
            ],
        );
    }
    Fixture {
        _root: root,
        main: repo.canonicalize().unwrap(),
        linked: linked.canonicalize().unwrap(),
        detached: [&detached_a, &detached_b, &detached_c]
            .into_iter()
            .map(|path| path.canonicalize().unwrap())
            .collect(),
    }
}

fn tree_bounds(win: gpui::WindowId, path: &Path) -> gpui::Bounds<gpui::Pixels> {
    let control = format!("graph-worktree-open:{}", path.display());
    e2e::control_bounds(win, &control)
        .unwrap_or_else(|| panic!("worktree tree bounds for {}", path.display()))
}

/// Tree glyphs are navigation controls distinct from the branch-name hitbox;
/// opening an existing path switches tabs, and clean detached worktrees remain
/// reachable from their HEAD. Run only with the matching E2E filter.
pub fn scenario_graph_worktree_open(cx: &mut VisualTestAppContext) {
    let fixture = fixture();
    let main_index = std::fs::read(fixture.main.join(".git/index")).unwrap();
    let linked_index_path = git2::Repository::open(&fixture.linked)
        .unwrap()
        .path()
        .join("index");
    let linked_index = std::fs::read(&linked_index_path).unwrap();
    let (kagi, win) = mount(cx, &fixture.main);

    let feature_row = cx.read(|app| {
        kagi.read(app)
            .view()
            .rows
            .iter()
            .position(|row| {
                row.badges
                    .iter()
                    .any(|badge| badge.label.as_ref() == "🌲 feature")
            })
            .expect("feature worktree badge row")
    });
    let tree = tree_bounds(win.window_id(), &fixture.linked);
    let name = e2e::control_bounds(win.window_id(), "graph-worktree-branch-name")
        .expect("attached worktree branch-name bounds");
    let detached_trees: Vec<_> = fixture
        .detached
        .iter()
        .map(|path| tree_bounds(win.window_id(), path))
        .collect();
    for pair in detached_trees.windows(2) {
        assert!(
            pair[0].origin.x + pair[0].size.width <= pair[1].origin.x,
            "detached worktree controls overlap: {pair:?}"
        );
    }
    assert!(
        tree.origin.x + tree.size.width <= name.origin.x
            || name.origin.x + name.size.width <= tree.origin.x,
        "tree and branch-name hitboxes overlap: tree={tree:?} name={name:?}"
    );

    cx.simulate_mouse_move(win, name.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, name.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.selected, Some(feature_row));
        assert_eq!(app.tabs.len(), 1, "branch-name click opened a worktree");
        assert_eq!(app.tabs[app.active_tab].path, fixture.main);
    });

    cx.simulate_event(
        win,
        gpui::MouseDownEvent {
            position: name.center(),
            button: gpui::MouseButton::Left,
            modifiers: gpui::Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        },
    );
    cx.simulate_event(
        win,
        gpui::MouseUpEvent {
            position: name.center(),
            button: gpui::MouseButton::Left,
            modifiers: gpui::Modifiers::none(),
            click_count: 2,
        },
    );
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        let modal = app.plan_modal().expect("double-click checkout plan");
        assert!(
            modal.plan.blockers.iter().any(|note| matches!(
                note,
                kagi_domain::plan_note::PlanNote::Worktree(
                    kagi_domain::plan_note::WorktreeNote::BranchInOtherWorktree { branch, .. }
                ) if branch == "feature"
            )),
            "{:?}",
            modal.plan.blockers
        );
        assert_eq!(modal.plan.predicted.head, "branch: feature");
    });
    crate::recovery_operations::press_key(cx, &kagi, win, "enter");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(fixture.main.join("f.txt")).unwrap(),
        b"main\n"
    );
    assert_eq!(
        std::fs::read(fixture.linked.join("f.txt")).unwrap(),
        b"base\n"
    );
    assert_eq!(
        std::fs::read(fixture.main.join(".git/index")).unwrap(),
        main_index
    );
    assert_eq!(std::fs::read(&linked_index_path).unwrap(), linked_index);
    crate::recovery_operations::press_key(cx, &kagi, win, "escape");
    // The row click toggles selection; select it again after the double-click.
    cx.simulate_mouse_move(win, name.center(), None, gpui::Modifiers::none());
    cx.simulate_click(win, name.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| assert_eq!(kagi.read(app).selected, Some(feature_row)));
    crate::recovery_operations::dispatch_checkout_selected(cx, &kagi, win);
    cx.read(|app| {
        let app = kagi.read(app);
        let modal = app
            .plan_modal()
            .expect("selected worktree branch checkout plan");
        assert!(
            modal.plan.blockers.iter().any(|note| matches!(
                note,
                kagi_domain::plan_note::PlanNote::Worktree(
                    kagi_domain::plan_note::WorktreeNote::BranchInOtherWorktree { branch, .. }
                ) if branch == "feature"
            )),
            "{:?}",
            modal.plan.blockers
        );
        assert_eq!(modal.plan.predicted.head, "branch: feature");
    });
    crate::recovery_operations::press_key(cx, &kagi, win, "enter");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read(fixture.main.join("f.txt")).unwrap(),
        b"main\n"
    );
    assert_eq!(
        std::fs::read(fixture.main.join(".git/index")).unwrap(),
        main_index
    );
    assert_eq!(std::fs::read(&linked_index_path).unwrap(), linked_index);
    crate::recovery_operations::press_key(cx, &kagi, win, "escape");
    assert_eq!(
        rev_parse(&fixture.main, "HEAD"),
        rev_parse(&fixture.main, "main")
    );
    assert_eq!(
        rev_parse(&fixture.linked, "HEAD"),
        rev_parse(&fixture.main, "feature")
    );

    cx.simulate_mouse_move(win, tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[app.active_tab].path, fixture.linked);
    });

    let main_tree = tree_bounds(win.window_id(), &fixture.main);
    cx.simulate_mouse_move(win, main_tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, main_tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 2, "existing main tab was duplicated");
        assert_eq!(app.tabs[app.active_tab].path, fixture.main);
    });

    let detached_tree = tree_bounds(win.window_id(), &fixture.detached[0]);
    cx.simulate_mouse_down(
        win,
        detached_tree.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.read(|app| {
        let menu = kagi
            .read(app)
            .worktree_menu
            .as_ref()
            .expect("detached worktree menu");
        assert_eq!(menu.path.as_deref(), Some(fixture.detached[0].as_path()));
    });
    kagi.update(cx, |app, cx| {
        app.worktree_menu = None;
        cx.notify();
    });
    cx.run_until_parked();

    let detached_tree = tree_bounds(win.window_id(), &fixture.detached[0]);
    cx.simulate_mouse_move(win, detached_tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, detached_tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.tabs[app.active_tab].path, fixture.detached[0]);
    });

    // Open every remaining tree from the same aggregate badge. In particular,
    // the third target used to fall into badge overflow and was inoperable.
    for (index, path) in fixture.detached.iter().enumerate().skip(1) {
        let main_tree = tree_bounds(win.window_id(), &fixture.main);
        cx.simulate_mouse_move(win, main_tree.center(), None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(win, main_tree.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        let detached_tree = tree_bounds(win.window_id(), path);
        cx.simulate_mouse_move(win, detached_tree.center(), None, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.simulate_click(win, detached_tree.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        cx.read(|app| {
            let app = kagi.read(app);
            assert_eq!(app.tabs.len(), index + 3);
            assert_eq!(app.tabs[app.active_tab].path, *path);
        });
    }

    unmount(cx, kagi, win);
    eprintln!("[gui-e2e] PASS graph_worktree_open tabs=5 detached-targets=3");
}

fn remote_merge_fixture() -> (Fixture, String) {
    let fixture = fixture();
    let remote = fixture._root.path().join("origin.git");
    git(
        &fixture.main,
        &[
            "clone",
            "-q",
            "--bare",
            fixture.main.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    let repo = git2::Repository::open_bare(&remote).unwrap();
    let base = repo
        .find_branch("feature", git2::BranchType::Local)
        .unwrap()
        .get()
        .peel_to_commit()
        .unwrap();
    let mut tree = repo.treebuilder(Some(&base.tree().unwrap())).unwrap();
    tree.insert("remote.txt", repo.blob(b"remote\n").unwrap(), 0o100644)
        .unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let source = repo
        .commit(
            Some("refs/heads/source"),
            &sig,
            &sig,
            "remote source",
            &repo.find_tree(tree.write().unwrap()).unwrap(),
            &[&base],
        )
        .unwrap()
        .to_string();
    git(
        &fixture.main,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&fixture.main, &["fetch", "-q", "origin"]);
    std::fs::write(fixture.linked.join("target.txt"), "target\n").unwrap();
    git(&fixture.linked, &["add", "."]);
    git(&fixture.linked, &["commit", "-qm", "target diverges"]);
    std::fs::write(fixture.main.join("f.txt"), "parent edits stay here\n").unwrap();
    (fixture, source)
}

fn drag_remote_to_worktree(cx: &mut VisualTestAppContext, window: gpui::AnyWindowHandle) {
    use crate::recovery_operations::paint;
    use gpui::{Modifiers, MouseButton};
    paint(cx, window);
    let source = e2e::control_bounds(window.window_id(), "graph-remote-origin/source")
        .expect("remote source chip")
        .center();
    let target = e2e::control_bounds(window.window_id(), "graph-worktree-branch-name")
        .expect("other worktree branch chip")
        .center();
    cx.simulate_mouse_move(window, source, None, Modifiers::none());
    cx.simulate_mouse_down(window, source, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        window,
        source + gpui::point(gpui::px(12.), gpui::px(0.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_move(window, target, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    paint(cx, window);
    cx.simulate_mouse_up(window, target, MouseButton::Left, Modifiers::none());
}

fn guard_dirty_editor_merge(
    cx: &mut VisualTestAppContext,
    app: &gpui::Entity<kagi::ui::KagiApp>,
    window: gpui::AnyWindowHandle,
    fixture: &Fixture,
) {
    use crate::recovery_operations::{paint, press_key, wait_idle};
    use gpui::Focusable;
    use std::time::{Duration, Instant};
    app.update(cx, |app, cx| app.open_editor_workspace(cx));
    let editor = cx.read(|cx| app.read(cx).editor_workspace.clone()).unwrap();
    editor.update(cx, |view, cx| view.open_tab("f.txt".into(), cx));
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        paint(cx, window);
        if cx.read(|cx| editor.read(cx).editor.is_some() && editor.read(cx).content.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline, "editor buffer did not load");
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
    app.update(cx, |app, cx| {
        app.start_merge_into_from_drag("origin/source".into(), "feature".into(), cx);
    });
    cx.read(|cx| {
        let app = app.read(cx);
        assert!(app.editor_dirty_guard_modal().is_some());
        assert_eq!(app.tabs[app.active_tab].path, fixture.main);
        assert!(app.merge_modal().is_none());
    });
    press_key(cx, app, window, "escape");
    assert!(cx.read(|cx| editor.read(cx).dirty));
    app.update(cx, |app, cx| {
        app.start_merge_into_from_drag("origin/source".into(), "feature".into(), cx);
    });
    press_key(cx, app, window, "enter");
    wait_idle(cx, app);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs[app.active_tab].path, fixture.linked);
        assert!(app.editor_workspace.is_none());
        assert!(
            app.merge_modal().is_some(),
            "discard must continue to a plan, not execute"
        );
    });
    assert!(!fixture.linked.join("remote.txt").exists());
    press_key(cx, app, window, "escape");
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
}

pub fn scenario_cross_worktree_merge(cx: &mut VisualTestAppContext) {
    use crate::recovery_operations::{press_key, wait_idle};
    let (fixture, source) = remote_merge_fixture();
    let old_target = rev_parse(&fixture.linked, "HEAD");
    let old_main = rev_parse(&fixture.main, "HEAD");
    let parent_index = std::fs::read(fixture.main.join(".git/index")).unwrap();
    let (app, window) = mount(cx, &fixture.main);
    drag_remote_to_worktree(cx, window);
    wait_idle(cx, &app);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[app.active_tab].path, fixture.linked);
        let modal = app.merge_modal().expect("destination merge plan");
        assert!(
            !modal.off_branch,
            "must merge in the destination worktree, not move its ref"
        );
        assert_eq!(modal.target, "origin/source");
        assert_eq!(modal.into_branch, "feature");
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
    });
    assert_eq!(rev_parse(&fixture.linked, "HEAD"), old_target);
    assert!(!fixture.linked.join("remote.txt").exists());
    press_key(cx, &app, window, "escape");
    assert_eq!(rev_parse(&fixture.linked, "HEAD"), old_target);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    // A remote drop target resolves to its local branch before worktree
    // routing, just as the off-branch planner resolves origin/feature to
    // feature. Its linked worktree must therefore use the HEAD-merge path.
    app.update(cx, |app, cx| {
        app.start_merge_into_from_drag("origin/source".into(), "origin/feature".into(), cx);
    });
    wait_idle(cx, &app);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs[app.active_tab].path, fixture.linked);
        let modal = app.merge_modal().expect("remote destination merge plan");
        assert!(!modal.off_branch);
        assert_eq!(modal.into_branch, "feature");
    });
    press_key(cx, &app, window, "escape");
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    // Leave again before the background plan can arrive: neither tab may
    // receive a stale confirmable plan, and the planning latch must settle.
    app.update(cx, |app, cx| {
        app.start_merge_into_from_drag("origin/source".into(), "feature".into(), cx);
        app.switch_repo(0, cx);
    });
    wait_idle(cx, &app);
    cx.read(|cx| assert!(app.read(cx).merge_modal().is_none()));
    assert_eq!(rev_parse(&fixture.linked, "HEAD"), old_target);
    guard_dirty_editor_merge(cx, &app, window, &fixture);
    // The graph can still show feature while an external checkout changes
    // the linked worktree. Never reinterpret the drop as a merge into drifted.
    git(&fixture.linked, &["switch", "-qc", "drifted"]);
    app.update(cx, |app, cx| {
        app.start_merge_into_from_drag("origin/source".into(), "feature".into(), cx);
    });
    wait_idle(cx, &app);
    cx.read(|cx| {
        assert!(app.read(cx).merge_modal().is_none());
        assert!(e2e::app_notice_message(app.read(cx)).is_some());
    });
    press_key(cx, &app, window, "enter");
    assert_eq!(rev_parse(&fixture.linked, "HEAD"), old_target);
    assert!(!fixture.linked.join("remote.txt").exists());
    git(&fixture.linked, &["switch", "-q", "feature"]);
    app.update(cx, |app, cx| app.switch_repo(0, cx));
    cx.run_until_parked();
    drag_remote_to_worktree(cx, window);
    wait_idle(cx, &app);
    cx.read(|cx| {
        let app = app.read(cx);
        assert_eq!(app.tabs.len(), 2, "reuse the existing worktree tab");
        assert!(app.merge_modal().is_some(), "drop must open a fresh plan");
    });
    press_key(cx, &app, window, "enter");
    wait_idle(cx, &app);
    let merged = rev_parse(&fixture.linked, "HEAD");
    assert_eq!(rev_parse(&fixture.linked, "HEAD^1"), old_target);
    assert_eq!(rev_parse(&fixture.linked, "HEAD^2"), source);
    assert_eq!(
        std::fs::read(fixture.linked.join("remote.txt")).unwrap(),
        b"remote\n"
    );
    assert_eq!(
        std::fs::read(fixture.linked.join("target.txt")).unwrap(),
        b"target\n"
    );
    let destination = git2::Repository::open(&fixture.linked).unwrap();
    assert_eq!(
        destination.index().unwrap().write_tree().unwrap(),
        destination
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .tree_id()
    );
    assert_eq!(rev_parse(&fixture.main, "HEAD"), old_main);
    assert_eq!(
        std::fs::read(fixture.main.join(".git/index")).unwrap(),
        parent_index
    );
    assert_eq!(
        std::fs::read(fixture.main.join("f.txt")).unwrap(),
        b"parent edits stay here\n"
    );
    assert!(!fixture.main.join("remote.txt").exists());
    assert_eq!(
        rev_parse(&fixture.main, "refs/remotes/origin/source"),
        source
    );
    assert!(destination
        .find_branch("source", git2::BranchType::Local)
        .is_err());
    let records = kagi_git::oplog::read_oplog_tail_for_repo(&fixture.linked, 100);
    let merges: Vec<_> = records.iter().filter(|entry| entry.op == "merge").collect();
    assert_eq!(merges.len(), 1);
    assert!(matches!(
        merges[0].outcome,
        kagi_git::OpOutcome::Success { .. }
    ));
    assert_eq!(rev_parse(&fixture.main, "feature"), merged);
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS cross_worktree_merge");
}
