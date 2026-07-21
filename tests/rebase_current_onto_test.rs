//! Integration tests for rebase-current-onto (branch-menu "Integrate" group,
//! "Rebase current branch onto <target>").
//!
//! All write operations are confined to `TempDir` repositories created
//! within each test.

use std::path::Path;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::ops::{execute_rebase_current_onto, plan_rebase_current_onto, RebaseOutcome};

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
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
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `main`: base -> "main change" (file2.txt). `side` (checked out): base ->
/// "side change" (file1.txt), branched before "main change" — a clean,
/// non-conflicting rebase target.
fn clean_rebase_repo() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    git(&dir, &["init", "-q", "-b", "main", "."]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);

    write_file(&dir, "base.txt", "base\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "base"]);

    git(&dir, &["checkout", "-q", "-b", "side"]);
    write_file(&dir, "file1.txt", "side\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "side change"]);

    git(&dir, &["checkout", "-q", "main"]);
    write_file(&dir, "file2.txt", "main\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "main change"]);

    git(&dir, &["checkout", "-q", "side"]);
    (tmp, dir)
}

#[test]
fn test_plan_normal_no_blockers() {
    let (_tmp, dir) = clean_rebase_repo();
    let repo = Repository::open(&dir).expect("open");

    let plan = plan_rebase_current_onto(&repo, "main").expect("plan failed");

    assert!(
        plan.blockers.is_empty(),
        "expected no blockers, got: {:?}",
        plan.blockers
    );
    assert!(!plan.destructive, "rebase is Guarded, not Destructive");
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("conflict")),
        "expected a may-conflict warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_plan_dirty_working_tree_blocker() {
    let (_tmp, dir) = clean_rebase_repo();
    write_file(&dir, "base.txt", "dirty\n");
    let repo = Repository::open(&dir).expect("open");

    let plan = plan_rebase_current_onto(&repo, "main").expect("plan failed");
    assert!(!plan.blockers.is_empty(), "dirty tree should block");
}

#[test]
fn test_plan_invalid_onto_blocker() {
    let (_tmp, dir) = clean_rebase_repo();
    let repo = Repository::open(&dir).expect("open");

    let plan = plan_rebase_current_onto(&repo, "no-such-branch").expect("plan failed");
    assert!(!plan.blockers.is_empty(), "invalid onto should block");
}

#[test]
fn test_plan_already_up_to_date_blocker() {
    let (_tmp, dir) = clean_rebase_repo();
    let repo = Repository::open(&dir).expect("open");

    let plan = plan_rebase_current_onto(&repo, "side").expect("plan failed");
    assert!(
        !plan.blockers.is_empty(),
        "rebasing onto self should block (nothing to replay)"
    );
}

#[test]
fn test_execute_clean_rebase_completes() {
    let (_tmp, dir) = clean_rebase_repo();
    let repo = Repository::open(&dir).expect("open");

    let outcome = execute_rebase_current_onto(&repo, &dir, "main").expect("execute failed");
    match outcome {
        RebaseOutcome::Completed { head } => {
            assert_eq!(head.0, head_sha(&dir));
        }
        other => panic!("expected Completed, got {:?}", other),
    }

    // side's tip is now a descendant of main's tip.
    let side_parent = format!("{}^", head_sha(&dir));
    let side_parent_sha = std::process::Command::new("git")
        .args(["rev-parse", &side_parent])
        .current_dir(&dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap();
    let main_sha = std::process::Command::new("git")
        .args(["rev-parse", "main"])
        .current_dir(&dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap();
    assert_eq!(side_parent_sha, main_sha);

    // Both files present — the rebase replayed cleanly.
    assert!(dir.join("base.txt").exists());
    assert!(dir.join("file1.txt").exists());
    assert!(dir.join("file2.txt").exists());
}

#[test]
fn test_execute_conflicting_rebase_reports_conflicted_not_error() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    git(&dir, &["init", "-q", "-b", "main", "."]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);

    write_file(&dir, "file.txt", "base\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "base"]);

    git(&dir, &["checkout", "-q", "-b", "side"]);
    write_file(&dir, "file.txt", "SIDE\n");
    git(&dir, &["commit", "-qam", "side change"]);

    git(&dir, &["checkout", "-q", "main"]);
    write_file(&dir, "file.txt", "MAIN\n");
    git(&dir, &["commit", "-qam", "main change"]);

    git(&dir, &["checkout", "-q", "side"]);
    let repo = Repository::open(&dir).expect("open");

    let outcome = execute_rebase_current_onto(&repo, &dir, "main").expect("execute failed");
    assert_eq!(
        outcome,
        RebaseOutcome::Conflicted,
        "a conflicting rebase must report Conflicted, not GitError"
    );
    assert!(dir.join(".git").join("rebase-merge").exists());
}
