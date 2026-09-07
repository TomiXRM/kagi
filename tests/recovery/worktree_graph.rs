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
    // Keep checkout in its confirmation modal so the decorated-name paths can
    // be exercised without moving either worktree's HEAD.
    std::fs::write(fixture.main.join("untracked.txt"), "keep\n").unwrap();
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
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert_eq!(modal.plan.predicted.head, "branch: feature");
    });
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
        assert!(modal.plan.blockers.is_empty(), "{:?}", modal.plan.blockers);
        assert_eq!(modal.plan.predicted.head, "branch: feature");
    });
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
