//! Integration tests for `Backend::file_content_at_commit` (editor mode
//! History → Snapshot tab).
//!
//! Each test builds a small Git repository in a `tempfile::TempDir` using the
//! `git` CLI, then asserts the result of `Backend::file_content_at_commit`.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use kagi_git::{commit_log, Backend, CommitId};

// ────────────────────────────────────────────────────────────
// Test helpers
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

/// Initialise a repo and make an initial commit, return its Backend.
fn init_repo(tmp: &TempDir) -> Backend {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "base.txt", "line one\nline two\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Backend::open(dir).expect("failed to open repo")
}

/// Return the HEAD commit id by reading the repo log.
fn head_commit_id(backend: &Backend) -> CommitId {
    let repo = git2::Repository::open(backend.path()).expect("reopen for log");
    let commits = commit_log(&repo, 1).expect("commit_log failed");
    commits.into_iter().next().expect("no commits in repo").id
}

#[test]
fn returns_content_at_the_requested_commit() {
    let tmp = TempDir::new().unwrap();
    let backend = init_repo(&tmp);
    let dir = tmp.path();
    let first = head_commit_id(&backend);

    write_file(dir, "base.txt", "line one\nline two\nline three\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "append line three"]);

    let at_first = backend
        .file_content_at_commit(&first, Path::new("base.txt"))
        .expect("file_content_at_commit failed")
        .expect("path should exist at first commit");
    assert_eq!(at_first.content.as_deref(), Some("line one\nline two\n"));
    assert!(!at_first.is_binary);

    let second = head_commit_id(&backend);
    let at_second = backend
        .file_content_at_commit(&second, Path::new("base.txt"))
        .expect("file_content_at_commit failed")
        .expect("path should exist at second commit");
    assert_eq!(
        at_second.content.as_deref(),
        Some("line one\nline two\nline three\n")
    );
}

#[test]
fn follows_a_rename_at_the_old_path_before_the_rename() {
    let tmp = TempDir::new().unwrap();
    let backend = init_repo(&tmp);
    let dir = tmp.path();
    let before_rename = head_commit_id(&backend);

    git(dir, &["mv", "base.txt", "renamed.txt"]);
    git(dir, &["commit", "-m", "rename base.txt to renamed.txt"]);

    // The old path still resolves at the commit before the rename.
    let old_path_old_commit = backend
        .file_content_at_commit(&before_rename, Path::new("base.txt"))
        .expect("file_content_at_commit failed");
    assert!(old_path_old_commit.is_some());

    // But not at the new (post-rename) commit — no rename-following without
    // a real path (that's what the git-log-based FileHistoryEntry list is
    // for); a plain tree lookup at the old path is simply absent.
    let second = head_commit_id(&backend);
    let old_path_new_commit = backend
        .file_content_at_commit(&second, Path::new("base.txt"))
        .expect("file_content_at_commit failed");
    assert!(old_path_new_commit.is_none());
}

#[test]
fn path_absent_at_commit_returns_none() {
    let tmp = TempDir::new().unwrap();
    let backend = init_repo(&tmp);
    let first = head_commit_id(&backend);

    let missing = backend
        .file_content_at_commit(&first, Path::new("never-existed.txt"))
        .expect("file_content_at_commit failed");
    assert!(missing.is_none());
}

#[test]
fn binary_blob_reports_is_binary_with_no_content() {
    let tmp = TempDir::new().unwrap();
    let backend = init_repo(&tmp);
    let dir = tmp.path();

    std::fs::write(dir.join("blob.bin"), [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
    git(dir, &["add", "blob.bin"]);
    git(dir, &["commit", "-m", "add binary file"]);

    let id = head_commit_id(&backend);
    let snapshot = backend
        .file_content_at_commit(&id, Path::new("blob.bin"))
        .expect("file_content_at_commit failed")
        .expect("path should exist");
    assert!(snapshot.is_binary);
    assert!(snapshot.content.is_none());
}
