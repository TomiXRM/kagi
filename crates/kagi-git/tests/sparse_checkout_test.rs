//! #675: a sparse-excluded path is absent on purpose, not deleted.
//!
//! Sparse-checkout sets `skip-worktree` on the index entry and removes the file.
//! "Missing from the working tree" therefore does not mean "the user deleted
//! it" — a distinction Git makes and libgit2's status does not. Kagi reported
//! every excluded file as a deletion, and staging one recorded that deletion for
//! real, in a product whose reason to exist is preventing exactly that.

use kagi_git::{working_tree_status, Backend, GitError};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A repo where `drop/` is sparse-excluded and `keep/` is present.
fn sparse_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["config", "user.email", "t@example.com"]);
    git(path, &["config", "user.name", "t"]);
    std::fs::create_dir_all(path.join("keep")).unwrap();
    std::fs::create_dir_all(path.join("drop")).unwrap();
    std::fs::write(path.join("keep/a.txt"), "a\n").unwrap();
    std::fs::write(path.join("drop/b1.txt"), "b1\n").unwrap();
    std::fs::write(path.join("drop/b2.txt"), "b2\n").unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-qm", "seed"]);
    git(path, &["sparse-checkout", "init", "--cone"]);
    git(path, &["sparse-checkout", "set", "keep"]);
    assert!(
        !path.join("drop/b1.txt").exists(),
        "fixture needs the excluded file gone from the worktree"
    );
    dir
}

#[test]
fn sparse_excluded_files_are_not_reported_as_deletions() {
    let dir = sparse_repo();
    let repo = git2::Repository::open(dir.path()).expect("open");

    let status = working_tree_status(&repo).expect("status");

    assert!(
        status.unstaged.is_empty() && status.staged.is_empty(),
        "git reports this tree clean; a sparse-excluded file is absent on \
         purpose, not deleted (#675): {status:?}"
    );
}

#[test]
fn staging_a_sparse_excluded_path_is_refused() {
    let dir = sparse_repo();
    let backend = Backend::open(dir.path()).expect("open");

    let error = backend
        .stage_file(Path::new("drop/b1.txt"))
        .expect_err("staging a sparse-excluded path must be refused (#675)");

    assert!(
        matches!(error, GitError::Blocked(_)),
        "the refusal must be the typed plan refusal so the UI can localise it, \
         got: {error:?}"
    );

    // The index must still carry the entry: refusing is only meaningful if the
    // deletion was not recorded on the way out.
    let repo = git2::Repository::open(dir.path()).expect("reopen");
    let index = repo.index().expect("index");
    assert!(
        index.get_path(Path::new("drop/b1.txt"), 0).is_some(),
        "the refused staging must leave the index untouched — otherwise the \
         next commit deletes a file the user never touched"
    );
}

#[test]
fn a_real_deletion_is_still_stageable() {
    let dir = sparse_repo();
    // `keep/a.txt` is inside the sparse cone, so deleting it is a real deletion.
    std::fs::remove_file(dir.path().join("keep/a.txt")).unwrap();
    let backend = Backend::open(dir.path()).expect("open");

    backend.stage_file(Path::new("keep/a.txt")).expect(
        "a genuine deletion must still stage — the guard must not block \
                 ordinary work",
    );

    let repo = git2::Repository::open(dir.path()).expect("reopen");
    let index = repo.index().expect("index");
    assert!(
        index.get_path(Path::new("keep/a.txt"), 0).is_none(),
        "a real deletion should have been staged"
    );
}

#[test]
fn staging_everything_is_refused_when_a_sparse_path_is_in_the_batch() {
    let dir = sparse_repo();
    let backend = Backend::open(dir.path()).expect("open");

    // "Stage all" is the path a user actually takes, and it is a different
    // function from the single-file one (#675).
    let error = backend
        .stage_files(&[
            std::path::PathBuf::from("drop/b1.txt"),
            std::path::PathBuf::from("drop/b2.txt"),
        ])
        .expect_err("bulk staging must refuse a sparse-excluded path too");

    assert!(
        matches!(error, GitError::Blocked(_)),
        "the refusal must be the typed plan refusal, got: {error:?}"
    );

    let repo = git2::Repository::open(dir.path()).expect("reopen");
    let index = repo.index().expect("index");
    for path in ["drop/b1.txt", "drop/b2.txt"] {
        assert!(
            index.get_path(Path::new(path), 0).is_some(),
            "the refused batch must leave the index untouched: {path}"
        );
    }
}

#[test]
fn staging_everything_still_works_on_real_changes() {
    let dir = sparse_repo();
    std::fs::write(dir.path().join("keep/a.txt"), "changed\n").unwrap();
    std::fs::write(dir.path().join("keep/new.txt"), "new\n").unwrap();
    let backend = Backend::open(dir.path()).expect("open");

    let staged = backend
        .stage_files(&[
            std::path::PathBuf::from("keep/a.txt"),
            std::path::PathBuf::from("keep/new.txt"),
        ])
        .expect("ordinary bulk staging must not be blocked by the guard");

    assert_eq!(staged, 2);
}
