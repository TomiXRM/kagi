//! Integration tests for checkout operation pipeline (T013).
//!
//! All write operations are confined to `TempDir` repositories created within
//! each test.  This project's own repository and any other existing repository
//! are **never** touched.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    Head,
    ops::{execute_checkout, plan_checkout, preflight_check},
    snapshot,
};

// ────────────────────────────────────────────────────────────
// Helpers (copied from snapshot_test pattern)
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
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

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Build a minimal repo with two branches: `main` and `feature/one`.
/// HEAD is on `main`.  The repo is initially clean.
///
/// Returns `(TempDir, repo_dir_path, opened Repository)`.
fn build_two_branch_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();

    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);

    // Create feature/one from main.
    git(d, &["checkout", "-qb", "feature/one"]);
    write_file(d, "feat.txt", "feature work\n");
    git(d, &["add", "feat.txt"]);
    git(d, &["commit", "-qm", "feature/one work"]);

    // Return to main.
    git(d, &["checkout", "-q", "main"]);

    let repo = Repository::open(d).expect("failed to open repo");
    (d.to_path_buf(), repo)
}

// ────────────────────────────────────────────────────────────
// Test 1: clean repo — plan has no blockers, execute moves HEAD
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_clean_repo_no_blockers() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "clean repo should have no blockers, got: {:?}",
        plan.blockers
    );
    assert_eq!(
        plan.predicted.head,
        "branch: feature/one",
        "predicted HEAD should be 'branch: feature/one'"
    );
}

#[test]
fn test_execute_clean_repo_moves_head() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty(), "should have no blockers");

    preflight_check(&repo, &plan).expect("preflight_check failed");
    execute_checkout(&repo, "feature/one").expect("execute_checkout failed");

    // Re-open and verify HEAD moved.
    let repo2 = Repository::open(&repo_dir).expect("re-open repo");
    let mut repo2 = repo2;
    let snap = snapshot(&mut repo2, 100).expect("snapshot after checkout");
    assert!(
        matches!(&snap.head, Head::Attached { branch, .. } if branch == "feature/one"),
        "HEAD should be on feature/one after checkout, got: {:?}",
        snap.head
    );

    // Verify feat.txt was checked out.
    assert!(
        repo_dir.join("feat.txt").exists(),
        "feat.txt should exist after checkout to feature/one"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: dirty repo (unstaged modified) — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_dirty_unstaged_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Modify README.md without staging.
    write_file(&repo_dir, "README.md", "modified content\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty repo (unstaged) should have at least one blocker"
    );
    let has_stash_mention = plan.blockers.iter().any(|b| b.contains("stash"));
    assert!(
        has_stash_mention,
        "blocker should mention 'stash', got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_plan_dirty_staged_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Stage a new file.
    write_file(&repo_dir, "staged.txt", "staged\n");
    git(&repo_dir, &["add", "staged.txt"]);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty repo (staged) should have at least one blocker"
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: untracked only — no blocker, has warning, execute succeeds
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_untracked_only_no_blocker_warning() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Add an untracked file (not staged).
    write_file(&repo_dir, "untracked.txt", "not tracked\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "untracked-only should have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        !plan.warnings.is_empty(),
        "untracked-only should have a warning about untracked files"
    );
}

#[test]
fn test_execute_untracked_only_file_remains() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Add an untracked file.
    write_file(&repo_dir, "untracked.txt", "not tracked\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty());

    preflight_check(&repo, &plan).expect("preflight failed");
    execute_checkout(&repo, "feature/one").expect("execute failed");

    // Untracked file should still be there.
    assert!(
        repo_dir.join("untracked.txt").exists(),
        "untracked.txt should survive the checkout"
    );

    // HEAD should have moved.
    let repo2 = Repository::open(&repo_dir).expect("re-open");
    let head_ref = repo2.head().expect("head");
    let branch = head_ref.shorthand().unwrap_or("");
    assert_eq!(branch, "feature/one", "HEAD should be on feature/one");
}

// ────────────────────────────────────────────────────────────
// Test 4: nonexistent branch — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_nonexistent_branch_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "nonexistent-branch").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "nonexistent branch should have a blocker"
    );
    let has_not_exist = plan
        .blockers
        .iter()
        .any(|b| b.contains("not exist") || b.contains("does not exist"));
    assert!(
        has_not_exist,
        "blocker should mention branch not existing, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: already on HEAD branch — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_already_head_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    // HEAD is on main; try to plan checkout of main.
    let plan = plan_checkout(&repo, "main").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "checking out the current HEAD branch should produce a blocker"
    );
    let has_already = plan
        .blockers
        .iter()
        .any(|b| b.contains("already") || b.contains("current HEAD"));
    assert!(
        has_already,
        "blocker should mention 'already' or 'current HEAD', got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: preflight detects HEAD change, returns error
// ────────────────────────────────────────────────────────────

#[test]
fn test_preflight_aborts_when_head_changed() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Plan while on main.
    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty());

    // Simulate HEAD changing between plan and execute: add a commit on main.
    write_file(&repo_dir, "extra.txt", "extra\n");
    git(&repo_dir, &["add", "extra.txt"]);
    git(&repo_dir, &["commit", "-qm", "extra commit"]);

    // preflight_check must detect the HEAD target changed.
    let result = preflight_check(&repo, &plan);
    assert!(
        result.is_err(),
        "preflight_check should return Err when HEAD has changed"
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: execute_checkout on nonexistent branch returns error
// ────────────────────────────────────────────────────────────

#[test]
fn test_execute_nonexistent_branch_returns_error() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let result = execute_checkout(&repo, "does-not-exist");
    assert!(
        result.is_err(),
        "execute_checkout on nonexistent branch should return Err"
    );
}

// ────────────────────────────────────────────────────────────
// Test 8: plan includes recovery text with original branch name
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_recovery_mentions_original_branch() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.recovery.contains("main"),
        "recovery text should mention 'main' (the original branch), got: {:?}",
        plan.recovery
    );
    assert!(
        plan.recovery.contains("reflog"),
        "recovery text should mention 'reflog', got: {:?}",
        plan.recovery
    );
}
