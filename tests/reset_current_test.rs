//! Integration tests for reset-current-to-HEAD (branch-menu "Advanced /
//! Dangerous" group, "Reset current to this HEAD...").
//!
//! All write operations are confined to `TempDir` repositories created
//! within each test.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    ops::{execute_reset_current_to_head, plan_reset_current_to_head},
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

/// Three commits on `main`: c1 (base.txt) -> c2 (a.txt) -> c3 (b.txt), HEAD
/// at c3. Returns `(repo_dir, repo, [c1, c2, c3])`.
fn build_three_commit_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository, Vec<String>) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "base.txt", "base\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "c1"]);
    let c1 = head_sha(d);

    write_file(d, "a.txt", "a\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "c2"]);
    let c2 = head_sha(d);

    write_file(d, "b.txt", "b\n");
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", "c3"]);
    let c3 = head_sha(d);

    let repo = Repository::open(d).expect("open repo");
    (d.to_path_buf(), repo, vec![c1, c2, c3])
}

#[test]
fn test_plan_normal_no_blockers_and_warns_abandons() {
    let tmp = TempDir::new().unwrap();
    let (_dir, repo, commits) = build_three_commit_repo(&tmp);
    let target = CommitId(commits[0].clone());

    let plan = plan_reset_current_to_head(&repo, &target).expect("plan failed");

    assert!(
        plan.blockers.is_empty(),
        "no blockers expected, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.destructive,
        "reset-current-to-head must be destructive"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("2 commit(s)")),
        "expected an abandons-2-commits warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_execute_moves_the_branch_ref_only() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo, commits) = build_three_commit_repo(&tmp);
    let target = CommitId(commits[0].clone());

    execute_reset_current_to_head(&repo, &target).expect("execute failed");

    assert_eq!(
        head_sha(&dir),
        commits[0],
        "HEAD should now point at the target commit"
    );
    // Ref-only: files from the abandoned commits are untouched on disk
    // (index/working tree are never touched by this op).
    assert!(dir.join("a.txt").exists(), "a.txt must still be on disk");
    assert!(dir.join("b.txt").exists(), "b.txt must still be on disk");
}

#[test]
fn test_plan_missing_commit_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_dir, repo, _commits) = build_three_commit_repo(&tmp);
    let target = CommitId("0".repeat(40));

    let plan = plan_reset_current_to_head(&repo, &target).expect("plan failed");
    assert!(
        !plan.blockers.is_empty(),
        "a nonexistent commit should block"
    );
}

#[test]
fn test_plan_unrelated_target_warns_not_ancestor() {
    let tmp = TempDir::new().unwrap();
    let (dir, _repo, commits) = build_three_commit_repo(&tmp);

    // Create an unrelated branch/commit off c1, not an ancestor of c3 (HEAD).
    git(&dir, &["checkout", "-qb", "other", &commits[0]]);
    write_file(&dir, "other.txt", "other\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "unrelated"]);
    let unrelated = head_sha(&dir);
    git(&dir, &["checkout", "-q", "main"]);

    let repo2 = Repository::open(&dir).expect("re-open");
    let plan = plan_reset_current_to_head(&repo2, &CommitId(unrelated)).expect("plan failed");

    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("not an ancestor")),
        "expected a not-an-ancestor warning, got: {:?}",
        plan.warnings
    );
}
