//! #351 / ADR-0207: per-file "viewed" marks in a PR tab's file list.
//!
//! A real PR ref fetch (a bare remote holding `refs/pull/7/head`, reached
//! through `url.<file>.insteadOf` for the PR's GitHub identity) loads two
//! files. Clicking each row's measured checkbox marks it viewed without
//! selecting the row, and the marks land in `pr-viewed/example-repo-7.json`.
//! Closing and reopening the tab reads them back from disk. When the listed
//! head moves (a queued list response) and the new head changes one file, that
//! file is unviewed again while the unchanged one stays viewed.

use std::path::{Path, PathBuf};

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::PullRequest;

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

const BASE_REPO: &str = "github.com/example/repo";

pub(crate) fn rev(repo: &Path, rev: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .expect("git rev-parse");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// Commit `files` on top of `parent` and publish it as the PR head.
pub(crate) fn push_pr_head(
    repo: &Path,
    remote: &Path,
    parent: &str,
    files: &[(&str, &str)],
) -> String {
    git(repo, &["checkout", "-q", "-B", "pr-head", parent]);
    for (path, text) in files {
        std::fs::write(repo.join(path), text).unwrap();
    }
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "pr head"]);
    let head = rev(repo, "HEAD");
    let refspec = "+pr-head:refs/pull/7/head";
    git(repo, &["push", "-q", remote.to_str().unwrap(), refspec]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["branch", "-q", "-D", "pr-head"]);
    head
}

pub(crate) fn pr_at(head: &str) -> PullRequest {
    PullRequest {
        head_sha: head.to_string(),
        ..pull_request(7, "viewed files", "feature")
    }
}

pub(crate) fn paint(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw PR window");
}

/// The tab's files are loaded from `head` (a real fetch, so wait for it).
pub(crate) fn wait_loaded(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, head: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let loaded = cx.read(|cx| {
            let app = app.read(cx);
            let tab = app
                .pr_mode()
                .and_then(|m| m.active.and_then(|i| m.tabs.get(i)));
            app.write_busy_op.is_none()
                && tab.is_some_and(|t| t.head.0 == head && !t.files.is_empty())
        });
        if loaded {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PR head {head} never loaded"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// (path, viewed) per row, and the selected row.
fn rows(
    cx: &mut VisualTestAppContext,
    app: &Entity<KagiApp>,
) -> (Vec<(String, bool)>, Option<usize>) {
    cx.read(|cx| {
        let mode = app.read(cx).pr_mode().expect("PR mode");
        let tab = mode
            .active
            .and_then(|i| mode.tabs.get(i))
            .expect("a PR tab");
        let rows = tab
            .files
            .iter()
            .map(|f| (f.path.display().to_string(), tab.viewed.is_viewed(&f.path)))
            .collect();
        (rows, tab.selected_file)
    })
}

fn click_checkbox(cx: &mut VisualTestAppContext, window: AnyWindowHandle, ix: usize) {
    let id = format!("pr-file-viewed-{ix}");
    e2e::clear_control_bounds(window.window_id(), &id);
    paint(cx, window);
    let bounds = e2e::control_bounds(window.window_id(), &id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn stored(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("marks stored")).unwrap()
}

fn open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, pr: &PullRequest) {
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(Vec::new()),
    )));
    app.update(cx, |app, cx| app.pr_mode_open(pr, cx));
    wait_loaded(cx, app, &pr.head_sha);
    // The file list is shown beside a file diff.
    app.update(cx, |app, cx| app.pr_mode_select_file(0, cx));
}

pub fn scenario_pr_viewed(cx: &mut VisualTestAppContext) {
    let _gh = OfflineGh::install();
    let fixture = build_fixture();
    let repo = fixture.path().canonicalize().unwrap();
    let remote_dir = tempfile::tempdir().unwrap();
    let remote = remote_dir.path().join("repo.git");
    git(remote_dir.path(), &["init", "-q", "--bare", "repo.git"]);
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "main:refs/heads/main",
        ],
    );
    let url = "https://github.com/example/repo.git";
    git(&repo, &["remote", "add", "origin", url]);
    let file_url = format!("file://{}", remote.display());
    git(
        &repo,
        &["config", &format!("url.{file_url}.insteadOf"), url],
    );
    let head1 = push_pr_head(
        &repo,
        &remote,
        "main",
        &[("a.txt", "a1\n"), ("b.txt", "b1\n")],
    );

    let marks_file: PathBuf =
        kagi_ui_core::pr_viewed::viewed_path(BASE_REPO, 7).expect("storage path");
    let _ = std::fs::remove_file(&marks_file);

    let (app, window) = mount(cx, &repo);
    open(cx, &app, &pr_at(&head1));
    paint(cx, window);
    let (files, selected) = rows(cx, &app);
    assert_eq!(
        files,
        vec![("a.txt".into(), false), ("b.txt".into(), false)],
        "nothing viewed yet"
    );
    assert_eq!(selected, Some(0));

    // The checkbox marks its row without selecting it.
    click_checkbox(cx, window, 1);
    assert_eq!(
        rows(cx, &app),
        (
            vec![("a.txt".into(), false), ("b.txt".into(), true)],
            Some(0)
        )
    );
    click_checkbox(cx, window, 0);
    assert_eq!(
        rows(cx, &app).0,
        vec![("a.txt".into(), true), ("b.txt".into(), true)]
    );
    let blob = |rev_path: &str| rev(&repo, rev_path);
    assert_eq!(
        stored(&marks_file),
        serde_json::json!({
            "a.txt": blob(&format!("{head1}:a.txt")),
            "b.txt": blob(&format!("{head1}:b.txt")),
        })
    );

    // Reopening reads the marks back from disk.
    app.update(cx, |app, cx| app.pr_mode_close_tab(0, cx));
    cx.run_until_parked();
    open(cx, &app, &pr_at(&head1));
    assert_eq!(
        rows(cx, &app).0,
        vec![("a.txt".into(), true), ("b.txt".into(), true)]
    );

    // The head moves and changes a.txt only: a.txt is unviewed, b.txt stays.
    let head2 = push_pr_head(&repo, &remote, &head1, &[("a.txt", "a2\n")]);
    let moved = pr_at(&head2);
    e2e::queue_github_pr_fetch(gpui::Task::ready(Ok(vec![moved])));
    app.update(cx, |app, cx| app.refresh_github_prs(cx));
    wait_loaded(cx, &app, &head2);
    paint(cx, window);
    assert_eq!(
        rows(cx, &app).0,
        vec![("a.txt".into(), false), ("b.txt".into(), true)],
        "a changed blob unviews the file; an unchanged one stays viewed"
    );

    // gpui-component's Checkbox holds its toggle state in a 250 ms animation
    // timer per render; let those fire so no task outlives the window.
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_viewed: marked by checkbox, kept across reopen, unviewed when the head changes the file");
}
