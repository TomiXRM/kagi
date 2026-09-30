//! #351 / ADR-0210: "Apply suggestion…" on a review conversation entry.
//!
//! A real PR ref fetch (a bare remote holding `refs/pull/7/head`, reached
//! through `url.<file>.insteadOf` for the PR's GitHub identity) loads the PR,
//! whose branch is checked out, so the working-tree file is the head's blob.
//! The injected conversation carries a line comment with a ```suggestion
//! block. Clicking the entry's measured "Apply suggestion…" opens the plan
//! card without writing; Cancel leaves the file and the oplog alone. The
//! second click and the card's Confirm rewrite exactly the anchored line,
//! stage nothing, and record one `apply-suggestion` oplog entry carrying its
//! backup ref.

use std::path::Path;

use gpui::{AnyWindowHandle, Entity, VisualTestAppContext};
use kagi::ui::{e2e, KagiApp};
use kagi_domain::github::{PullRequest, ReviewComment};
use kagi_git::oplog::{read_oplog_tail_for_repo, OpOutcome};

use crate::evidence_support::pull_request;
use crate::macos::{build_fixture, git, mount, unmount};
use crate::pr_fields_focus::OfflineGh;

const APPLY: &str = "pr-convo-apply-suggestion-7000";

fn rev(repo: &Path, rev: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .expect("git rev-parse");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn draw(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear();
    })
    .expect("draw PR window");
}

/// Draw until the control is laid out (a virtualized list measures on one
/// frame and lays out on the next), then click its centre.
fn click(cx: &mut VisualTestAppContext, window: AnyWindowHandle, id: &str) {
    e2e::clear_control_bounds(window.window_id(), id);
    for _ in 0..3 {
        draw(cx, window);
    }
    let bounds = e2e::control_bounds(window.window_id(), id)
        .unwrap_or_else(|| panic!("{id} was not laid out"));
    cx.simulate_click(window, bounds.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn wait_loaded(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>, head: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let loaded = cx.read(|cx| {
            let app = app.read(cx);
            let tab = app
                .pr_mode()
                .and_then(|m| m.active.and_then(|i| m.tabs.get(i)));
            app.write_busy_op.is_none()
                && tab.is_some_and(|t| {
                    t.head.0 == head && !t.files.is_empty() && t.conversation_loaded
                })
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

fn wait_idle(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| app.read(cx).write_busy_op.is_none()) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the apply never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn modal_open(cx: &mut VisualTestAppContext, app: &Entity<KagiApp>) -> Option<usize> {
    cx.read(|cx| {
        app.read(cx)
            .apply_suggestion_modal()
            .map(|m| m.plan.blockers.len())
    })
}

fn applies(repo: &Path) -> Vec<kagi_git::oplog::OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|e| e.op == "apply-suggestion")
        .collect()
}

pub fn scenario_pr_suggestion_apply(cx: &mut VisualTestAppContext) {
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
    // The PR branch, checked out: its file is the head's blob.
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("s.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "s.txt"]);
    git(&repo, &["commit", "-q", "-m", "pr head"]);
    let head = rev(&repo, "HEAD");
    git(
        &repo,
        &[
            "push",
            "-q",
            remote.to_str().unwrap(),
            "feature:refs/pull/7/head",
        ],
    );

    let (app, window) = mount(cx, &repo);
    e2e::queue_github_pr_conversation(gpui::Task::ready((
        Ok((Vec::new(), Vec::new())),
        Ok(vec![ReviewComment {
            author: "reviewer".into(),
            path: "s.txt".into(),
            line: 2,
            start_line: None,
            body: "shout it\n\n```suggestion\nTWO\n```\n".into(),
            diff_hunk: "@@ -0,0 +1,3 @@\n+one\n+two\n+three".into(),
            created_at: "2026-10-01T00:00:00Z".into(),
            in_reply_to: None,
        }]),
    )));
    let pr = PullRequest {
        head_sha: head.clone(),
        ..pull_request(7, "suggestion apply", "feature")
    };
    app.update(cx, |app, cx| app.pr_mode_open(&pr, cx));
    wait_loaded(cx, &app, &head);
    app.update(cx, |app, cx| {
        app.pr_mode_show(kagi::ui::pr_mode::PrView::Review, cx)
    });
    let file = || std::fs::read_to_string(repo.join("s.txt")).unwrap();

    // Opening only plans; Cancel writes nothing and records nothing.
    click(cx, window, APPLY);
    assert_eq!(
        modal_open(cx, &app),
        Some(0),
        "the plan card opens with no blocker: the file is the head's blob"
    );
    assert_eq!(file(), "one\ntwo\nthree\n", "opening the card wrote");
    click(cx, window, "plan-cancel");
    assert_eq!(modal_open(cx, &app), None, "Cancel closes the card");
    assert_eq!(file(), "one\ntwo\nthree\n", "Cancel wrote");
    assert!(applies(&repo).is_empty(), "Cancel recorded an apply");

    // Confirm: exactly the anchored line changes, nothing is staged, and one
    // successful entry carries the backup ref.
    click(cx, window, APPLY);
    assert_eq!(modal_open(cx, &app), Some(0));
    click(cx, window, "plan-confirm");
    wait_idle(cx, &app);
    assert_eq!(modal_open(cx, &app), None, "Confirm closes the card");
    assert_eq!(file(), "one\nTWO\nthree\n");
    let staged = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(staged.stdout.is_empty(), "the apply staged something");
    let entries = applies(&repo);
    assert_eq!(entries.len(), 1, "one oplog entry per confirmed apply");
    assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
    assert_eq!(entries[0].backup_refs.len(), 1);
    assert!(entries[0].backup_refs[0].starts_with("refs/kagi/backups/"));

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pr_suggestion_apply");
}
