//! Worktree creation pipeline tests (T-CM-023/T-CM-024).

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    ops::{execute_create_worktree, plan_create_worktree, preflight_check, validate_worktree_path},
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

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn build_repo(tmp: &TempDir) -> Repository {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);

    Repository::open(d).expect("failed to open repo")
}

fn head_commit_id(repo: &Repository) -> CommitId {
    CommitId(
        repo.head()
            .expect("head")
            .target()
            .expect("head target")
            .to_string(),
    )
}

#[test]
fn create_worktree_success_creates_branch_and_linked_repo() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-feature");

    let plan = plan_create_worktree(&repo, "wt-feature", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );

    preflight_check(&repo, &plan).expect("preflight");
    execute_create_worktree(&repo, "wt-feature", &path, &at).expect("execute_create_worktree");

    assert!(path.join("README.md").exists());
    assert!(repo
        .find_branch("wt-feature", git2::BranchType::Local)
        .is_ok());
    let linked = Repository::open(&path).expect("open linked worktree");
    assert_eq!(linked.head().unwrap().shorthand().ok(), Some("wt-feature"));
}

#[test]
fn create_worktree_path_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("exists");
    std::fs::create_dir(&path).unwrap();

    let plan =
        plan_create_worktree(&repo, "wt-collision", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers.iter().any(|b| b.contains("already exists")),
        "expected path collision blocker, got {:?}",
        plan.blockers
    );
}

#[test]
fn create_worktree_branch_collision_is_blocker() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-main");

    let plan = plan_create_worktree(&repo, "main", &path, &at).expect("plan_create_worktree");
    assert!(
        plan.blockers.iter().any(|b| b.contains("already exists")),
        "expected branch collision blocker, got {:?}",
        plan.blockers
    );
}

#[test]
fn create_worktree_preflight_detects_head_move() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo = build_repo(&repo_tmp);
    let at = head_commit_id(&repo);
    let path = worktrees_tmp.path().join("wt-preflight");

    let plan =
        plan_create_worktree(&repo, "wt-preflight", &path, &at).expect("plan_create_worktree");
    assert!(plan.blockers.is_empty());

    write_file(repo_tmp.path(), "second.txt", "second\n");
    git(repo_tmp.path(), &["add", "second.txt"]);
    git(repo_tmp.path(), &["commit", "-qm", "second commit"]);

    let moved_repo = Repository::open(repo_tmp.path()).unwrap();
    assert!(
        preflight_check(&moved_repo, &plan).is_err(),
        "preflight should reject a moved HEAD"
    );
}

#[test]
fn validate_worktree_path_rejects_repo_inside_and_accepts_japanese_path() {
    let repo_tmp = TempDir::new().unwrap();
    let worktrees_tmp = TempDir::new().unwrap();
    let repo_root = repo_tmp.path();

    let inside = validate_worktree_path(repo_root, "inside-wt");
    assert!(inside.is_err(), "repo-internal path should be rejected");

    let japanese = worktrees_tmp.path().join("作業ツリー");
    let normalized =
        validate_worktree_path(repo_root, &japanese).expect("Japanese path should validate");
    assert_eq!(
        normalized,
        std::fs::canonicalize(worktrees_tmp.path())
            .unwrap()
            .join("作業ツリー")
    );
}
