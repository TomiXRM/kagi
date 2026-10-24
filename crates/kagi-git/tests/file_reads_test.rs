//! Real Git/index/workdir and frozen conflict sources behind media preparation.
use std::path::Path;
use std::process::Command;

use git2::{Repository, Signature};
use kagi_git::{
    backend::file_reads::FileReadSource, detect_conflict_session, Backend, CommitId, ConflictKind,
    ResolutionBuffer, SelectionSide,
};
use tempfile::TempDir;

const LIMIT: usize = 10 * 1024 * 1024;

struct Fixture {
    temp: tempfile::TempDir,
    repo: Repository,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = Repository::init(temp.path()).unwrap();
        Self { temp, repo }
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        std::fs::write(self.temp.path().join(path), bytes).unwrap();
    }
    fn stage(&self, path: &str) {
        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new(path)).unwrap();
        index.write().unwrap();
    }
    fn commit(&self) -> CommitId {
        let mut index = self.repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = self.repo.find_tree(tree_id).unwrap();
        let parent = self
            .repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok());
        let signature = Signature::now("Test", "test@example.com").unwrap();
        let id = self
            .repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                "media source",
                &tree,
                &parent.iter().collect::<Vec<_>>(),
            )
            .unwrap();
        CommitId(id.to_string())
    }
    fn backend(&self) -> Backend {
        Backend::open(self.temp.path()).unwrap()
    }
}

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

/// Distinct frozen descriptors must preserve the exact current/incoming blobs,
/// even after staging a replacement; missing objects are errors, not absence.
#[test]
fn binary_conflict_stage_reads_are_exact_per_side() {
    if !test_support::run_isolated() {
        return;
    }
    let (tmp, current, incoming) = binary_conflict();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.files[0].kind, ConflictKind::Binary);
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let path = Path::new("blob.bin");
    assert_ne!(current, incoming, "fixture sides must differ");
    assert_eq!(
        buffer.side_raw_meta(Path::new("absent"), SelectionSide::Current),
        None
    );
    let descriptors = [SelectionSide::Current, SelectionSide::Incoming]
        .map(|side| buffer.side_raw_meta(path, side).unwrap());
    std::fs::write(tmp.path().join(path), b"replacement").unwrap();
    git(tmp.path(), &["add", "blob.bin"]);
    let backend = Backend::open(tmp.path()).unwrap();
    for ((side, expected), (oid, mode)) in [
        (SelectionSide::Current, current.as_slice()),
        (SelectionSide::Incoming, incoming.as_slice()),
    ]
    .into_iter()
    .zip(&descriptors)
    {
        assert_eq!(buffer.side_raw_meta(path, side), Some((oid.clone(), *mode)));
        let source = FileReadSource::ConflictStage { oid, mode: *mode };
        assert_eq!(
            backend.file_bytes(source, path, expected.len()).unwrap(),
            Some(expected.to_vec()),
            "{side:?} bytes must match the frozen side"
        );
        assert_eq!(
            backend.file_size(source, path).unwrap(),
            Some(expected.len() as u64)
        );
        assert!(backend
            .file_bytes(source, path, expected.len() - 1)
            .is_err());
    }
    let (oid, mode) = &descriptors[0];
    let object = repo.path().join("objects").join(&oid[..2]).join(&oid[2..]);
    std::fs::remove_file(object).unwrap();
    // A fresh handle avoids libgit2's already-materialized object cache.
    let backend = Backend::open(tmp.path()).unwrap();
    let source = FileReadSource::ConflictStage { oid, mode: *mode };
    assert!(backend.file_bytes(source, path, usize::MAX).is_err());
    assert!(backend.file_size(source, path).is_err());
}

