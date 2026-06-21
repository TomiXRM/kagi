//! Integration tests for working tree status (T003).
//!
//! Each test builds a small Git repository inside a `tempfile::TempDir` using
//! `std::process::Command` to call the `git` CLI, then asserts the result of
//! `kagi_git::working_tree_status`.
//!
//! All writes are confined to the temporary directory; no existing repository
//! is ever modified.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

// Re-use the public types from the library.
use kagi_git::{working_tree_status, ChangeKind};

// ────────────────────────────────────────────────────────────
// Helpers
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
        .env("HOME", dir) // avoids picking up ~/.gitconfig gpg settings
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

/// Initialise a bare-minimum repo with a single "initial commit" so HEAD is
/// not unborn.
fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    // Create a base commit so the index is not unborn.
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Repository::open(dir).expect("failed to open repo")
}

// ────────────────────────────────────────────────────────────
// Test: clean working tree
// ────────────────────────────────────────────────────────────

#[test]
fn test_clean_repo() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);

    let status = working_tree_status(&repo).expect("status failed");

    assert!(
        !status.is_dirty(),
        "expected clean repo but got dirty: {:?}",
        status
    );
    assert!(status.staged.is_empty(), "staged should be empty");
    assert!(status.unstaged.is_empty(), "unstaged should be empty");
    assert!(status.untracked.is_empty(), "untracked should be empty");
    assert!(status.conflicted.is_empty(), "conflicted should be empty");
}

// ────────────────────────────────────────────────────────────
// Test: staged file (Added)
// ────────────────────────────────────────────────────────────

#[test]
fn test_staged_added() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Stage a new file without committing.
    write_file(dir, "staged.txt", "hello\n");
    git(dir, &["add", "staged.txt"]);

    let status = working_tree_status(&repo).expect("status failed");

    assert!(status.is_dirty(), "repo should be dirty");

    let staged_paths: Vec<_> = status.staged.iter().map(|f| f.path.as_path()).collect();
    let staged_path = Path::new("staged.txt");
    assert!(
        staged_paths.contains(&staged_path),
        "expected staged.txt in staged, got: {:?}",
        staged_paths
    );

    let staged_file = status
        .staged
        .iter()
        .find(|f| f.path == staged_path)
        .unwrap();
    assert_eq!(
        staged_file.change,
        ChangeKind::Added,
        "change kind should be Added"
    );
}

// ────────────────────────────────────────────────────────────
// Test: unstaged modification
// ────────────────────────────────────────────────────────────

#[test]
fn test_unstaged_modified() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Modify tracked file without staging.
    write_file(dir, "base.txt", "modified content\n");

    let status = working_tree_status(&repo).expect("status failed");

    assert!(status.is_dirty(), "repo should be dirty");
    assert!(status.staged.is_empty(), "staged should be empty");

    let unstaged_paths: Vec<_> = status.unstaged.iter().map(|f| f.path.as_path()).collect();
    let expected = Path::new("base.txt");
    assert!(
        unstaged_paths.contains(&expected),
        "expected base.txt in unstaged, got: {:?}",
        unstaged_paths
    );

    let unstaged_file = status.unstaged.iter().find(|f| f.path == expected).unwrap();
    assert_eq!(
        unstaged_file.change,
        ChangeKind::Modified,
        "change kind should be Modified"
    );
}

// ────────────────────────────────────────────────────────────
// Test: untracked file
// ────────────────────────────────────────────────────────────

#[test]
fn test_untracked() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Write a new file but do NOT add it.
    write_file(dir, "untracked.txt", "I am not tracked\n");

    let status = working_tree_status(&repo).expect("status failed");

    assert!(status.is_dirty(), "repo should be dirty");
    assert!(status.staged.is_empty(), "staged should be empty");
    assert!(status.unstaged.is_empty(), "unstaged should be empty");

    let untracked_paths: Vec<_> = status.untracked.iter().map(|p| p.as_path()).collect();
    let expected = Path::new("untracked.txt");
    assert!(
        untracked_paths.contains(&expected),
        "expected untracked.txt in untracked, got: {:?}",
        untracked_paths
    );
}

// ────────────────────────────────────────────────────────────
// Test: combination — staged + unstaged + untracked
// ────────────────────────────────────────────────────────────

#[test]
fn test_combination_staged_unstaged_untracked() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Add a second tracked file so we can modify it without staging.
    write_file(dir, "tracked.txt", "original\n");
    git(dir, &["add", "tracked.txt"]);
    git(dir, &["commit", "-m", "add tracked.txt"]);

    // 1. Stage a new file.
    write_file(dir, "new_staged.txt", "staged\n");
    git(dir, &["add", "new_staged.txt"]);

    // 2. Modify a tracked file without staging (unstaged).
    write_file(dir, "tracked.txt", "modified\n");

    // 3. Drop a completely new file (untracked).
    write_file(dir, "untracked.txt", "untracked\n");

    let status = working_tree_status(&repo).expect("status failed");

    assert!(status.is_dirty());

    // Staged check
    let staged_paths: Vec<_> = status.staged.iter().map(|f| f.path.as_path()).collect();
    assert!(
        staged_paths.contains(&Path::new("new_staged.txt")),
        "new_staged.txt missing from staged: {:?}",
        staged_paths
    );

    // Unstaged check
    let unstaged_paths: Vec<_> = status.unstaged.iter().map(|f| f.path.as_path()).collect();
    assert!(
        unstaged_paths.contains(&Path::new("tracked.txt")),
        "tracked.txt missing from unstaged: {:?}",
        unstaged_paths
    );

    // Untracked check
    let untracked_paths: Vec<_> = status.untracked.iter().map(|p| p.as_path()).collect();
    assert!(
        untracked_paths.contains(&Path::new("untracked.txt")),
        "untracked.txt missing from untracked: {:?}",
        untracked_paths
    );
}

// ────────────────────────────────────────────────────────────
// Test: staged deletion
// ────────────────────────────────────────────────────────────

#[test]
fn test_staged_deleted() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Stage deletion of the base file.
    git(dir, &["rm", "base.txt"]);

    let status = working_tree_status(&repo).expect("status failed");

    assert!(status.is_dirty());
    let staged_paths: Vec<_> = status.staged.iter().map(|f| f.path.as_path()).collect();
    assert!(
        staged_paths.contains(&Path::new("base.txt")),
        "base.txt missing from staged: {:?}",
        staged_paths
    );
    let staged_file = status
        .staged
        .iter()
        .find(|f| f.path == Path::new("base.txt"))
        .unwrap();
    assert_eq!(staged_file.change, ChangeKind::Deleted);
}

// ────────────────────────────────────────────────────────────
// Note on `conflicted` test
// ────────────────────────────────────────────────────────────
//
// A conflicted-state test is NOT included here because constructing a merge
// conflict programmatically requires:
//   1. Two diverging branches that modify the same line,
//   2. `git merge --no-commit` (or equivalent) to leave the conflict in place,
//   3. Ensuring git2 sees CONFLICTED bits (not just WT_MODIFIED).
//
// This is feasible but adds significant setup complexity for MVP.  The
// `conflicted` field is exercised by the domain model and is wire-correct
// (uses `Status::CONFLICTED` bit); a dedicated conflict test can be added in a
// follow-up ticket.
