//! Real Git/index/workdir sources behind main-diff media preparation.
use std::path::Path;

use git2::{Repository, Signature};
use kagi_git::{backend::file_reads::FileReadSource, Backend, CommitId};

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

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
