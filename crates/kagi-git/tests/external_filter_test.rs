//! #1136: unsupported filters must be refused before index/worktree writes.
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod isolated;

use git_fixture::{commit_all, git, git_output, init_repo, write_file};
use kagi_git::{Backend, GitError, Operation};
use std::path::Path;
use tempfile::TempDir;

const POINTER: &str = "version https://git-lfs.github.com/spec/v1\noid sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nsize 42\n";

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path(), "main");
    write_file(
        dir.path(),
        ".gitattributes",
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    write_file(dir.path(), "asset.bin", POINTER);
    write_file(dir.path(), "plain.txt", "base\n");
    commit_all(dir.path(), "pointer without git-lfs");
    write_file(
        dir.path(),
        "asset.bin",
        "full worktree content, not a pointer\n",
    );
    write_file(dir.path(), "plain.txt", "changed\n");
    dir
}

fn assert_filter(error: GitError) {
    assert!(
        matches!(error, GitError::Blocked(_)),
        "typed filter refusal: {error:?}"
    );
    assert!(
        error.to_string().contains("external filter (lfs)"),
        "{error}"
    );
}

#[test]
fn external_filter_single_and_bulk_stage_preserve_index() {
    if !isolated::run_isolated() {
        return;
    }
    for bulk in [false, true] {
        let dir = fixture();
        let backend = Backend::open(dir.path()).unwrap();
        let before = std::fs::read(dir.path().join(".git/index")).unwrap();
        let result = if bulk {
            backend
                .stage_files(&["plain.txt".into(), "asset.bin".into()])
                .map(|_| ())
        } else {
            backend.stage_file(Path::new("asset.bin"))
        };
        assert_filter(result.expect_err("raw worktree bytes must not be staged"));
        assert_eq!(
            std::fs::read(dir.path().join(".git/index")).unwrap(),
            before
        );
        assert_eq!(
            git_output(dir.path(), &["show", ":asset.bin"]),
            POINTER.trim()
        );
        backend.stage_file(Path::new("plain.txt")).unwrap();
        assert_eq!(git_output(dir.path(), &["show", ":plain.txt"]), "changed");
    }
}

#[test]
fn external_filter_hunk_and_discard_preserve_index_and_worktree() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    let mut backend = Backend::open(dir.path()).unwrap();
    let before = std::fs::read(dir.path().join(".git/index")).unwrap();
    let bytes = std::fs::read(dir.path().join("asset.bin")).unwrap();
    let diff = backend.unstaged_file_diff(Path::new("asset.bin")).unwrap();
    let approved = diff.hunks[0].approval().unwrap();
    assert_filter(
        backend
            .stage_hunk(Path::new("asset.bin"), approved)
            .unwrap_err(),
    );
    let op = Operation::Discard {
        paths: vec!["asset.bin".into()],
    };
    let plan = backend.plan(&op).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|n| n.message_en().contains("external filter (lfs)")),
        "discard must refuse filtered checkout: {:?}",
        plan.blockers
    );
    assert!(backend.run(&op, &plan).is_err());
    assert_eq!(
        std::fs::read(dir.path().join(".git/index")).unwrap(),
        before
    );
    assert_eq!(std::fs::read(dir.path().join("asset.bin")).unwrap(), bytes);
    assert!(git_output(
        dir.path(),
        &["for-each-ref", "--format=%(refname)", "refs/kagi/backups"]
    )
    .is_empty());
}

#[test]
fn external_filter_info_attributes_and_lfs_markers_are_honored() {
    if !isolated::run_isolated() {
        return;
    }
    for attributes in [
        "asset.bin filter=lfs\n",
        "asset.bin diff=lfs\n",
        "asset.bin merge=lfs\n",
        "asset.bin filter=custom\n",
    ] {
        let dir = fixture();
        write_file(dir.path(), ".gitattributes", "*.bin -filter -diff -merge\n");
        write_file(dir.path(), ".git/info/attributes", attributes);
        let backend = Backend::open(dir.path()).unwrap();
        let before = std::fs::read(dir.path().join(".git/index")).unwrap();
        let error = backend.stage_file(Path::new("asset.bin")).unwrap_err();
        assert!(matches!(error, GitError::Blocked(_)), "{error:?}");
        assert_eq!(
            std::fs::read(dir.path().join(".git/index")).unwrap(),
            before
        );
    }
}

#[test]
fn external_filter_late_attributes_refuse_discard_before_backup() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    write_file(dir.path(), ".gitattributes", "*.bin -filter -diff -merge\n");
    git(dir.path(), &["add", ".gitattributes"]);
    let mut backend = Backend::open(dir.path()).unwrap();
    let op = Operation::Discard {
        paths: vec!["asset.bin".into()],
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty());
    write_file(dir.path(), ".git/info/attributes", "asset.bin filter=lfs\n");
    let before = std::fs::read(dir.path().join(".git/index")).unwrap();
    assert!(backend.run(&op, &plan).is_err());
    assert_eq!(
        std::fs::read(dir.path().join(".git/index")).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("asset.bin")).unwrap(),
        "full worktree content, not a pointer\n"
    );
    assert!(git_output(
        dir.path(),
        &["for-each-ref", "--format=%(refname)", "refs/kagi/backups"]
    )
    .is_empty());
}

#[test]
fn external_filter_unstage_restores_pointer_without_importing_worktree() {
    if !isolated::run_isolated() {
        return;
    }
    let dir = fixture();
    // Git without a configured LFS driver can stage raw bytes; Kagi must still
    // let users remove them from the index using HEAD's pointer content.
    git(dir.path(), &["add", "asset.bin"]);
    let backend = Backend::open(dir.path()).unwrap();
    let diff = backend.staged_file_diff(Path::new("asset.bin")).unwrap();
    backend
        .unstage_hunk(Path::new("asset.bin"), diff.hunks[0].approval().unwrap())
        .unwrap();
    assert_eq!(
        git_output(dir.path(), &["show", ":asset.bin"]),
        POINTER.trim()
    );
    git(dir.path(), &["add", "asset.bin"]);
    backend.unstage_file(Path::new("asset.bin")).unwrap();
    assert_eq!(
        git_output(dir.path(), &["show", ":asset.bin"]),
        POINTER.trim()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("asset.bin")).unwrap(),
        "full worktree content, not a pointer\n"
    );
}