/// Reading the worktree link instead would substitute one side or dereference
/// its target: each frozen symlink blob must return its own target verbatim.
#[test]
fn symlink_conflict_stage_reads_are_link_targets() {
    if !test_support::run_isolated() {
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
    let backend = Backend::open(dir).unwrap();
    let path = Path::new("link");
    for (side, expected) in [
        (SelectionSide::Current, b"current-target".as_slice()),
        (SelectionSide::Incoming, b"incoming-target".as_slice()),
    ] {
        let (oid, mode) = buffer.side_raw_meta(path, side).unwrap();
        assert_eq!(mode, 0o120000);
        let source = FileReadSource::ConflictStage { oid: &oid, mode };
        assert_eq!(
            backend.file_bytes(source, path, expected.len()).unwrap(),
            Some(expected.to_vec()),
            "{side:?} symlink target verbatim"
        );
    }
}

#[test]
fn frozen_conflict_sources_are_bounded_and_never_reread_the_index() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    let bytes = b"%PDF-frozen";
    let oid = fixture.repo.blob(bytes).unwrap().to_string();
    let empty_oid = fixture.repo.blob(b"").unwrap().to_string();
    fixture.write("side", b"replacement");
    fixture.stage("side");
    std::fs::write(fixture.repo.path().join("index"), b"invalid index").unwrap();
    let backend = fixture.backend();
    for mode in [0o100644, 0o100755, 0o120000] {
        let source = FileReadSource::ConflictStage { oid: &oid, mode };
        for path in [Path::new("side"), Path::new("absent")] {
            assert_eq!(
                backend.file_size(source, path).unwrap(),
                Some(bytes.len() as u64)
            );
            assert_eq!(
                backend
                    .file_bytes(source, path, bytes.len())
                    .unwrap()
                    .unwrap(),
                bytes
            );
            assert!(backend.file_bytes(source, path, bytes.len() - 1).is_err());
            for (prefix, expected) in [
                (b"".as_slice(), true),
                (b"%PDF-".as_slice(), true),
                (bytes.as_slice(), true),
                (b"%PDF-frozen-longer".as_slice(), false),
                (b"other".as_slice(), false),
            ] {
                assert_eq!(
                    backend.file_has_prefix(source, path, prefix).unwrap(),
                    Some(expected)
                );
            }
        }
    }
    let empty = FileReadSource::ConflictStage {
        oid: &empty_oid,
        mode: 0o100644,
    };
    assert_eq!(
        backend.file_bytes(empty, Path::new("absent"), 0).unwrap(),
        Some(vec![])
    );
    assert_eq!(
        backend
            .file_has_prefix(empty, Path::new("absent"), b"x")
            .unwrap(),
        Some(false)
    );
}

#[test]
fn frozen_present_object_errors_are_not_absence_and_gitlinks_skip_lookup() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    fixture.write("side", b"content");
    fixture.stage("side");
    let commit = fixture.commit();
    let backend = fixture.backend();
    let path = Path::new("absent");
    assert_eq!(
        backend
            .file_bytes(FileReadSource::Index, path, LIMIT)
            .unwrap(),
        None
    );
    assert_eq!(
        backend.file_size(FileReadSource::Index, path).unwrap(),
        None
    );
    assert_eq!(
        backend
            .file_has_prefix(FileReadSource::Index, path, b"x")
            .unwrap(),
        None
    );
    for oid in [
        "not-an-oid",
        "1111111111111111111111111111111111111111",
        &commit.0,
    ] {
        let source = FileReadSource::ConflictStage {
            oid,
            mode: 0o100644,
        };
        assert!(backend.file_bytes(source, path, LIMIT).is_err());
        assert!(backend.file_has_prefix(source, path, b"").is_err());
        assert!(backend.file_size(source, path).is_err());
        let gitlink = FileReadSource::ConflictStage {
            oid,
            mode: 0o160000,
        };
        assert_eq!(backend.file_bytes(gitlink, path, 0).unwrap(), None);
        assert_eq!(backend.file_has_prefix(gitlink, path, b"").unwrap(), None);
        assert_eq!(backend.file_size(gitlink, path).unwrap(), None);
    }
}

