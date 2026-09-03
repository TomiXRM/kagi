//! Integration tests for directory/file conflict classification + resolution
//! (#320 / ADR-0164).
//!
//! A D/F conflict is produced the way kagi produces it at runtime: libgit2's
//! `repo.merge` (NOT the git CLI, which renames the losing file to
//! `path~BRANCH` and so never leaves a same-namespace collision in the index).
//! Each test reverts to the fix in its comment to stay a real regression.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    detect_conflict_session, execute_dir_file_resolution, plan_dir_file_resolution, ConflictKind,
    DirFileChoice,
};

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git spawn");
    assert!(s.success(), "git {:?}", args);
}

/// Build a repo with `file-side` (commits `thing` as a file) and `dir-side`
/// (commits `thing/child` as a directory), check out `checkout`, then perform a
/// libgit2 merge of `other` into it — leaving a genuine D/F conflict in the
/// index. Returns the temp dir (kept alive) and the opened repo.
fn df_conflict(checkout: &str, other: &str) -> (TempDir, Repository) {
    let td = TempDir::new().unwrap();
    let d = td.path();
    git(d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("base.txt"), b"base\n").unwrap();
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "base"]);

    git(d, &["checkout", "-q", "-b", "file-side"]);
    std::fs::write(d.join("thing"), b"i am a file\n").unwrap();
    git(d, &["add", "thing"]);
    git(d, &["commit", "-qm", "file"]);

    git(d, &["checkout", "-q", "main"]);
    git(d, &["checkout", "-q", "-b", "dir-side"]);
    std::fs::create_dir(d.join("thing")).unwrap();
    std::fs::write(d.join("thing/child"), b"inside dir\n").unwrap();
    git(d, &["add", "thing/child"]);
    git(d, &["commit", "-qm", "dir"]);

    git(d, &["checkout", "-q", checkout]);
    let repo = Repository::open(d).unwrap();
    let other_commit = repo
        .revparse_single(other)
        .unwrap()
        .peel_to_commit()
        .unwrap();
    let other_oid = other_commit.id();
    drop(other_commit);
    let annotated = repo.find_annotated_commit(other_oid).unwrap();
    let mut co = git2::build::CheckoutBuilder::new();
    co.safe();
    repo.merge(&[&annotated], None, Some(&mut co)).unwrap();
    drop(annotated);
    (td, repo)
}

fn blob_text(repo: &Repository, oid: git2::Oid) -> String {
    let blob = repo.find_blob(oid).unwrap();
    String::from_utf8_lossy(blob.content()).into_owned()
}

/// A D/F conflict must be classified `DirFile`, not the modify/delete it used to
/// masquerade as. Mutation: make `classify_kind` skip the DirFile arm (return
/// early false) and this asserts fails with ModifyDelete.
#[test]
fn dir_file_conflict_is_classified() {
    for (co, other) in [("file-side", "dir-side"), ("dir-side", "file-side")] {
        let (_td, repo) = df_conflict(co, other);
        let session = detect_conflict_session(&repo).unwrap();
        let f = session
            .files
            .iter()
            .find(|f| f.path == PathBuf::from("thing"))
            .unwrap_or_else(|| panic!("no `thing` conflict merging {other} into {co}"));
        assert_eq!(
            f.kind,
            ConflictKind::DirFile,
            "merging {other} into {co} must classify `thing` as DirFile"
        );
    }
}

/// Keep-directory yields a tree with the directory side (`thing/child`) and no
/// file blob at `thing`.
#[test]
fn keep_directory_yields_directory_side() {
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepDirectory).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    let mut index = repo.index().unwrap();
    assert!(!index.has_conflicts(), "conflict must be cleared");
    assert!(
        index.get_path(Path::new("thing/child"), 0).is_some(),
        "directory side must survive"
    );
    assert!(
        index.get_path(Path::new("thing"), 0).is_none(),
        "file blob must be gone"
    );
    // The staged index must form a valid tree (no D/F namespace clash left).
    index.write_tree().expect("index must write a clean tree");
}

/// Keep-file yields a tree with the file blob at `thing` (its bytes intact) and
/// no directory entries under `thing/`.
#[test]
fn keep_file_yields_file_side() {
    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    let mut index = repo.index().unwrap();
    assert!(!index.has_conflicts(), "conflict must be cleared");
    let entry = index
        .get_path(Path::new("thing"), 0)
        .expect("file side must be staged at stage 0");
    assert_eq!(
        blob_text(&repo, entry.id),
        "i am a file\n",
        "kept file must carry the file side's bytes"
    );
    assert!(
        index.get_path(Path::new("thing/child"), 0).is_none(),
        "directory side must be removed"
    );
    index.write_tree().expect("index must write a clean tree");
}

/// The resolution is recorded to the oplog. Mutation: drop the `append_oplog`
/// call in `execute_dir_file_resolution` and this asserts fails (no entry).
#[test]
fn resolution_recorded_in_oplog() {
    let logdir = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    let (_td, repo) = df_conflict("file-side", "dir-side");
    let plan =
        plan_dir_file_resolution(&repo, Path::new("thing"), DirFileChoice::KeepFile).unwrap();
    execute_dir_file_resolution(&repo, repo.workdir().unwrap(), &plan).unwrap();

    let log = std::fs::read_to_string(logdir.path().join("operations.jsonl"))
        .expect("oplog file must exist");
    assert!(
        log.contains("conflict-dir-file:keep-file"),
        "oplog must record the dir-file resolution, got: {log}"
    );

    std::env::remove_var("KAGI_LOG_DIR");
}
