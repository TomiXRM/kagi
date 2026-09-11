//! #677: a partial clone's absent object is explained, not reported raw.
//!
//! libgit2 has no promisor support, so in a `--filter=blob:none` clone it cannot
//! fetch an object on demand where `git` would. The raw message is
//! `object not found - cannot read header for (<oid>)`, which tells the user
//! nothing about why an object is missing from their own repository.

use kagi_git::{commit_changed_files, commit_file_diff, CommitId, GitError};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .expect("git runs")
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// An origin with two commits, and a blobless clone of it.
fn partial_clone() -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let origin = dir.path().join("origin");
    std::fs::create_dir_all(&origin).unwrap();
    git_ok(&origin, &["init", "-q", "-b", "main"]);
    git_ok(&origin, &["config", "user.email", "t@example.com"]);
    git_ok(&origin, &["config", "user.name", "t"]);
    std::fs::write(origin.join("f.txt"), "first\n").unwrap();
    git_ok(&origin, &["add", "-A"]);
    git_ok(&origin, &["commit", "-qm", "first"]);
    std::fs::write(origin.join("f.txt"), "second\n").unwrap();
    git_ok(&origin, &["add", "-A"]);
    git_ok(&origin, &["commit", "-qm", "second"]);
    git_ok(&origin, &["config", "uploadpack.allowFilter", "true"]);

    let consumer = dir.path().join("consumer");
    let out = git(
        dir.path(),
        &[
            "clone",
            "-q",
            "--filter=blob:none",
            "--no-local",
            origin.to_str().unwrap(),
            consumer.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "partial clone failed: {out:?}");
    (dir, consumer)
}

#[test]
fn an_absent_object_says_it_is_a_partial_clone() {
    let (_dir, consumer) = partial_clone();
    let repo = git2::Repository::open(&consumer).expect("open");
    let head = repo.head().unwrap().peel_to_commit().unwrap();

    // The file list needs only trees and succeeds; reading the *content* of the
    // change is what needs a blob on each side, and that is the path a user hits
    // when they open a commit.
    let result = commit_file_diff(&repo, &CommitId(head.id().to_string()), Path::new("f.txt"));

    match result {
        // Some Git versions fetch enough during checkout that nothing is
        // missing; that is a legitimate outcome and not a failure of the guard.
        Ok(_) => {}
        Err(GitError::Blocked(note)) => {
            let message = note.message_en();
            assert!(
                message.contains("partial clone"),
                "the refusal must name partial clone so the user knows why an \
                 object is missing from their own repository: {message}"
            );
            assert!(
                message.contains("git fetch"),
                "the refusal must say what to do about it: {message}"
            );
        }
        Err(other) => panic!(
            "an absent object in a partial clone must be explained, not passed \
             through raw (#677): {other:?}"
        ),
    }
}

#[test]
fn an_ordinary_repository_still_reports_ordinary_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path();
    git_ok(path, &["init", "-q", "-b", "main"]);
    git_ok(path, &["config", "user.email", "t@example.com"]);
    git_ok(path, &["config", "user.name", "t"]);
    std::fs::write(path.join("a.txt"), "a\n").unwrap();
    git_ok(path, &["add", "-A"]);
    git_ok(path, &["commit", "-qm", "seed"]);
    let repo = git2::Repository::open(path).expect("open");

    // A commit id that does not exist: absent, but not because of a filter.
    let missing = CommitId("0000000000000000000000000000000000000001".to_string());
    let error = commit_changed_files(&repo, &missing).expect_err("must fail");

    assert!(
        !matches!(error, GitError::Blocked(_)),
        "a repository that is not a partial clone must keep the libgit2 wording \
         — mislabelling a corrupt repo as a partial clone hides the real fault: \
         {error:?}"
    );
}
