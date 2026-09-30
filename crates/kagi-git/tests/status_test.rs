//! Integration tests for working tree status (T003).
//!
//! Each test builds a small Git repository inside a `tempfile::TempDir` using
//! the hermetic `git` CLI helpers in `support/git_fixture.rs`, then asserts the
//! result of `kagi_git::working_tree_status`.
//!
//! All writes are confined to the temporary directory; no existing repository
//! is ever modified.

use std::path::Path;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, write_file};

use git2::Repository;
use tempfile::TempDir;

// Re-use the public types from the library.
use kagi_git::{working_tree_status, ChangeKind};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Initialise a bare-minimum repo with a single "initial commit" so HEAD is
/// not unborn.
fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git_fixture::init_repo(dir, "main");

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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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

// ────────────────────────────────────────────────────────────
// Test: worktree_files (T-WS-EDITOR-004) — tracked + untracked, ignored excluded
// ────────────────────────────────────────────────────────────

#[test]
fn test_worktree_files_tracked_untracked_ignored() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let _repo = init_repo(&tmp); // creates + commits base.txt
    let dir = tmp.path();

    // Untracked file.
    write_file(dir, "untracked.txt", "untracked\n");
    // Ignored file (+ .gitignore itself becomes a new tracked-able file, but
    // we don't add it — it stays untracked, which is fine, it's not ignored).
    write_file(dir, ".gitignore", "ignored.txt\n");
    write_file(dir, "ignored.txt", "should not appear\n");

    let backend = kagi_git::Backend::open(dir).expect("open failed");
    let files = backend.worktree_files().expect("worktree_files failed");

    assert!(
        files.contains(&Path::new("base.txt").to_path_buf()),
        "expected tracked base.txt in {:?}",
        files
    );
    assert!(
        files.contains(&Path::new("untracked.txt").to_path_buf()),
        "expected untracked.txt in {:?}",
        files
    );
    assert!(
        !files.contains(&Path::new("ignored.txt").to_path_buf()),
        "ignored.txt must be excluded from {:?}",
        files
    );
    // Sorted.
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted, "worktree_files must be sorted");
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
