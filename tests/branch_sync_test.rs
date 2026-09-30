//! Branch context menu sync/manage operation tests.

#[path = "support/backend_ops.rs"]
mod backend_ops;
use backend_ops::{
    execute_pull_branch_ff, execute_push_branch, execute_rename_branch, execute_set_upstream,
};
use std::path::{Path, PathBuf};

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi_git::{
    plan_pull_branch_ff, plan_push_branch, plan_rename_branch, plan_set_upstream,
    validate_branch_rename, BranchRenameValidation, PullOutcome,
};

#[path = "support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};

fn rev_parse(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
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
    init_repo(&local, "main");
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    commit_all(&local, "base");
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    git(&local, &["checkout", "-q", "-b", "feature/x"]);
    write_file(&local, "feature.txt", "one\n");
    commit_all(&local, "feature one");
    git(&local, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&local, &["checkout", "-q", "main"]);

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
fn non_current_pull_ff_updates_ref_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    assert_eq!(
        rev_parse(&r.local, "HEAD"),
        before_head,
        "HEAD must not move"
    );
    assert!(
        !r.local.join("remote.txt").exists(),
        "working tree must not change"
    );
}

#[test]
fn non_current_push_uses_branch_upstream() {
    if !crate::test_support::run_isolated() {
        return;
    }
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

/// The menu offers plain Push whenever an upstream exists; one that tracks
/// its base (`origin/main`) is published to `origin/<branch>` and moved there.
#[test]
fn non_current_push_of_branch_tracking_base_moves_upstream() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(
        &r.local,
        &[
            "checkout",
            "-q",
            "-b",
            "topic/base",
            "--track",
            "origin/main",
        ],
    );
    write_file(&r.local, "topic.txt", "topic\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "topic"]);
    let tip = rev_parse(&r.local, "topic/base");
    git(&r.local, &["checkout", "-q", "main"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_branch(&repo, "topic/base", false).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_push_branch(&repo, &r.local, &plan, "topic/base", false).expect("push");
    assert!(outcome.set_upstream);
    assert_eq!(outcome.pushed, 1);
    assert_eq!(rev_parse(&r.remote, "refs/heads/topic/base"), tip);
    let branch = repo.find_branch("topic/base", BranchType::Local).unwrap();
    assert_eq!(
        branch.upstream().unwrap().name().unwrap(),
        Some("origin/topic/base")
    );
}

#[test]
fn set_upstream_is_config_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "-b", "topic/no-upstream"]);
    git(&r.local, &["checkout", "-q", "main"]);
    let before_head = rev_parse(&r.local, "HEAD");

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_set_upstream(&repo, "topic/no-upstream", "origin/main").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_set_upstream(&repo, &plan, "topic/no-upstream", "origin/main").expect("set upstream");

    let branch = repo
        .find_branch("topic/no-upstream", BranchType::Local)
        .unwrap();
    let upstream = branch.upstream().expect("upstream");
    assert_eq!(upstream.name().unwrap().unwrap(), "origin/main");
    assert_eq!(rev_parse(&r.local, "HEAD"), before_head);
}

#[test]
fn rename_current_branch_carries_tracking_config() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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

#[path = "support/isolated.rs"]
mod test_support;
