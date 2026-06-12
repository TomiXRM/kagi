//! Integration tests for delete-branch — W2-DELETE
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_delete_branch_merged_success` | merged branch deleted successfully |
//! | 2 | `test_plan_delete_branch_unmerged_blocker` | unmerged branch → plan returns blocker |
//! | 3 | `test_plan_delete_branch_current_branch_blocker` | current branch → plan returns blocker |
//! | 4 | `test_plan_delete_branch_nonexistent_blocker` | non-existent branch → plan returns blocker |
//! | 5 | `test_delete_branch_recovery_sha` | recovery string contains the tip SHA |
//! | 6 | `test_execute_delete_branch_preflight_mismatch` | HEAD moved → execute returns Refused |
//! | 7 | `test_delete_branch_upstream_warning` | upstream configured → plan shows warning |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{execute_delete_branch, plan_delete_branch};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

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

#[allow(dead_code)]
fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repo with:
///   - `main`: two commits (initial + "base")
///   - `merged`: branched from initial commit, one commit, merged into main via --no-ff
///   - `unmerged`: branched from main, one commit, NOT merged into main
struct TestRepo {
    _tmp: TempDir,
    path: PathBuf,
}

fn setup_repo() -> TestRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(&path, "README.md", "# test\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    // Create `merged` branch and commit.
    git(&path, &["checkout", "-q", "-b", "merged"]);
    write_file(&path, "merged.txt", "merged content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "merged branch commit"]);

    // Merge `merged` into main (no-ff so a real merge commit is created).
    git(&path, &["checkout", "-q", "main"]);
    git(&path, &["merge", "--no-ff", "-m", "merge merged into main", "merged"]);

    // Create `unmerged` branch and commit (NOT merged into main).
    git(&path, &["checkout", "-q", "-b", "unmerged"]);
    write_file(&path, "unmerged.txt", "unmerged content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "unmerged branch commit"]);

    // Back to main.
    git(&path, &["checkout", "-q", "main"]);

    TestRepo { _tmp: tmp, path }
}

// ────────────────────────────────────────────────────────────
// Test 1: merged branch → plan has no blockers, execute succeeds
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_merged_success() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();

    // Plan should have no blockers for the merged branch.
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");
    assert!(
        plan.blockers.is_empty(),
        "merged branch must not have blockers, got: {:?}",
        plan.blockers
    );

    // Branch must exist before deletion.
    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_ok(),
        "merged branch should exist before delete"
    );

    // Execute must succeed.
    execute_delete_branch(&repo, &plan, "merged").expect("delete should succeed");

    // Branch must be gone.
    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_err(),
        "merged branch must be gone after delete"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: unmerged branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_unmerged_blocker() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "unmerged").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "unmerged branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("unmerged") || msg.contains("not reachable"),
        "blocker must mention unmerged/not-reachable: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: current branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_current_branch_blocker() {
    let r = setup_repo();

    // HEAD is on `main`.
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "main").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "current branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("checked-out") || msg.contains("current"),
        "blocker must mention current/checked-out: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 4: non-existent branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_nonexistent_blocker() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "does-not-exist").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "non-existent branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("does not exist") || msg.contains("not found"),
        "blocker must mention does-not-exist/not-found: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: recovery string contains the tip SHA
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_recovery_sha() {
    let r = setup_repo();

    // Get the tip SHA of `merged`.
    let tip_sha_out = Command::new("git")
        .args(["rev-parse", "--short", "merged"])
        .current_dir(&r.path)
        .output()
        .expect("rev-parse failed");
    let tip_short = String::from_utf8_lossy(&tip_sha_out.stdout)
        .trim()
        .to_string();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");

    assert!(
        plan.recovery.contains(&tip_short),
        "recovery string must contain tip SHA '{}', got: {}",
        tip_short,
        plan.recovery
    );

    // Also check that predicted text contains the tip SHA.
    assert!(
        plan.title.contains(&tip_short),
        "plan title must contain tip SHA '{}', got: {}",
        tip_short,
        plan.title
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: preflight mismatch → execute returns Err
// ────────────────────────────────────────────────────────────

#[test]
fn test_execute_delete_branch_preflight_mismatch() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    // Initial commit.
    write_file(&path, "base.txt", "base\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "base"]);

    // Create and merge a branch.
    git(&path, &["checkout", "-q", "-b", "to-delete"]);
    write_file(&path, "td.txt", "td\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "to delete commit"]);
    git(&path, &["checkout", "-q", "main"]);
    git(&path, &["merge", "--no-ff", "-m", "merge to-delete", "to-delete"]);

    // Build plan (captures current HEAD).
    let repo = Repository::open(&path).unwrap();
    let plan = plan_delete_branch(&repo, "to-delete").expect("plan should succeed");
    assert!(plan.blockers.is_empty(), "should have no blockers: {:?}", plan.blockers);

    // Simulate HEAD movement: add a new commit on main after planning.
    drop(repo);
    write_file(&path, "extra.txt", "extra\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "extra commit (moves HEAD after planning)"]);

    // Execute must fail because HEAD moved.
    let repo2 = Repository::open(&path).unwrap();
    let result = execute_delete_branch(&repo2, &plan, "to-delete");
    assert!(
        result.is_err(),
        "execute must fail when HEAD moved since planning"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("changed") || err.contains("re-plan") || err.contains("plan"),
        "error must mention state change/re-plan: {}",
        err
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: upstream configured → plan shows warning
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_upstream_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");

    // Create bare remote.
    git(
        tmp.path(),
        &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()],
    );

    // Create local repo.
    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(&local, &["remote", "add", "origin", remote.to_str().unwrap()]);

    // Initial commit + push.
    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    // Create a branch, push it (sets upstream), merge it into main.
    git(&local, &["checkout", "-q", "-b", "feat"]);
    write_file(&local, "feat.txt", "feat\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "feat commit"]);
    git(&local, &["push", "-q", "-u", "origin", "feat"]);
    git(&local, &["checkout", "-q", "main"]);
    git(&local, &["merge", "--no-ff", "-m", "merge feat", "feat"]);

    // Plan should have no blockers but a warning about upstream.
    let repo = Repository::open(&local).unwrap();
    let plan = plan_delete_branch(&repo, "feat").expect("plan should succeed");

    assert!(
        plan.blockers.is_empty(),
        "merged branch with upstream must not have blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        !plan.warnings.is_empty(),
        "plan must have a warning about the upstream not being deleted, got none"
    );
    let warn_msg = plan.warnings.join(" ");
    assert!(
        warn_msg.contains("upstream") || warn_msg.contains("remote"),
        "warning must mention upstream/remote: {}",
        warn_msg
    );
}

// ────────────────────────────────────────────────────────────
// Regression: duplicated gh CLI branch config keys must not
// break deletion (user repo had dozens of duplicated
// `branch.<name>.github-pr-owner-number` entries written by
// `gh pr`, making the 1st delete fail and the 2nd succeed).
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_with_duplicated_gh_config_keys() {
    let r = setup_repo();

    // Simulate gh CLI's duplicated-key pollution on the merged branch.
    for _ in 0..3 {
        git(
            &r.path,
            &[
                "config",
                "--add",
                "branch.merged.github-pr-owner-number",
                "owner#repo#42",
            ],
        );
    }
    // A second polluted key, plus a normal upstream-style key.
    git(&r.path, &["config", "--add", "branch.merged.gh-merge-base", "main"]);
    git(&r.path, &["config", "--add", "branch.merged.gh-merge-base", "main"]);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    // First attempt must succeed (this used to fail with
    // "could not find key '…github-pr-owner-number' to delete").
    execute_delete_branch(&repo, &plan, "merged")
        .expect("delete must succeed on the FIRST attempt despite duplicated config keys");

    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_err(),
        "branch must be gone"
    );
    // The polluted section must be cleaned up.
    let cfg = repo.config().unwrap().snapshot().unwrap();
    let mut leftover = 0;
    let mut entries = cfg.entries(Some("branch\\.merged\\..*")).unwrap();
    while let Some(_) = entries.next() {
        leftover += 1;
    }
    assert_eq!(leftover, 0, "branch.merged.* config entries must be removed");
}
