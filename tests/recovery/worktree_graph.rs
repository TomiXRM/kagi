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

/// A main worktree plus an attached linked worktree and a clean detached
/// linked worktree. The linked/detached pair share the base commit while main
/// advances, so all three graph navigation cases render.
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let root = tempfile::tempdir().expect("tempdir");
    let repo = root.path().join("repo");
    let linked = root.path().join("linked");
    let detached = root.path().join("detached");
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
            detached.to_str().unwrap(),
            &base,
        ],
    );
    (
        root,
        repo.canonicalize().unwrap(),
        linked.canonicalize().unwrap(),
        detached.canonicalize().unwrap(),
    )
}

/// Tree glyphs are navigation controls distinct from the branch-name hitbox;
/// opening an existing path switches tabs, and clean detached worktrees remain
/// reachable from their HEAD. Run only with the matching E2E filter.
pub fn scenario_graph_worktree_open(cx: &mut VisualTestAppContext) {
    let (_fixture, main, linked, detached) = fixture();
    let (kagi, win) = mount(cx, &main);

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
    let tree = e2e::control_bounds(win.window_id(), "graph-worktree-open-attached")
        .expect("attached worktree tree bounds");
    let name = e2e::control_bounds(win.window_id(), "graph-worktree-branch-name")
        .expect("attached worktree branch-name bounds");
    let detached_tree = e2e::control_bounds(win.window_id(), "graph-worktree-open-detached")
        .expect("detached worktree tree bounds");
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
        assert_eq!(app.tabs[app.active_tab].path, main);
    });

    cx.simulate_mouse_move(win, tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[app.active_tab].path, linked);
    });

    let main_tree = e2e::control_bounds(win.window_id(), "graph-worktree-open-attached")
        .expect("main worktree tree bounds from linked tab");
    cx.simulate_mouse_move(win, main_tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, main_tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 2, "existing main tab was duplicated");
        assert_eq!(app.tabs[app.active_tab].path, main);
    });

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
        assert_eq!(menu.path.as_deref(), Some(detached.as_path()));
    });
    kagi.update(cx, |app, cx| {
        app.worktree_menu = None;
        cx.notify();
    });
    cx.run_until_parked();

    let detached_tree = e2e::control_bounds(win.window_id(), "graph-worktree-open-detached")
        .expect("detached worktree tree bounds after menu dismiss");
    cx.simulate_mouse_move(win, detached_tree.center(), None, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_click(win, detached_tree.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.read(|app| {
        let app = kagi.read(app);
        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.tabs[app.active_tab].path, detached);
    });

    unmount(cx, kagi, win);
    eprintln!("[gui-e2e] PASS graph_worktree_open tabs=3");
}
