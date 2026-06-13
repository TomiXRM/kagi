//! Branch context menu operation backend tests (T-BCM-030/061/073).

use std::path::Path;
use std::process::Command;

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi::git::ops::{
    default_tracking_branch_name, execute_checkout_tracking_branch, plan_checkout_tracking_branch,
    plan_merge_branch,
};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
    Repository::open(dir).expect("open repo")
}

#[test]
fn merge_plan_reports_fast_forward_direction() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);

    let plan = plan_merge_branch(&repo, "feature").expect("plan merge");
    assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan.blockers);
    assert_eq!(plan.title, "Merge feature into main");
    assert!(
        plan.predicted.head.contains("fast-forward"),
        "expected ff plan, got {}",
        plan.predicted.head
    );
    assert!(
        plan.preview_files
            .iter()
            .any(|f| f.path == Path::new("feature.txt"))
    );
}

#[test]
fn merge_plan_reports_merge_commit_for_diverged_branch() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    let plan = plan_merge_branch(&repo, "feature").expect("plan merge");
    assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan.blockers);
    assert_eq!(plan.title, "Merge feature into main");
    assert!(
        plan.predicted.head.contains("merge commit"),
        "expected merge-commit plan, got {}",
        plan.predicted.head
    );
}

#[test]
fn merge_plan_conflict_is_blocker_and_leaves_worktree_intact() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "same.txt", "base\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "same base"]);

    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "same.txt", "feature\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "same.txt", "main\n");
    git(dir, &["add", "same.txt"]);
    git(dir, &["commit", "-qm", "main change"]);

    let before = std::fs::read_to_string(dir.join("same.txt")).unwrap();
    let plan = plan_merge_branch(&repo, "feature").expect("plan merge");
    let after = std::fs::read_to_string(dir.join("same.txt")).unwrap();

    assert!(
        plan.blockers.iter().any(|b| b.contains("conflict")),
        "expected conflict blocker, got {:?}",
        plan.blockers
    );
    assert_eq!(before, after, "plan must not modify working tree");
}

#[test]
fn checkout_tracking_branch_plan_and_execute_from_file_remote() {
    let repo_tmp = TempDir::new().unwrap();
    let remote_tmp = TempDir::new().unwrap();
    let repo = init_repo(&repo_tmp);
    let dir = repo_tmp.path();
    let remote_url = format!("file://{}", remote_tmp.path().display());

    git(remote_tmp.path(), &["init", "-q", "--bare", "."]);
    git(dir, &["remote", "add", "origin", &remote_url]);
    git(dir, &["push", "-q", "-u", "origin", "main"]);
    git(dir, &["checkout", "-qb", "remote-only"]);
    write_file(dir, "remote.txt", "remote\n");
    git(dir, &["add", "remote.txt"]);
    git(dir, &["commit", "-qm", "remote only"]);
    git(dir, &["push", "-q", "origin", "remote-only"]);
    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["branch", "-D", "remote-only"]);
    git(dir, &["fetch", "-q", "origin"]);

    assert_eq!(
        default_tracking_branch_name("origin/remote-only"),
        "remote-only"
    );
    let plan = plan_checkout_tracking_branch(&repo, "origin/remote-only", "remote-only")
        .expect("tracking plan");
    assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan.blockers);

    execute_checkout_tracking_branch(&repo, "origin/remote-only", "remote-only")
        .expect("checkout tracking");

    let repo2 = Repository::open(dir).unwrap();
    assert_eq!(repo2.head().unwrap().shorthand().unwrap(), "remote-only");
    let branch = repo2
        .find_branch("remote-only", BranchType::Local)
        .expect("local branch");
    assert_eq!(
        branch.upstream().unwrap().name().unwrap(),
        Some("origin/remote-only")
    );
}

#[test]
fn checkout_tracking_branch_name_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let remote_tmp = TempDir::new().unwrap();
    let repo = init_repo(&repo_tmp);
    let dir = repo_tmp.path();
    let remote_url = format!("file://{}", remote_tmp.path().display());

    git(remote_tmp.path(), &["init", "-q", "--bare", "."]);
    git(dir, &["remote", "add", "origin", &remote_url]);
    git(dir, &["push", "-q", "-u", "origin", "main"]);
    git(dir, &["fetch", "-q", "origin"]);

    let plan = plan_checkout_tracking_branch(&repo, "origin/main", "main")
        .expect("tracking plan");
    assert!(
        plan.blockers.iter().any(|b| b.contains("already exists")),
        "expected collision blocker, got {:?}",
        plan.blockers
    );
}