#[test]
fn historical_head_index_and_workdir_reads_do_not_substitute_each_other() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    let path = Path::new("signature.note");
    fixture.write("signature.note", b"%PDF-historical");
    fixture.stage("signature.note");
    let historical = fixture.commit();
    fixture.write("signature.note", b"%PDF-head");
    fixture.stage("signature.note");
    fixture.commit();
    fixture.write("signature.note", b"%PDF-index");
    fixture.stage("signature.note");
    fixture.write("signature.note", b"%PDF-workdir");
    let backend = fixture.backend();
    for (source, expected) in [
        (
            FileReadSource::Commit(&historical),
            b"%PDF-historical".as_slice(),
        ),
        (FileReadSource::Head, b"%PDF-head".as_slice()),
        (FileReadSource::Index, b"%PDF-index".as_slice()),
        (FileReadSource::IndexOrHead, b"%PDF-index".as_slice()),
        (FileReadSource::Workdir, b"%PDF-workdir".as_slice()),
    ] {
        assert_eq!(
            backend.file_has_prefix(source, path, b"%PDF-").unwrap(),
            Some(true)
        );
        assert_eq!(
            backend.file_has_prefix(source, path, expected).unwrap(),
            Some(true)
        );
        assert_eq!(
            backend.file_has_prefix(source, path, b"plain").unwrap(),
            Some(false)
        );
        assert_eq!(
            backend
                .file_bytes(source, path, expected.len())
                .unwrap()
                .unwrap(),
            expected
        );
        assert!(backend
            .file_bytes(source, path, expected.len() - 1)
            .is_err());
    }
}

#[test]
fn all_sources_accept_the_pdf_boundary_and_reject_one_byte_over_as_failure() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    for (path, size) in [
        ("below.note", LIMIT - 1),
        ("exact.note", LIMIT),
        ("over.note", LIMIT + 1),
    ] {
        let mut bytes = vec![b' '; size];
        bytes[..5].copy_from_slice(b"%PDF-");
        fixture.write(path, &bytes);
        fixture.stage(path);
    }
    let commit = fixture.commit();
    let backend = fixture.backend();
    for source in [
        FileReadSource::Commit(&commit),
        FileReadSource::Head,
        FileReadSource::Index,
        FileReadSource::IndexOrHead,
        FileReadSource::Workdir,
    ] {
        for (path, size) in [("below.note", LIMIT - 1), ("exact.note", LIMIT)] {
            let bytes = backend
                .file_bytes(source, Path::new(path), LIMIT)
                .unwrap()
                .unwrap();
            assert_eq!(bytes.len(), size);
            assert!(bytes.starts_with(b"%PDF-"));
            assert!(bytes[5..].iter().all(|byte| *byte == b' '));
            assert!(bytes.capacity() <= LIMIT);
        }
        let over = Path::new("over.note");
        assert_eq!(
            backend.file_has_prefix(source, over, b"%PDF-").unwrap(),
            Some(true)
        );
        assert!(backend.file_bytes(source, over, LIMIT).is_err());
        let missing = Path::new("absent.note");
        assert_eq!(backend.file_bytes(source, missing, LIMIT).unwrap(), None);
        assert_eq!(
            backend.file_has_prefix(source, missing, b"%PDF-").unwrap(),
            None
        );
    }
}

#[test]
fn missing_index_falls_back_to_head_but_unreadable_index_does_not() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    fixture.write("document.pdf", b"%PDF-head");
    fixture.stage("document.pdf");
    fixture.commit();
    let path = Path::new("document.pdf");
    let mut index = fixture.repo.index().unwrap();
    index.remove_path(path).unwrap();
    index.write().unwrap();
    let backend = fixture.backend();
    assert_eq!(
        backend
            .file_bytes(FileReadSource::Index, path, LIMIT)
            .unwrap(),
        None
    );
    assert_eq!(
        backend
            .file_bytes(FileReadSource::IndexOrHead, path, LIMIT)
            .unwrap()
            .unwrap(),
        b"%PDF-head"
    );
    assert_eq!(
        backend
            .file_has_prefix(FileReadSource::IndexOrHead, path, b"%PDF-head")
            .unwrap(),
        Some(true)
    );

    std::fs::write(fixture.repo.path().join("index"), b"invalid index").unwrap();
    let backend = fixture.backend();
    for source in [FileReadSource::Index, FileReadSource::IndexOrHead] {
        assert!(backend.file_has_prefix(source, path, b"%PDF-").is_err());
        assert!(backend.file_bytes(source, path, LIMIT).is_err());
    }
    assert_eq!(
        backend
            .file_bytes(FileReadSource::Head, path, LIMIT)
            .unwrap()
            .unwrap(),
        b"%PDF-head"
    );
}

