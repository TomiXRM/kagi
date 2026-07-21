//! Integration tests for delete-remote-branch (branch-menu "Advanced /
//! Dangerous" group).
//!
//! All repositories (local + bare remote) are created inside `TempDir`s. No
//! network access: the "remote" is a local bare repository on disk.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::ops::{execute_delete_remote_branch, plan_delete_remote_branch};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git command failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Layout: tmp/remote.git (bare, with `main` + `feature/x`) + tmp/local (clone,
/// fetched so `refs/remotes/origin/feature/x` exists locally).
struct Repos {
    _tmp: TempDir,
    remote: PathBuf,
    local: PathBuf,
}

fn setup() -> Repos {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");

    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );

    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    git(&local, &["checkout", "-qb", "feature/x"]);
    write_file(&local, "feat.txt", "feature\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "feature work"]);
    git(&local, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&local, &["checkout", "-q", "main"]);

    Repos {
        _tmp: tmp,
        remote,
        local,
    }
}

fn remote_has_branch(remote: &Path, branch: &str) -> bool {
    let out = Command::new("git")
        .args(["ls-remote", "--heads", remote.to_str().unwrap(), branch])
        .output()
        .expect("ls-remote failed");
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

#[test]
fn test_plan_normal_no_blockers() {
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    let plan = plan_delete_remote_branch(&repo, "origin/feature/x")
        .expect("plan_delete_remote_branch failed");

    assert!(
        plan.blockers.is_empty(),
        "expected no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.destructive,
        "delete-remote-branch must be marked destructive"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("local branch")),
        "expected a local-branch-untouched warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_plan_not_found_blocker() {
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    let plan = plan_delete_remote_branch(&repo, "origin/does-not-exist")
        .expect("plan_delete_remote_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "a remote-tracking ref that doesn't exist locally should block"
    );
}

#[test]
fn test_execute_deletes_the_remote_branch() {
    let r = setup();

    execute_delete_remote_branch(&r.local, "origin/feature/x")
        .expect("execute_delete_remote_branch failed");

    assert!(
        !remote_has_branch(&r.remote, "feature/x"),
        "feature/x should no longer exist on the remote"
    );
    assert!(
        remote_has_branch(&r.remote, "main"),
        "main must be untouched"
    );
}

#[test]
fn test_execute_does_not_touch_local_branch() {
    let r = setup();

    execute_delete_remote_branch(&r.local, "origin/feature/x")
        .expect("execute_delete_remote_branch failed");

    let repo = Repository::open(&r.local).expect("open local");
    assert!(
        repo.find_branch("feature/x", git2::BranchType::Local)
            .is_ok(),
        "local branch 'feature/x' must survive a remote-branch delete"
    );

    // HEAD must still be on main — this op never touches HEAD.
    let head_ref = repo.head().expect("repo.head()");
    assert_eq!(head_ref.shorthand().unwrap_or(""), "main");
}
