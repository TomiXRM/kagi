//! Integration tests for the `Backend::run` preflight gate (ADR-0104).
//!
//! `run` is the single enforced entry point for every mutating operation; its
//! preflight (HEAD unchanged since plan, plus stash-count unchanged for stash
//! ops) is the one place that stops a stale, user-confirmed plan from being
//! applied to a repository that moved underneath it. Every test here builds a
//! plan, moves the repo, calls `run`, and asserts BOTH the refusal and that the
//! repository was left untouched.
//!
//! Covers in particular the two destructive ops that take no plan argument, so
//! `run`'s preflight is their *only* staleness guard:
//! `ResetCurrentToHead` and `DeleteRemoteBranch`.
//!
//! All repositories (including the "remote", a local bare repo) live in
//! `TempDir`s — no network, never a user repo.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use kagi_git::{Backend, CommitId, Head, Operation};

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

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(out.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn head_sha(dir: &Path) -> String {
    git_out(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Repo with one commit on `main`, HEAD attached, clean.
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
}

/// Add a commit, moving HEAD out from under any plan built before it.
fn move_head(dir: &Path, name: &str) {
    write_file(dir, name, "moved\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", name]);
}

fn head_commit(backend: &Backend) -> CommitId {
    match backend.head_state().expect("head") {
        Head::Attached { target, .. } => CommitId(target),
        other => panic!("expected attached HEAD, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────
// The stash arm of run()'s preflight (preflight_check_stash)
// ────────────────────────────────────────────────────────────

/// A concurrent `stash push` shifts every stash index, so a confirmed
/// `StashApply { index }` plan now names a different entry. HEAD does NOT move
/// here, so only the stash arm of the preflight can catch this.
#[test]
fn run_rejects_stale_stash_plan() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    build_repo(d);

    // Stash #0: an edit to base.txt.
    write_file(d, "base.txt", "first stashed edit\n");
    git(d, &["stash", "push", "-qm", "first"]);

    let mut backend = Backend::open(d).expect("open");
    let op = Operation::StashApply { index: 0 };
    let plan = backend.plan(&op).expect("plan");
    assert_eq!(
        plan.stash_count_at_plan(),
        1,
        "plan captured the stash count"
    );

    let head_before = head_sha(d);

    // Someone else stashes: index 0 is now a DIFFERENT entry. HEAD is unchanged.
    write_file(d, "base.txt", "second stashed edit\n");
    git(d, &["stash", "push", "-qm", "second"]);
    assert_eq!(head_sha(d), head_before, "stashing must not move HEAD");

    let err = backend
        .run(&op, &plan)
        .expect_err("run must refuse a stash plan whose stash list shifted");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Stash list changed"),
        "expected the stash preflight error, got: {}",
        msg
    );

    // Nothing was applied: both stashes are still there and the WT is clean.
    assert_eq!(
        git_out(d, &["stash", "list"]).lines().count(),
        2,
        "no stash entry may be consumed by a refused run"
    );
    assert!(
        git_out(d, &["status", "--porcelain"]).trim().is_empty(),
        "working tree must be untouched by a refused run"
    );
    assert_eq!(head_sha(d), head_before, "HEAD must be untouched");
}

// ────────────────────────────────────────────────────────────
// The ordinary (HEAD) arm of run()'s preflight
// ────────────────────────────────────────────────────────────

#[test]
fn run_rejects_stale_create_branch_plan() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    build_repo(d);

    let mut backend = Backend::open(d).expect("open");
    let op = Operation::CreateBranch {
        name: "before-stale".to_string(),
        at: head_commit(&backend),
    };
    let plan = backend.plan(&op).expect("plan");

    move_head(d, "second.txt");

    let err = backend
        .run(&op, &plan)
        .expect_err("run must refuse a plan built before HEAD moved");
    assert!(
        format!("{:?}", err).contains("changed since planning"),
        "expected the HEAD preflight error, got: {:?}",
        err
    );
    assert!(
        !backend.local_branch_exists("before-stale"),
        "a refused run must not create the branch"
    );
}

// ────────────────────────────────────────────────────────────
// The two destructive ops whose ONLY staleness guard is run()
// ────────────────────────────────────────────────────────────

#[test]
fn run_rejects_stale_reset_current_to_head_plan() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    build_repo(d);
    let c1 = head_sha(d);
    move_head(d, "a.txt");
    let c2 = head_sha(d);

    let mut backend = Backend::open(d).expect("open");
    let op = Operation::ResetCurrentToHead {
        target: CommitId(c1.clone()),
    };
    let plan = backend.plan(&op).expect("plan");

    // Someone commits between confirm and execute: resetting to c1 would now
    // abandon one more commit than the user was shown.
    move_head(d, "b.txt");
    let c3 = head_sha(d);
    assert_ne!(c2, c3);

    let err = backend
        .run(&op, &plan)
        .expect_err("run must refuse a stale reset-current plan");
    assert!(
        format!("{:?}", err).contains("changed since planning"),
        "expected the HEAD preflight error, got: {:?}",
        err
    );
    assert_eq!(
        head_sha(d),
        c3,
        "a refused reset must leave the branch ref where it was"
    );
}

/// Layout: `tmp/remote.git` (bare, holds `main` + `feature/x`) and `tmp/local`.
fn setup_with_remote(tmp: &TempDir) -> (PathBuf, PathBuf) {
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
    build_repo(&local);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&local, &["push", "-q", "origin", "main"]);
    git(&local, &["branch", "feature/x"]);
    git(&local, &["push", "-q", "origin", "feature/x"]);
    git(&local, &["fetch", "-q", "origin"]);
    (remote, local)
}

fn remote_has_branch(remote: &Path, name: &str) -> bool {
    git_out(
        remote,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .any(|l| l.trim() == name)
}

#[test]
fn run_rejects_stale_delete_remote_branch_plan() {
    let tmp = TempDir::new().unwrap();
    let (remote, local) = setup_with_remote(&tmp);

    let mut backend = Backend::open(&local).expect("open");
    let op = Operation::DeleteRemoteBranch {
        remote_branch: "origin/feature/x".to_string(),
    };
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    // The repo moves under the confirmed plan.
    move_head(&local, "c.txt");
    let after = head_sha(&local);

    let err = backend
        .run(&op, &plan)
        .expect_err("run must refuse a stale delete-remote-branch plan");
    assert!(
        format!("{:?}", err).contains("changed since planning"),
        "expected the HEAD preflight error, got: {:?}",
        err
    );

    // Nothing was deleted, locally or on the remote.
    assert!(
        remote_has_branch(&remote, "feature/x"),
        "a refused run must not delete the remote branch"
    );
    assert!(
        backend.local_branch_exists("feature/x"),
        "the local branch must survive too"
    );
    assert_eq!(head_sha(&local), after, "HEAD must be untouched");
}