#[test]
fn workdir_prefix_handles_short_empty_and_sparse_files_without_full_reads() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    let backend = fixture.backend();
    for (name, bytes, expected) in [
        ("empty", b"".as_slice(), false),
        ("short", b"%PDF".as_slice(), false),
        ("text", b"ordinary text".as_slice(), false),
        ("pdf", b"%PDF-".as_slice(), true),
    ] {
        fixture.write(name, bytes);
        assert_eq!(
            backend
                .file_has_prefix(FileReadSource::Workdir, Path::new(name), b"%PDF-")
                .unwrap(),
            Some(expected)
        );
    }
    let sparse = fixture.temp.path().join("huge.note");
    fixture.write("huge.note", b"%PDF-");
    std::fs::OpenOptions::new()
        .write(true)
        .open(sparse)
        .unwrap()
        // Extending a file does not guarantee sparse allocation on Windows.
        .set_len(if cfg!(windows) {
            LIMIT as u64 + 1
        } else {
            (LIMIT as u64) * 1024
        })
        .unwrap();
    let path = Path::new("huge.note");
    assert_eq!(
        backend
            .file_has_prefix(FileReadSource::Workdir, path, b"%PDF-")
            .unwrap(),
        Some(true)
    );
    assert!(backend
        .file_bytes(FileReadSource::Workdir, path, LIMIT)
        .is_err());
    std::fs::create_dir(fixture.temp.path().join("directory.pdf")).unwrap();
    assert!(backend
        .file_has_prefix(
            FileReadSource::Workdir,
            Path::new("directory.pdf"),
            b"%PDF-"
        )
        .is_err());
    assert!(backend
        .file_bytes(FileReadSource::Workdir, Path::new("directory.pdf"), LIMIT)
        .is_err());
}

#[test]
fn unborn_head_is_absent_but_invalid_history_is_an_error() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    let backend = fixture.backend();
    let path = Path::new("document.pdf");
    for source in [
        FileReadSource::Head,
        FileReadSource::Index,
        FileReadSource::IndexOrHead,
    ] {
        assert_eq!(backend.file_bytes(source, path, LIMIT).unwrap(), None);
        assert_eq!(
            backend.file_has_prefix(source, path, b"%PDF-").unwrap(),
            None
        );
    }
    let invalid = CommitId("not-a-commit-id".into());
    assert!(backend
        .file_has_prefix(FileReadSource::Commit(&invalid), path, b"%PDF-")
        .is_err());
    assert!(backend
        .file_bytes(FileReadSource::Commit(&invalid), path, LIMIT)
        .is_err());
}

#[cfg(unix)]
#[test]
fn workdir_symlinks_are_not_previewed_as_their_targets() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    fixture.write("regular.pdf", b"%PDF-inside");
    std::fs::write(outside.path().join("outside.pdf"), b"%PDF-outside").unwrap();
    let backend = fixture.backend();
    for (name, target) in [
        ("inside-link.pdf", fixture.temp.path().join("regular.pdf")),
        ("outside-link.pdf", outside.path().join("outside.pdf")),
        ("dangling-link.pdf", outside.path().join("missing.pdf")),
    ] {
        std::os::unix::fs::symlink(target, fixture.temp.path().join(name)).unwrap();
        assert!(backend
            .file_has_prefix(FileReadSource::Workdir, Path::new(name), b"%PDF-")
            .is_err());
        assert!(backend
            .file_bytes(FileReadSource::Workdir, Path::new(name), LIMIT)
            .is_err());
    }
    assert_eq!(
        backend
            .file_bytes(FileReadSource::Workdir, Path::new("regular.pdf"), LIMIT)
            .unwrap(),
        Some(b"%PDF-inside".to_vec())
    );
}

