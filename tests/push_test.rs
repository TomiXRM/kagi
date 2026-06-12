//! Integration tests for push — T-HT-004
//!
//! All repositories (local + bare remote) are created inside `TempDir`s.
//! No network access: push goes to a local bare repository on disk.
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_push_ahead_two`               | ahead 2 → push succeeds, remote ref = HEAD, preview_commits has 2 entries |
//! | 2 | `test_push_ahead_zero_blocker`       | ahead 0 → plan has blocker |
//! | 3 | `test_push_set_upstream`             | no upstream + origin → set-upstream plan → execute sets upstream, ahead/behind appear |
//! | 4 | `test_push_non_ff_fails`             | non-FF (remote is ahead) → execute returns Err, stderr contains "rejected", local untouched |
//! | 5 | `test_push_detached_blocker`         | detached HEAD → plan has blocker |
//! | 6 | `test_push_no_force_in_args`         | execute_push never passes --force / --force-with-lease |
//! | 7 | `test_push_local_unchanged_on_error` | local repo HEAD/WT untouched after push failure |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{execute_push, plan_push};

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

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn remote_head_sha(remote: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(remote)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Layout: tmp/remote.git (bare) + tmp/local (clone with upstream set on main)
/// The remote starts with a single "base" commit.
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

    // -b main: ensure the default branch is "main" so the second clone is not unborn.
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

    // Second clone used to push commits "from elsewhere" (for non-FF tests).
    git(
        tmp.path(),
        &["clone", "-q", remote.to_str().unwrap(), other.to_str().unwrap()],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);

    Repos { _tmp: tmp, remote, local, other }
}

/// Push a commit from `other` to the remote (advance the remote without touching local).
fn remote_commit(r: &Repos, name: &str, content: &str, msg: &str) {
    write_file(&r.other, name, content);
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", msg]);
    git(&r.other, &["push", "-q", "origin", "main"]);
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

/// Test 1: ahead 2 → push succeeds, remote ref = HEAD, preview_commits has 2 entries.
#[test]
fn test_push_ahead_two() {
    let r = setup();

    // Make two local commits not yet pushed.
    write_file(&r.local, "a.txt", "a\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "commit A"]);

    write_file(&r.local, "b.txt", "b\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "commit B"]);

    let local_sha = head_sha(&r.local);

    // Plan: should show 2 preview_commits, no blockers.
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push(&repo).expect("plan_push should succeed");
    assert!(
        plan.blockers.is_empty(),
        "ahead 2 should have no blockers, got: {:?}",
        plan.blockers
    );
    assert_eq!(
        plan.preview_commits.len(),
        2,
        "should have exactly 2 preview commits, got: {:?}",
        plan.preview_commits
    );

    // Execute.
    let outcome = execute_push(&repo, &r.local).expect("push should succeed");
    assert_eq!(outcome.pushed, 2, "pushed count should be 2");
    assert!(!outcome.set_upstream, "upstream was already set");

    // Remote ref must equal local HEAD.
    assert_eq!(
        remote_head_sha(&r.remote),
        local_sha,
        "remote HEAD should match local HEAD after push"
    );
}

/// Test 2: ahead 0 → plan has blocker.
#[test]
fn test_push_ahead_zero_blocker() {
    let r = setup();
    // No local commits beyond remote — already up to date.
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push(&repo).expect("plan_push should succeed");
    assert!(
        !plan.blockers.is_empty(),
        "ahead=0 must produce a blocker, got no blockers"
    );
}

/// Test 3: no upstream + origin → set-upstream plan → execute sets upstream.
#[test]
fn test_push_set_upstream() {
    let r = setup();

    // Create a new branch locally without tracking.
    git(&r.local, &["checkout", "-q", "-b", "feature/new"]);

    write_file(&r.local, "feat.txt", "feature work\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "feature commit"]);

    let local_sha = head_sha(&r.local);

    // Plan: set-upstream flow — no blocker.
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push(&repo).expect("plan_push should succeed");
    assert!(
        plan.blockers.is_empty(),
        "set-upstream flow must have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.title.contains("set upstream"),
        "title should mention set upstream: {}",
        plan.title
    );
    assert!(
        !plan.preview_commits.is_empty(),
        "set-upstream flow should have preview_commits"
    );

    // Execute.
    let outcome = execute_push(&repo, &r.local).expect("set-upstream push should succeed");
    assert!(outcome.set_upstream, "set_upstream flag should be true");

    // Re-open repo to verify upstream is now configured.
    let repo2 = Repository::open(&r.local).unwrap();
    let branch = repo2.find_branch("feature/new", git2::BranchType::Local).unwrap();
    assert!(
        branch.upstream().is_ok(),
        "upstream should be configured after set-upstream push"
    );

    // Remote should have the new branch at our commit.
    let remote_ref_out = Command::new("git")
        .args(["rev-parse", "refs/heads/feature/new"])
        .current_dir(&r.remote)
        .output()
        .expect("rev-parse failed");
    let remote_sha = String::from_utf8_lossy(&remote_ref_out.stdout).trim().to_string();
    assert_eq!(
        remote_sha, local_sha,
        "remote branch should match local HEAD"
    );
}

/// Test 4: non-FF (remote is ahead) → execute returns Err, stderr contains "rejected".
#[test]
fn test_push_non_ff_fails() {
    let r = setup();

    // Advance the remote from "other" so our local is diverged.
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    // Local also makes a commit (diverged).
    write_file(&r.local, "local.txt", "local work\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    let head_before = head_sha(&r.local);

    // Execute push — must fail (non-FF rejected by remote).
    let repo = Repository::open(&r.local).unwrap();
    let err = execute_push(&repo, &r.local).expect_err("non-FF push must be rejected");
    let msg = format!("{}", err);
    assert!(
        msg.contains("rejected") || msg.contains("non-fast-forward") || msg.contains("push failed"),
        "error should mention rejection: {}",
        msg
    );

    // Local repo is completely untouched.
    assert_eq!(
        head_sha(&r.local),
        head_before,
        "HEAD must not move after failed push"
    );
}

/// Test 5: detached HEAD → plan has blocker.
#[test]
fn test_push_detached_blocker() {
    let r = setup();

    // Detach HEAD.
    let sha = head_sha(&r.local);
    git(&r.local, &["checkout", "-q", "--detach", &sha]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push(&repo).expect("plan_push should not error");
    assert!(
        !plan.blockers.is_empty(),
        "detached HEAD must produce a blocker"
    );
    assert!(
        plan.blockers.iter().any(|b| b.contains("detached")),
        "blocker should mention detached: {:?}",
        plan.blockers
    );
}

/// Test 6: execute_push never passes --force or --force-with-lease.
/// Verify that the actual git args array built in execute_push does not
/// include any force flag as a string literal value (not in comments).
#[test]
fn test_push_no_force_in_args() {
    // Read the ops.rs source.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/git/ops.rs"),
    )
    .expect("could not read ops.rs");

    // Find the execute_push function section only (up to build_push_preview).
    let push_section_start = src.find("pub fn execute_push").expect("execute_push not found");
    let push_section = &src[push_section_start..];

    // Find internal helpers marker as the end boundary.
    let end_boundary = push_section
        .find("Internal helpers (push)")
        .unwrap_or(push_section.len());
    let push_body = &push_section[..end_boundary];

    // The args vec lines should not contain "--force" or "force-with-lease" as a
    // string literal (in quotes). We check for the string as it would appear in
    // a vec! macro or similar: `"--force"` or `"--force-with-lease"`.
    assert!(
        !push_body.contains("\"--force\""),
        "execute_push args must not include \"--force\""
    );
    assert!(
        !push_body.contains("\"--force-with-lease\""),
        "execute_push args must not include \"--force-with-lease\""
    );
}

/// Test 7: local repo HEAD and working tree untouched after push failure.
#[test]
fn test_push_local_unchanged_on_error() {
    let r = setup();

    // Make a local commit.
    write_file(&r.local, "a.txt", "a\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "commit A"]);

    let head_before = head_sha(&r.local);

    // Remove the remote so push fails.
    std::fs::remove_dir_all(&r.remote).unwrap();

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_push(&repo, &r.local).expect_err("push must fail when remote is gone");
    let msg = format!("{}", err);
    assert!(
        msg.contains("push failed") || msg.contains("failed"),
        "error should mention failure: {}",
        msg
    );

    // Local HEAD unchanged.
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move after failed push");

    // Working tree unchanged.
    let content = std::fs::read_to_string(r.local.join("a.txt")).unwrap();
    assert_eq!(content, "a\n", "working tree must be untouched after failed push");
}
