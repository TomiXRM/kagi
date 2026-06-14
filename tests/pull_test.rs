//! Integration tests for pull — T-HT-003
//!
//! All repositories (local + bare remote) are created inside `TempDir`s.
//! No network access: fetch goes to a local bare repository on disk.
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_pull_fast_forward` | behind-only → FastForward, WT updated |
//! | 2 | `test_pull_merge_clean` | diverged without conflict → Merged (2 parents), WT has both changes |
//! | 3 | `test_pull_conflict_leaves_repo_untouched` | diverged with conflict → Err, HEAD/WT/index/state all untouched |
//! | 4 | `test_pull_up_to_date` | equal tips → UpToDate |
//! | 5 | `test_pull_ahead_only_up_to_date` | local ahead of upstream → UpToDate (nothing to merge) |
//! | 6 | `test_plan_pull_dirty_blocker` | dirty WT → blocker |
//! | 7 | `test_plan_pull_no_upstream_blocker` | branch without upstream → blocker |
//! | 8 | `test_pull_fetch_failure_untouched` | remote gone → Err mentions fetch, repo untouched |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{execute_pull, plan_pull, working_tree_status, PullOutcome};

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

/// Layout: tmp/remote.git (bare) + tmp/local (clone-ish with upstream set)
/// plus tmp/other (second working clone used to advance the remote).
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

    // -b main: under the isolated test env the default branch would be
    // "master", leaving the bare HEAD dangling and the second clone unborn.
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

    // Second clone used to push commits "from elsewhere".
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

/// Commit `content` into `name` in the `other` clone and push to the remote.
fn remote_commit(r: &Repos, name: &str, content: &str, msg: &str) {
    write_file(&r.other, name, content);
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", msg]);
    git(&r.other, &["push", "-q", "origin", "main"]);
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[test]
fn test_pull_fast_forward() {
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");

    match outcome {
        PullOutcome::FastForward { .. } => {}
        other => panic!("expected FastForward, got {:?}", other),
    }
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    // Clean status after FF.
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(!st.is_dirty(), "WT must be clean after FF");
}

#[test]
fn test_pull_merge_clean() {
    let r = setup();
    remote_commit(&r, "remote.txt", "from remote\n", "remote work");

    // Diverge locally with a different file.
    write_file(&r.local, "local.txt", "from local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should merge cleanly");

    let commit_id = match outcome {
        PullOutcome::Merged { commit } => commit,
        other => panic!("expected Merged, got {:?}", other),
    };

    // Merge commit has two parents.
    let repo = Repository::open(&r.local).unwrap();
    let oid = git2::Oid::from_str(&commit_id.0).unwrap();
    let merge_commit = repo.find_commit(oid).unwrap();
    assert_eq!(merge_commit.parent_count(), 2);

    // Both sides' files exist; WT clean.
    assert_eq!(read_file(&r.local, "remote.txt"), "from remote\n");
    assert_eq!(read_file(&r.local, "local.txt"), "from local\n");
    let st = working_tree_status(&repo).unwrap();
    assert!(!st.is_dirty(), "WT must be clean after merge");
}

#[test]
fn test_pull_conflict_leaves_repo_untouched() {
    let r = setup();
    // Both sides edit the same line of base.txt.
    remote_commit(&r, "base.txt", "remote version\n", "remote edit");

    write_file(&r.local, "base.txt", "local version\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local edit"]);

    let head_before = head_sha(&r.local);

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must fail on conflict");
    let msg = format!("{}", err);
    assert!(
        msg.contains("conflict"),
        "error should mention conflict: {}",
        msg
    );
    assert!(
        msg.contains("base.txt"),
        "error should name the file: {}",
        msg
    );

    // Repo completely untouched:
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    assert_eq!(
        read_file(&r.local, "base.txt"),
        "local version\n",
        "WT must be untouched"
    );
    let repo = Repository::open(&r.local).unwrap();
    assert_eq!(
        repo.state(),
        git2::RepositoryState::Clean,
        "no MERGING state"
    );
    let st = working_tree_status(&repo).unwrap();
    assert!(!st.is_dirty(), "index/WT must stay clean");
}

#[test]
fn test_pull_up_to_date() {
    let r = setup();
    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert_eq!(outcome, PullOutcome::UpToDate);
}

#[test]
fn test_pull_ahead_only_up_to_date() {
    let r = setup();
    // Local ahead of upstream.
    write_file(&r.local, "ahead.txt", "ahead\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "ahead work"]);

    let head_before = head_sha(&r.local);
    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert_eq!(outcome, PullOutcome::UpToDate);
    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
}

#[test]
fn test_plan_pull_dirty_blocker() {
    let r = setup();
    write_file(&r.local, "base.txt", "dirty\n");

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");
    assert!(
        !plan.blockers.is_empty(),
        "dirty WT must be a blocker, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_plan_pull_no_upstream_blocker() {
    let r = setup();
    git(&r.local, &["checkout", "-q", "-b", "no-upstream-branch"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull(&repo).expect("plan should succeed");
    assert!(
        !plan.blockers.is_empty(),
        "missing upstream must be a blocker, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_pull_fetch_failure_untouched() {
    let r = setup();
    let head_before = head_sha(&r.local);

    // Make fetch fail by removing the remote repository.
    std::fs::remove_dir_all(&r.remote).unwrap();

    let repo = Repository::open(&r.local).unwrap();
    let err = execute_pull(&repo, &r.local).expect_err("pull must fail when remote is gone");
    let msg = format!("{}", err);
    assert!(msg.contains("fetch"), "error should mention fetch: {}", msg);

    assert_eq!(head_sha(&r.local), head_before, "HEAD must not move");
    let repo = Repository::open(&r.local).unwrap();
    assert_eq!(repo.state(), git2::RepositoryState::Clean);
}

#[test]
fn test_pull_ff_updates_modified_existing_file() {
    // Regression: FF must update files that EXIST locally but were modified
    // upstream (not just create new files).
    let r = setup();
    remote_commit(&r, "base.txt", "updated upstream\n", "edit base");

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should succeed");
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));

    assert_eq!(read_file(&r.local, "base.txt"), "updated upstream\n");
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(
        !st.is_dirty(),
        "WT must be clean after FF over modified file"
    );
}

#[test]
fn test_pull_merge_updates_modified_existing_file() {
    // Regression: merge must update an EXISTING file modified upstream while
    // the local side changed a different file.
    let r = setup();
    remote_commit(&r, "base.txt", "upstream edit\n", "remote edits base");

    write_file(&r.local, "local.txt", "local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local work"]);

    let repo = Repository::open(&r.local).unwrap();
    let outcome = execute_pull(&repo, &r.local).expect("pull should merge");
    assert!(matches!(outcome, PullOutcome::Merged { .. }));

    assert_eq!(read_file(&r.local, "base.txt"), "upstream edit\n");
    assert_eq!(read_file(&r.local, "local.txt"), "local\n");
    let st = working_tree_status(&Repository::open(&r.local).unwrap()).unwrap();
    assert!(
        !st.is_dirty(),
        "WT must be clean after merge over modified file"
    );
}