#[test]
fn conflicted_unstaged_sources_use_ours_then_ancestor_without_head_fallback() {
    if !test_support::run_isolated() {
        return;
    }
    for (head, ancestor, ours, expected) in [
        (
            None,
            None,
            Some(b"%PDF-ours".as_slice()),
            Some(b"%PDF-ours".as_slice()),
        ),
        (
            Some(b"%PDF-head".as_slice()),
            Some(b"%PDF-base".as_slice()),
            None,
            Some(b"%PDF-base".as_slice()),
        ),
        (Some(b"%PDF-head".as_slice()), None, None, None),
    ] {
        let fixture = Fixture::new();
        fixture.write("other.txt", b"base");
        fixture.stage("other.txt");
        if let Some(head) = head {
            fixture.write("document.pdf", head);
            fixture.stage("document.pdf");
        }
        fixture.commit();
        fixture.write("document.pdf", b"%PDF-working-tree");
        fixture.stage("document.pdf");
        let path = Path::new("document.pdf");
        let mut index = fixture.repo.index().unwrap();
        let mut entry = index.get_path(path, 0).unwrap();
        index.remove_path(path).unwrap();
        for (stage, bytes) in [
            (1u16, ancestor),
            (2u16, ours),
            (3u16, Some(b"%PDF-theirs".as_slice())),
        ] {
            if let Some(bytes) = bytes {
                entry.id = fixture.repo.blob(bytes).unwrap();
                entry.flags = (entry.flags & !0x3000) | (stage << 12);
                index.add(&entry).unwrap();
            }
        }
        index.write().unwrap();
        assert!(index.has_conflicts());
        let backend = fixture.backend();
        assert_eq!(
            backend
                .file_bytes(FileReadSource::IndexOrHead, path, LIMIT)
                .unwrap(),
            expected.map(<[u8]>::to_vec)
        );
        assert_eq!(
            backend
                .file_has_prefix(FileReadSource::IndexOrHead, path, b"%PDF-")
                .unwrap(),
            expected.map(|_| true)
        );
        assert_eq!(
            backend
                .file_bytes(FileReadSource::Index, path, LIMIT)
                .unwrap(),
            None
        );
        let diff = backend.unstaged_file_diff(path).unwrap();
        assert_eq!(diff.old_path.as_deref(), expected.map(|_| path));
        assert_eq!(
            diff.change,
            if expected.is_some() {
                kagi_git::ChangeKind::Modified
            } else {
                kagi_git::ChangeKind::Added
            },
            "an absent conflict base remains absent for media preparation, even when HEAD has it"
        );
    }
}

#[cfg(unix)]
#[test]
fn conflicted_sources_match_exact_non_utf8_paths() {
    use std::os::unix::ffi::OsStrExt;

    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new();
    fixture.write("other.txt", b"base");
    fixture.stage("other.txt");
    fixture.commit();
    let mut index = fixture.repo.index().unwrap();
    for (name, bytes) in [
        (b"a-\xff.pdf", b"%PDF-first".as_slice()),
        (b"b-\xff.pdf", b"%PDF-second".as_slice()),
    ] {
        // Index paths need not be representable by the host filesystem.
        let mut entry = index.get_path(Path::new("other.txt"), 0).unwrap();
        entry.path = name.to_vec();
        entry.id = fixture.repo.blob(bytes).unwrap();
        entry.flags = (2 << 12) | name.len() as u16;
        index.add(&entry).unwrap();
    }
    index.write().unwrap();
    let requested = Path::new(std::ffi::OsStr::from_bytes(b"b-\xff.pdf"));
    assert_eq!(
        fixture
            .backend()
            .file_bytes(FileReadSource::IndexOrHead, requested, LIMIT)
            .unwrap(),
        Some(b"%PDF-second".to_vec())
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
