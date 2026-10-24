//! #321 viewer — side-bytes accessor for binary / symlink conflicts.
//!
//! The Conflict Mode viewer reads each side's raw blob bytes so it can render an
//! image / show a symlink target WITHOUT re-opening git2 in the UI. Each test
//! drives a real git fixture; the per-test comment records the mutation.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{detect_conflict_session, ConflictKind, ResolutionBuffer, SelectionSide};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {:?} failed", args);
}

fn git_allow_fail(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status();
}

fn init_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    git(tmp.path(), &["init", "-q", "-b", "main"]);
    tmp
}

/// A binary conflict: our=main_bytes, their=side_bytes (main merges side).
fn binary_conflict() -> (TempDir, Vec<u8>, Vec<u8>) {
    let tmp = init_repo();
    let dir = tmp.path();
    let base = vec![0u8, 1, 2, 0, 3];
    let main_bytes = vec![0u8, 1, 2, 0, 9, 9, 9];
    let side_bytes = vec![0u8, 7, 7, 0, 4, 2];
    std::fs::write(dir.join("blob.bin"), &base).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    std::fs::write(dir.join("blob.bin"), &side_bytes).unwrap();
    git(dir, &["commit", "-qam", "side"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("blob.bin"), &main_bytes).unwrap();
    git(dir, &["commit", "-qam", "main"]);

    git_allow_fail(dir, &["merge", "side"]);
    (tmp, main_bytes, side_bytes)
}

/// The viewer must get EXACTLY each side's blob bytes.
///
/// Mutation: swapping Current/Incoming in `side_bytes` flips this red
/// (main_bytes != side_bytes by construction).
#[test]
fn binary_conflict_side_bytes_are_exact_per_side() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (tmp, main_bytes, side_bytes) = binary_conflict();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.files[0].kind, ConflictKind::Binary);

    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let p = Path::new("blob.bin");

    assert_ne!(main_bytes, side_bytes, "fixture sides must differ");
    assert_eq!(
        buffer
            .side_bytes(&repo, p, SelectionSide::Current, main_bytes.len())
            .unwrap()
            .unwrap(),
        main_bytes,
        "current side bytes must be the 'our' blob"
    );
    assert_eq!(
        buffer
            .side_bytes(&repo, p, SelectionSide::Incoming, side_bytes.len())
            .unwrap()
            .unwrap(),
        side_bytes,
        "incoming side bytes must be the 'their' blob"
    );

    let ci = buffer
        .side_blob_info(&repo, p, SelectionSide::Current)
        .unwrap()
        .unwrap();
    assert_eq!(ci.size, Some(main_bytes.len() as u64));
    assert_eq!(ci.oid_short.len(), 7);
}

/// A conflicted symlink's side bytes ARE the link target path.
///
/// Mutation: dereferencing the on-disk link instead of reading the blob would
/// yield the same target for both sides (or escape the repo); here each side's
/// distinct target must come back byte-for-byte.
#[test]
fn symlink_conflict_side_bytes_are_link_targets() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = init_repo();
    let dir = tmp.path();
    std::os::unix::fs::symlink("placeholder", dir.join("link")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base link"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    std::fs::remove_file(dir.join("link")).unwrap();
    std::os::unix::fs::symlink("incoming-target", dir.join("link")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "side link"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::remove_file(dir.join("link")).unwrap();
    std::os::unix::fs::symlink("current-target", dir.join("link")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "main link"]);

    git_allow_fail(dir, &["merge", "side"]);

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.files[0].kind, ConflictKind::Symlink);
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let p = Path::new("link");

    let cur = buffer
        .side_bytes(&repo, p, SelectionSide::Current, 1024)
        .unwrap()
        .unwrap();
    let inc = buffer
        .side_bytes(&repo, p, SelectionSide::Incoming, 1024)
        .unwrap()
        .unwrap();
    assert_eq!(cur, b"current-target", "current symlink target verbatim");
    assert_eq!(inc, b"incoming-target", "incoming symlink target verbatim");
}

/// Frozen sides survive index replacement; lost objects are not absent sides.
#[test]
fn frozen_side_reads_preserve_bounds_identity_and_errors() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (tmp, current, _) = binary_conflict();
    let repo = Repository::open(tmp.path()).unwrap();
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let path = Path::new("blob.bin");
    let side = SelectionSide::Current;
    let descriptor = buffer.side_raw_meta(path, side).unwrap();
    let backend = kagi_git::Backend::open(tmp.path()).unwrap();
    assert!(backend
        .conflict_side_bytes(&buffer, path, side, current.len() - 1)
        .is_err());
    assert_eq!(
        buffer
            .side_bytes(&repo, Path::new("absent"), side, 0)
            .unwrap(),
        None
    );
    assert!(buffer
        .side_blob_info(&repo, Path::new("absent"), side)
        .unwrap()
        .is_none());
    std::fs::write(tmp.path().join(path), b"replacement").unwrap();
    git(tmp.path(), &["add", "blob.bin"]);
    assert_eq!(buffer.side_raw_meta(path, side).unwrap(), descriptor);
    assert_eq!(
        backend
            .conflict_side_bytes(&buffer, path, side, current.len())
            .unwrap()
            .unwrap(),
        current
    );
    let object = repo
        .path()
        .join("objects")
        .join(&descriptor.0[..2])
        .join(&descriptor.0[2..]);
    std::fs::remove_file(object).unwrap();
    // Fresh handles avoid libgit2's already-materialized object cache.
    let repo = Repository::open(tmp.path()).unwrap();
    let backend = kagi_git::Backend::open(tmp.path()).unwrap();
    assert!(buffer.side_bytes(&repo, path, side, usize::MAX).is_err());
    assert!(buffer.side_blob_info(&repo, path, side).is_err());
    assert!(backend
        .conflict_side_bytes(&buffer, path, side, usize::MAX)
        .is_err());
    assert!(backend
        .conflict_side_blob_info(&buffer, path, side)
        .is_err());
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
