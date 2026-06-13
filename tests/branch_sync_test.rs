//! Branch context menu sync/manage operation tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi::git::{
    BranchRenameValidation, PullOutcome, execute_pull_branch_ff, execute_push_branch,
    execute_rename_branch, execute_set_upstream, plan_pull_branch_ff, plan_push_branch,
    plan_rename_branch, plan_set_upstream, validate_branch_rename,
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
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write failed");
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    assert!(out.status.success(), "rev-parse {} failed", rev);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

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

    git(tmp.path(), &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(&local, &["remote", "add", "origin", remote.to_str().unwrap()]);

    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    git(&local, &["checkout", "-q", "-b", "feature/x"]);
    write_file(&local, "feature.txt", "one\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "feature one"]);
    git(&local, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&local, &["checkout", "-q", "main"]);

    git(tmp.path(), &["clone", "-q", remote.to_str().unwrap(), other.to_str().unwrap()]);
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);

    Repos { _tmp: tmp, remote, local, other }
}

#[test]
fn non_current_pull_ff_updates_ref_only() {
    let r = setup();
    git(&r.other, &["checkout", "-q", "feature/x"]);
    write_file(&r.other, "remote.txt", "remote\n");
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "remote feature"]);
    git(&r.other, &["push", "-q", "origin", "feature/x"]);
    git(&r.local, &["fetch", "-q", "origin"]);

    let before_head = rev_parse(&r.local, "HEAD");
    let remote_feature = rev_parse(&r.local, "origin/feature/x");
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x").expect("execute");
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));
    assert_eq!(rev_parse(&r.local, "feature/x"), remote_feature);
    assert_eq!(rev_parse(&r.local, "HEAD"), before_head, "HEAD must not move");
    assert!(!r.local.join("remote.txt").exists(), "working tree must not change");
}

#[test]
fn non_current_push_uses_branch_upstream() {
    let r = setup();
    git(&r.local, &["checkout", "-q", "feature/x"]);
    write_file(&r.local, "local.txt", "local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local feature"]);
    let feature_tip = rev_parse(&r.local, "feature/x");
    git(&r.local, &["checkout", "-q", "main"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_branch(&repo, "feature/x", false).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_push_branch(&repo, &r.local, &plan, "feature/x", false).expect("push");
    assert_eq!(outcome.pushed, 1);
    assert_eq!(rev_parse(&r.remote, "refs/heads/feature/x"), feature_tip);
    assert_eq!(rev_parse(&r.local, "HEAD"), rev_parse(&r.local, "main"));
}

#[test]
fn set_upstream_is_config_only() {
    let r = setup();
    git(&r.local, &["checkout", "-q", "-b", "topic/no-upstream"]);
    git(&r.local, &["checkout", "-q", "main"]);
    let before_head = rev_parse(&r.local, "HEAD");

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_set_upstream(&repo, "topic/no-upstream", "origin/main").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_set_upstream(&repo, &plan, "topic/no-upstream", "origin/main").expect("set upstream");

    let branch = repo.find_branch("topic/no-upstream", BranchType::Local).unwrap();
    let upstream = branch.upstream().expect("upstream");
    assert_eq!(upstream.name().unwrap().unwrap(), "origin/main");
    assert_eq!(rev_parse(&r.local, "HEAD"), before_head);
}

#[test]
fn rename_current_branch_carries_tracking_config() {
    let r = setup();
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_rename_branch(&repo, "main", "trunk").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_rename_branch(&repo, &plan, "main", "trunk").expect("rename");

    assert!(repo.find_branch("main", BranchType::Local).is_err());
    assert!(repo.find_branch("trunk", BranchType::Local).is_ok());
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "trunk");
    let branch = repo.find_branch("trunk", BranchType::Local).unwrap();
    assert_eq!(
        branch.upstream().unwrap().name().unwrap().unwrap(),
        "origin/main"
    );
}

#[test]
fn branch_rename_validation_is_pure() {
    let existing = vec!["main".to_string(), "feature/x".to_string()];
    assert_eq!(
        validate_branch_rename("main", "topic/new", &existing),
        BranchRenameValidation::Valid
    );
    assert!(matches!(
        validate_branch_rename("main", "feature/x", &existing),
        BranchRenameValidation::Invalid(_)
    ));
    assert!(matches!(
        validate_branch_rename("main", "bad name.lock", &existing),
        BranchRenameValidation::Invalid(_)
    ));
    assert!(matches!(
        validate_branch_rename("main", " main", &existing),
        BranchRenameValidation::Invalid(_)
    ));
}
