//! Integration tests for force-with-lease push (branch-menu "Advanced /
//! Dangerous" group, "Force-with-lease push...").
//!
//! All repositories (local + bare remote) are created inside `TempDir`s. No
//! network access: the "remote" is a local bare repository on disk.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::ops::{execute_force_with_lease_push, plan_force_with_lease_push};

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

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn remote_head_sha(remote: &Path, branch: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(remote)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Layout: tmp/remote.git (bare) + tmp/local (clone, upstream set on main) +
/// tmp/other (a second clone used to push "from elsewhere").
struct Repos {
    _tmp: TempDir,
    remote: PathBuf,
    local: PathBuf,
    other: PathBuf,
}

fn setup() -> Repos {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    let other = tmp.path().join("other");

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

    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);

    Repos {
        _tmp: tmp,
        remote,
        local,
        other,
    }
}

#[test]
fn test_plan_normal_no_blockers_after_amend() {
    let r = setup();
    // Amend locally so `local`'s HEAD diverges from the remote-tracking ref
    // (the classic force-with-lease scenario).
    write_file(&r.local, "base.txt", "base amended\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);

    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan failed");

    assert!(
        plan.blockers.is_empty(),
        "expected no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.destructive,
        "force-with-lease push must be destructive"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("overwrites the remote branch")),
        "expected a rewrites-history warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_plan_nothing_to_push_blocker() {
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan failed");
    assert!(
        !plan.blockers.is_empty(),
        "local == remote tip should block (nothing to force-push)"
    );
}

#[test]
fn test_execute_overwrites_remote_after_amend() {
    let r = setup();
    write_file(&r.local, "base.txt", "base amended\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);
    let new_local_sha = head_sha(&r.local);

    let repo = Repository::open(&r.local).expect("open local");
    execute_force_with_lease_push(&repo, &r.local).expect("execute failed");

    assert_eq!(
        remote_head_sha(&r.remote, "main"),
        new_local_sha,
        "remote main should now match the amended local commit"
    );
}

#[test]
fn test_execute_rejects_when_remote_moved_since_last_fetch() {
    let r = setup();

    // Someone else pushes to the remote via `other`, without `local` ever
    // fetching it.
    write_file(&r.other, "other.txt", "other\n");
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "concurrent change"]);
    git(&r.other, &["push", "-q", "origin", "main"]);
    let concurrent_sha = remote_head_sha(&r.remote, "main");

    // `local` amends its own (now-stale) view of main.
    write_file(&r.local, "base.txt", "base amended locally\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);

    let repo = Repository::open(&r.local).expect("open local");
    let result = execute_force_with_lease_push(&repo, &r.local);

    assert!(
        result.is_err(),
        "the lease must reject a push when the remote moved since local's last known state"
    );
    assert_eq!(
        remote_head_sha(&r.remote, "main"),
        concurrent_sha,
        "the concurrent change on the remote must survive the rejected push"
    );
}
