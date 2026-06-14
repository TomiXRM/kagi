//! Integration tests for commit log (T004).
//!
//! Each test builds a small Git repository inside a `tempfile::TempDir` using
//! `std::process::Command` to call the `git` CLI, then asserts the result of
//! `kagi::git::commit_log`.
//!
//! All writes are confined to the temporary directory; no existing repository
//! is ever modified.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{commit_log, CommitId};

// ────────────────────────────────────────────────────────────
// Helpers (mirrors status_test.rs by design; shared helper
// extraction is intentionally deferred per T004 scope)
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

/// Capture stdout of a git command; panics on failure.
fn git_output(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("non-UTF-8 git output")
}

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Initialise a bare-minimum repo with a single "initial commit".
fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Repository::open(dir).expect("failed to open repo")
}

// ────────────────────────────────────────────────────────────
// Build a branching + merge repo:
//
//   A  (initial commit)
//   B  (add b.txt)   ← main
//   |\
//   | C  (feature work)  ← feature/x
//   | D  (feature more)
//   |/
//   E  (merge feature/x into main)
//
// Expected topo order: E D C B A  (or E C D B A — both are valid topo orders)
// Only invariant: for every commit its parents appear later in the list.
// ────────────────────────────────────────────────────────────

fn build_branching_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    // A
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    // B
    write_file(dir, "b.txt", "b\n");
    git(dir, &["add", "b.txt"]);
    git(dir, &["commit", "-m", "add b.txt"]);

    // branch feature/x from B
    git(dir, &["checkout", "-b", "feature/x"]);

    // C
    write_file(dir, "fx.txt", "fx\n");
    git(dir, &["add", "fx.txt"]);
    git(dir, &["commit", "-m", "feature work"]);

    // D
    write_file(dir, "fx.txt", "fx2\n");
    git(dir, &["add", "fx.txt"]);
    git(dir, &["commit", "-m", "feature more"]);

    // back to main, add one more commit then merge
    git(dir, &["checkout", "main"]);

    // E: merge commit
    git(
        dir,
        &["merge", "--no-ff", "feature/x", "-m", "merge feature/x"],
    );

    Repository::open(dir).expect("failed to open repo")
}

// ────────────────────────────────────────────────────────────
// Test: unborn repo returns empty Vec (no crash)
// ────────────────────────────────────────────────────────────

#[test]
fn test_unborn_repo_is_empty() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    let repo = Repository::open(dir).expect("failed to open repo");
    let commits = commit_log(&repo, 10_000).expect("commit_log must not fail on unborn repo");

    assert!(
        commits.is_empty(),
        "expected 0 commits for unborn repo, got {}",
        commits.len()
    );
}

// ────────────────────────────────────────────────────────────
// Test: all commits are returned
// ────────────────────────────────────────────────────────────

#[test]
fn test_all_commits_returned() {
    let tmp = TempDir::new().unwrap();
    let repo = build_branching_repo(&tmp);

    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    // A(1) + B(1) + C(1) + D(1) + E(merge, 1) = 5
    assert_eq!(
        commits.len(),
        5,
        "expected 5 commits, got {}: {:?}",
        commits.len(),
        commits.iter().map(|c| &c.summary).collect::<Vec<_>>()
    );
}

// ────────────────────────────────────────────────────────────
// Test: topological order — every parent appears after its child
// ────────────────────────────────────────────────────────────

#[test]
fn test_topological_order() {
    let tmp = TempDir::new().unwrap();
    let repo = build_branching_repo(&tmp);

    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    // Build a position map: CommitId → index in the returned Vec.
    let pos: HashMap<&CommitId, usize> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| (&c.id, i))
        .collect();

    for c in &commits {
        for parent_id in &c.parents {
            let child_pos = pos[&c.id];
            let parent_pos = pos.get(parent_id).copied();
            match parent_pos {
                Some(pp) => assert!(
                    child_pos < pp,
                    "topo order violated: commit '{}' (pos {}) must come before parent '{}' (pos {})",
                    c.summary,
                    child_pos,
                    parent_id.short(),
                    pp
                ),
                None => panic!(
                    "parent {} of commit '{}' not found in result",
                    parent_id.short(),
                    c.summary
                ),
            }
        }
    }
}

// ────────────────────────────────────────────────────────────
// Test: merge commit has 2 parents, parents[0] is first parent
// ────────────────────────────────────────────────────────────

