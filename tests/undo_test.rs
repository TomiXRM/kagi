//! Integration tests for undo-commit — T-HT-009
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_undo_commit_normal` | happy path: HEAD moves to parent, changed files are staged, WT unchanged |
//! | 2 | `test_undo_commit_staged_remain` | after undo the diff shows the undone change in the INDEX (A/M prefix) |
//! | 3 | `test_undo_commit_round_trip` | undo → `git reset --soft <original>` restores original state |
//! | 4 | `test_plan_undo_commit_pushed_blocker` | upstream == HEAD → plan returns blocker |
//! | 5 | `test_plan_undo_commit_merge_commit_blocker` | merge commit HEAD → plan returns blocker |
//! | 6 | `test_plan_undo_commit_root_commit_blocker` | root commit (no parent) → plan returns blocker |
//! | 7 | `test_plan_undo_commit_detached_blocker` | detached HEAD → plan returns blocker |
//! | 8 | `test_undo_commit_no_upstream_allowed` | local branch without upstream → undo is allowed |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{execute_undo_commit, plan_undo_commit, working_tree_status};

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

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run `git status --porcelain` and return the raw output lines.
fn porcelain_status(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .expect("git status failed");
    let raw = String::from_utf8_lossy(&out.stdout).to_string();
    raw.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// A minimal local repo: one base commit on `main`, no remote.
struct LocalRepo {
    _tmp: TempDir,
    path: PathBuf,
}

fn setup_local() -> LocalRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "base.txt", "base content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    LocalRepo { _tmp: tmp, path }
}

/// A repo that has a bare remote and has pushed `main`.
struct RepoWithRemote {
    _tmp: TempDir,
    local: PathBuf,
    _remote: PathBuf,
}

fn setup_with_remote() -> RepoWithRemote {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");

    // bare remote — `-b main` prevents the default-branch issue in isolated env.
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
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    RepoWithRemote {
        _tmp: tmp,
        local,
        _remote: remote,
    }
}

// ────────────────────────────────────────────────────────────
// Test 1: happy path — HEAD moves to parent, changes are staged, WT unchanged
// ────────────────────────────────────────────────────────────

#[test]
fn test_undo_commit_normal() {
    let r = setup_local();

    // Add a second commit that we will undo.
    write_file(&r.path, "feature.txt", "new feature\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "add feature"]);

    let sha_before_undo = head_sha(&r.path);
    let parent_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(&r.path)
            .output()
            .expect("rev-parse HEAD~1 failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let repo = Repository::open(&r.path).unwrap();
    let outcome = execute_undo_commit(&repo).expect("undo should succeed");

    // HEAD moved to parent.
    assert_eq!(head_sha(&r.path), parent_sha, "HEAD must move to parent");

    // UndoOutcome carries the correct SHAs.
    assert_eq!(
        outcome.undone.0, sha_before_undo,
        "undone SHA must be the old HEAD"
    );
    assert_eq!(
        outcome.now_at.0, parent_sha,
        "now_at SHA must be the parent"
    );

    // Working tree: feature.txt still exists with the same content.
    assert_eq!(
        read_file(&r.path, "feature.txt"),
        "new feature\n",
        "WT file must be unchanged"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: undone commit changes are staged (INDEX side of status)
// ────────────────────────────────────────────────────────────

#[test]
fn test_undo_commit_staged_remain() {
    let r = setup_local();

    // Commit a new file.
    write_file(&r.path, "staged_check.txt", "will be staged after undo\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "commit to undo"]);

    let repo = Repository::open(&r.path).unwrap();
    execute_undo_commit(&repo).expect("undo should succeed");

    // Check porcelain status: staged_check.txt should show as "A " (added in index).
    let lines = porcelain_status(&r.path);
    assert!(
        !lines.is_empty(),
        "status must be non-empty after undo (changes are staged)"
    );

    let found = lines.iter().any(|l| {
        // Porcelain v1: first char is index status, second is WT status.
        // "A " means added in index, clean in WT.
        // "M " means modified in index, clean in WT.
        let index_char = l.chars().next().unwrap_or(' ');
        let path_part = l.get(3..).unwrap_or("");
        (index_char == 'A' || index_char == 'M') && path_part.contains("staged_check.txt")
    });
    assert!(
        found,
        "staged_check.txt must be staged (index-side) after undo, got: {:?}",
        lines
    );

    // Also verify through git2 WorkingTreeStatus.
    let repo2 = Repository::open(&r.path).unwrap();
    let st = working_tree_status(&repo2).unwrap();
    assert!(
        !st.staged.is_empty(),
        "git2 staged list must be non-empty after undo"
    );
    assert!(
        st.unstaged.is_empty(),
        "WT (unstaged) must be clean — undo must not touch WT"
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: round-trip — undo then `git reset --soft <original>` restores HEAD
// ────────────────────────────────────────────────────────────

#[test]
fn test_undo_commit_round_trip() {
    let r = setup_local();

    write_file(&r.path, "round.txt", "round trip\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "round-trip commit"]);

    let original_sha = head_sha(&r.path);

    let repo = Repository::open(&r.path).unwrap();
    let outcome = execute_undo_commit(&repo).expect("undo should succeed");

    // HEAD is now at parent.
    assert_ne!(head_sha(&r.path), original_sha);

    // Restore via `git reset --soft <original_sha>`.
    git(&r.path, &["reset", "--soft", &outcome.undone.0]);

    // HEAD must be back to the original commit.
    assert_eq!(
        head_sha(&r.path),
        original_sha,
        "after reset --soft, HEAD must be restored to original SHA"
    );

    // WT file still intact.
    assert_eq!(read_file(&r.path, "round.txt"), "round trip\n");
}

// ────────────────────────────────────────────────────────────
// Test 4: pushed commit → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_undo_commit_pushed_blocker() {
    let r = setup_with_remote();

    // The current HEAD has been pushed (upstream == HEAD).
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_undo_commit(&repo).expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "pushed commit must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("pushed") || msg.contains("upstream"),
        "blocker must mention push/upstream: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: merge commit HEAD → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_undo_commit_merge_commit_blocker() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(&path, "base.txt", "base\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "base"]);

    // Create a side branch and commit.
    git(&path, &["checkout", "-q", "-b", "side"]);
    write_file(&path, "side.txt", "side\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "side commit"]);

    // Merge into main — creates a merge commit.
    git(&path, &["checkout", "-q", "main"]);
    git(
        &path,
        &["merge", "--no-ff", "-m", "merge side into main", "side"],
    );

    // HEAD is now a merge commit.
    let repo = Repository::open(&path).unwrap();
    let plan = plan_undo_commit(&repo).expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "merge commit must be a blocker, got: {:?}",
        plan.blockers
    );
    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("merge"),
        "blocker must mention merge commit: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: root commit (no parent) → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_undo_commit_root_commit_blocker() {
    let r = setup_local();

    // HEAD is the very first commit (no parent).
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_undo_commit(&repo).expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "root commit must be a blocker, got: {:?}",
        plan.blockers
    );
    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("root") || msg.contains("no parent") || msg.contains("parent"),
        "blocker must mention root/no-parent: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: detached HEAD → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_undo_commit_detached_blocker() {
    let r = setup_local();

    // Add a second commit so detached HEAD has a real sha to point to.
    write_file(&r.path, "extra.txt", "extra\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "extra commit"]);

    // Detach HEAD by checking out by SHA.
    let sha = head_sha(&r.path);
    git(&r.path, &["checkout", "--detach", &sha]);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_undo_commit(&repo).expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "detached HEAD must be a blocker, got: {:?}",
        plan.blockers
    );
    let msg = plan.blockers.join(" ");
    assert!(
        msg.contains("detach") || msg.contains("branch"),
        "blocker must mention detached/branch: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 8: local branch without upstream → undo is allowed
// ────────────────────────────────────────────────────────────

#[test]
fn test_undo_commit_no_upstream_allowed() {
    let r = setup_local();

    // Add a second commit on a local-only branch (no remote, no upstream).
    git(&r.path, &["checkout", "-q", "-b", "local-only"]);
    write_file(&r.path, "local.txt", "local only\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "local only commit"]);

    let parent_sha = {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD~1"])
            .current_dir(&r.path)
            .output()
            .expect("rev-parse failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Plan must have no blockers.
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_undo_commit(&repo).expect("plan should succeed");
    assert!(
        plan.blockers.is_empty(),
        "local-only branch must not have blockers, got: {:?}",
        plan.blockers
    );

    // Execute must succeed.
    let repo2 = Repository::open(&r.path).unwrap();
    let outcome = execute_undo_commit(&repo2).expect("undo should succeed on local branch");
    assert_eq!(
        head_sha(&r.path),
        parent_sha,
        "HEAD must move to parent after undo"
    );
    assert_eq!(
        outcome.now_at.0, parent_sha,
        "UndoOutcome.now_at must be parent SHA"
    );
}
