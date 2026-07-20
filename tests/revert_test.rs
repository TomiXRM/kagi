//! Integration tests for the revert operation pipeline (T-CM-034).
//!
//! All repositories are created under `TempDir`; no existing user repository is
//! touched.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    ops::{execute_revert, plan_revert, preflight_check},
    CommitId,
};

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
    assert!(
        status.success(),
        "git {} exited with {:?}",
        args.join(" "),
        status.code()
    );
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} exited with {:?}\nstderr: {}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).expect("read_file failed")
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit_file(dir: &Path, name: &str, content: &str, message: &str) -> CommitId {
    write_file(dir, name, content);
    git(dir, &["add", name]);
    git(dir, &["commit", "-qm", message]);
    CommitId(git_output(dir, &["rev-parse", "HEAD"]))
}

#[test]
fn revert_success_creates_commit_and_updates_worktree() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    commit_file(dir, "file.txt", "base\n", "initial");
    let target = commit_file(dir, "file.txt", "base\nfeature\n", "add feature");
    let old_head = git_output(dir, &["rev-parse", "HEAD"]);
    let repo = Repository::open(dir).unwrap();

    let plan = plan_revert(&repo, &target).expect("plan_revert failed");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert!(
        plan.predicted.head.contains("revert commit"),
        "predicted should mention revert commit creation: {}",
        plan.predicted.head
    );
    preflight_check(&repo, &plan).expect("preflight should pass");

    let new_id = execute_revert(&repo, &target).expect("execute_revert failed");
    assert_ne!(new_id.0, old_head, "revert must create a new commit");
    assert_eq!(git_output(dir, &["rev-parse", "HEAD"]), new_id.0);
    assert_eq!(read_file(dir, "file.txt"), "base\n");
    assert_eq!(git_output(dir, &["status", "--porcelain"]), "");
    assert!(git_output(dir, &["log", "-1", "--pretty=%s"]).starts_with("Revert \"add feature\""));
}

#[test]
fn revert_conflict_is_blocker_and_leaves_repo_untouched() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    commit_file(dir, "file.txt", "base\n", "initial");
    let target = commit_file(dir, "file.txt", "target\n", "target change");
    commit_file(dir, "file.txt", "other\n", "divergent change");
    let head_before = git_output(dir, &["rev-parse", "HEAD"]);
    let content_before = read_file(dir, "file.txt");

    let repo = Repository::open(dir).unwrap();
    let plan = plan_revert(&repo, &target).expect("plan_revert failed");

    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("conflict") && b.message_en().contains("file.txt")),
        "expected conflict blocker with file name, got: {:?}",
        plan.blockers
    );
    assert_eq!(git_output(dir, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(read_file(dir, "file.txt"), content_before);
    assert_eq!(git_output(dir, &["status", "--porcelain"]), "");
}

#[test]
fn revert_merge_commit_is_blocked_by_plan() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    commit_file(dir, "base.txt", "base\n", "initial");
    git(dir, &["checkout", "-qb", "feature"]);
    commit_file(dir, "feature.txt", "feature\n", "feature change");
    git(dir, &["checkout", "-q", "main"]);
    commit_file(dir, "main.txt", "main\n", "main change");
    git(dir, &["merge", "--no-ff", "-m", "merge feature", "feature"]);
    let merge_id = CommitId(git_output(dir, &["rev-parse", "HEAD"]));

    let repo = Repository::open(dir).unwrap();
    let plan = plan_revert(&repo, &merge_id).expect("plan_revert failed");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("merge commit")),
        "expected merge blocker, got: {:?}",
        plan.blockers
    );
}

#[test]
fn revert_dirty_worktree_warns_without_blocking() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    commit_file(dir, "file.txt", "base\n", "initial");
    let target = commit_file(dir, "file.txt", "base\nfeature\n", "add feature");
    write_file(dir, "note.txt", "dirty\n");

    let repo = Repository::open(dir).unwrap();
    let plan = plan_revert(&repo, &target).expect("plan_revert failed");
    assert!(
        plan.blockers.is_empty(),
        "dirty should warn, not block: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("modified") || w.message_en().contains("untracked")),
        "expected dirty warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn revert_preflight_fails_when_head_moves_after_plan() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    commit_file(dir, "file.txt", "base\n", "initial");
    let target = commit_file(dir, "file.txt", "base\nfeature\n", "add feature");
    let repo = Repository::open(dir).unwrap();
    let plan = plan_revert(&repo, &target).expect("plan_revert failed");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );

    commit_file(dir, "later.txt", "later\n", "later change");
    let repo_after = Repository::open(dir).unwrap();
    let err = preflight_check(&repo_after, &plan).expect_err("preflight should fail");
    assert!(
        err.to_string()
            .contains("Repository state changed since planning"),
        "unexpected preflight error: {}",
        err
    );
}