#[test]
fn test_merge_commit_parents() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let repo = build_branching_repo(tmp.borrow());

    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    // Find the merge commit (summary starts with "merge feature/x")
    let merge = commits
        .iter()
        .find(|c| c.summary.starts_with("merge feature/x"))
        .expect("merge commit not found");

    assert_eq!(
        merge.parents.len(),
        2,
        "merge commit should have 2 parents, got {}",
        merge.parents.len()
    );

    // Verify parents[0] against `git log --pretty=%P -1 <sha>`
    let git_parents_line = git_output(dir, &["log", "--pretty=%P", "-1", merge.id.0.as_str()]);
    let git_parents: Vec<&str> = git_parents_line.split_whitespace().collect();
    assert_eq!(
        git_parents.len(),
        2,
        "git log should show 2 parents for the merge commit"
    );

    assert!(
        merge.parents[0].0.starts_with(git_parents[0]),
        "parents[0] mismatch: our={} git={}",
        merge.parents[0].short(),
        &git_parents[0][..8.min(git_parents[0].len())]
    );
    assert!(
        merge.parents[1].0.starts_with(git_parents[1]),
        "parents[1] mismatch: our={} git={}",
        merge.parents[1].short(),
        &git_parents[1][..8.min(git_parents[1].len())]
    );
}

// ────────────────────────────────────────────────────────────
// Test: limit parameter is respected
// ────────────────────────────────────────────────────────────

#[test]
fn test_limit() {
    let tmp = TempDir::new().unwrap();
    let repo = build_branching_repo(&tmp);

    let commits_3 = commit_log(&repo, 3).expect("commit_log failed");
    assert_eq!(
        commits_3.len(),
        3,
        "expected exactly 3 commits with limit=3, got {}",
        commits_3.len()
    );

    let commits_1 = commit_log(&repo, 1).expect("commit_log failed");
    assert_eq!(
        commits_1.len(),
        1,
        "expected exactly 1 commit with limit=1, got {}",
        commits_1.len()
    );

    let commits_0 = commit_log(&repo, 0).expect("commit_log failed");
    assert_eq!(
        commits_0.len(),
        0,
        "expected 0 commits with limit=0, got {}",
        commits_0.len()
    );
}

// ────────────────────────────────────────────────────────────
// Test: summary = first line of message; multi-line message
// ────────────────────────────────────────────────────────────

#[test]
fn test_summary_and_multiline_message() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Commit with a multi-line message.
    write_file(dir, "extra.txt", "extra\n");
    git(dir, &["add", "extra.txt"]);

    // Use -m twice to produce a multi-paragraph message.
    let status = Command::new("git")
        .args([
            "commit",
            "-m",
            "short summary line",
            "-m",
            "Second paragraph of\nthe commit message.",
        ])
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git commit failed to start");
    assert!(status.success(), "git commit returned non-zero");

    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    let multiline = commits
        .iter()
        .find(|c| c.summary == "short summary line")
        .expect("multi-line commit not found");

    // summary must equal the first line only.
    assert_eq!(
        multiline.summary, "short summary line",
        "summary mismatch: {:?}",
        multiline.summary
    );

    // full message must contain both lines.
    assert!(
        multiline.message.contains("Second paragraph"),
        "message should contain second paragraph: {:?}",
        multiline.message
    );
}

// ────────────────────────────────────────────────────────────
// Test: commits reachable only from non-HEAD refs are included
// ────────────────────────────────────────────────────────────

#[test]
fn test_all_refs_included() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    // Commit on main.
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "commit on main"]);

    // Create an orphan branch (unrelated history).
    git(dir, &["checkout", "--orphan", "orphan"]);
    git(dir, &["rm", "-rf", "--quiet", "."]);
    write_file(dir, "orphan.txt", "orphan\n");
    git(dir, &["add", "orphan.txt"]);
    git(dir, &["commit", "-m", "orphan commit"]);

    // Return to main.
    git(dir, &["checkout", "main"]);

    let repo = Repository::open(dir).expect("failed to open repo");
    let commits = commit_log(&repo, 10_000).expect("commit_log failed");

    // Both the main commit and the orphan commit must be present.
    assert_eq!(
        commits.len(),
        2,
        "expected 2 commits (main + orphan), got {}: {:?}",
        commits.len(),
        commits.iter().map(|c| &c.summary).collect::<Vec<_>>()
    );
}

// ────────────────────────────────────────────────────────────
// Helper: allow tmp.borrow() syntax in build_branching_repo call
// ────────────────────────────────────────────────────────────
trait Borrow {
    fn borrow(&self) -> &Self;
}
impl Borrow for TempDir {
    fn borrow(&self) -> &TempDir {
        self
    }
}
