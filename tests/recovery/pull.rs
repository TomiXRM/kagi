//! Dirty Pull GUI E2E scenarios for ADR-0189.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use gpui::VisualTestAppContext;
use kagi_git::oplog::{read_oplog_tail_for_repo, recovery, OpOutcome};

use crate::macos::{build_fixture, git, mount, unmount};
use crate::recovery_operations::{press_enter, wait_idle};

fn output(repo: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command");
    assert!(result.status.success(), "git {args:?}: {:?}", result.stderr);
    String::from_utf8(result.stdout)
        .expect("utf8 git output")
        .trim()
        .to_string()
}

fn records(repo: &Path, op: &str) -> Vec<kagi_git::oplog::OpLogEntry> {
    read_oplog_tail_for_repo(repo, 100)
        .into_iter()
        .filter(|entry| entry.op == op)
        .collect()
}

fn assert_stash_push_recovery(repo: &Path) {
    let pushes = records(repo, "stash-push");
    assert_eq!(pushes.len(), 1, "auto-stash must be recorded exactly once");
    assert!(
        pushes[0]
            .recovery
            .iter()
            .any(|handle| handle.kind == recovery::STASH && handle.oid.len() == 40),
        "the created stash OID must be a typed recovery handle"
    );
}

pub fn scenario_pull_auto_stash_success(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let other = remote_root.path().join("other");
    let bare_path = bare.to_str().unwrap();
    let other_path = other.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    git(remote_root.path(), &["clone", "-q", bare_path, other_path]);
    std::fs::write(other.join("upstream.txt"), "upstream\n").unwrap();
    git(&other, &["add", "upstream.txt"]);
    git(&other, &["commit", "-q", "-m", "upstream"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let upstream_head = output(&other, &["rev-parse", "HEAD"]);

    std::fs::write(repo.join("README.md"), "local staged change\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "local untracked change\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "fixture Pull must be executable"
        );
        assert!(
            modal.error.is_none(),
            "confirmation must not begin as an error"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.run_until_parked();

    assert_eq!(output(repo, &["rev-parse", "HEAD"]), upstream_head);
    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "local staged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "local untracked change\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Success { .. })),
        "Pull success must be durable"
    );
    cx.read(|cx| assert!(app.read(cx).pull_modal().is_none()));

    unmount(cx, app, window);
    eprintln!("[gui-e2e] PASS pull_auto_stash_success: dirty changes restored after Pull");
}

pub fn scenario_pull_auto_stash_failure_restores(cx: &mut VisualTestAppContext) {
    let fixture = build_fixture();
    let repo = fixture.path();
    let remote_root = tempfile::tempdir().expect("remote root");
    let bare = remote_root.path().join("origin.git");
    let bare_path = bare.to_str().unwrap();

    git(repo, &["init", "--bare", "-q", bare_path]);
    git(repo, &["remote", "add", "origin", bare_path]);
    git(repo, &["push", "-q", "-u", "origin", "main"]);
    std::fs::remove_dir_all(&bare).unwrap();
    std::fs::write(repo.join("README.md"), "restore after failed Pull\n").unwrap();
    std::fs::write(repo.join("scratch.txt"), "restore untracked\n").unwrap();
    git(repo, &["add", "README.md"]);

    let (app, window) = mount(cx, repo);
    app.update(cx, |app, cx| app.open_pull_modal(cx));
    cx.run_until_parked();
    cx.read(|cx| {
        let modal = app.read(cx).pull_modal().expect("dirty Pull confirmation");
        assert!(modal.auto_stash, "dirty Pull must confirm auto-stash");
        assert!(
            modal.plan.blockers.is_empty(),
            "failure must come from execution"
        );
    });

    press_enter(cx, &app, window);
    wait_idle(cx, &app);
    cx.advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "restore after failed Pull\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("scratch.txt")).unwrap(),
        "restore untracked\n"
    );
    assert!(output(repo, &["stash", "list"]).is_empty());
    assert_stash_push_recovery(repo);
    assert!(
        records(repo, "pull")
            .iter()
            .any(|entry| matches!(entry.outcome, OpOutcome::Failed { .. })),
        "Pull failure must be durable"
    );
    cx.read(|cx| {
        let modal = app
            .read(cx)
            .pull_modal()
            .expect("failed Pull modal must survive watcher reload");
        assert!(modal.error.is_some(), "failed Pull must show its error");
    });

    unmount(cx, app, window);
    eprintln!(
        "[gui-e2e] PASS pull_auto_stash_failure_restores: failed Pull restores changes and keeps its modal"
    );
}
