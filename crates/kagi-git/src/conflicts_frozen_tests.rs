//! Deterministic external-writer interleavings at the WT/index boundary.
use super::*;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();
    init_repo(path, "main");
    write_file(path, "file.txt", "base\n");
    commit_all(path, "base");
    git(path, &["checkout", "-qb", "side"]);
    write_file(path, "file.txt", "incoming\n");
    commit_all(path, "incoming");
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "file.txt", "current\n");
    commit_all(path, "current");
    assert!(!git_succeeds(path, &["merge", "--no-commit", "side"]));
    apply_file_mode(&path.join("file.txt"), Some(0o755));
    dir
}

fn assert_frozen_staging(continue_all: bool) {
    let dir = fixture();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let backend = crate::Backend::open(path).unwrap();
    let revision = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "APPROVED\n")
        .unwrap();
    let head = git_output(path, &["rev-parse", "HEAD"]);
    let merge_head = std::fs::read(path.join(".git/MERGE_HEAD")).unwrap();
    let interfere = || {
        // Two independent external writes after Kagi materializes Result. Neither
        // writer's bytes or changed executable bit may become approved staging.
        write_file(path, "file.txt", "WRITER ONE\n");
        write_file(path, "file.txt", "WRITER TWO\n");
        apply_file_mode(&path.join("file.txt"), Some(0o644));
    };
    if continue_all {
        stage_conflict_resolution_with_worktree_hook(
            &repo, &session, &buffer, &revision, interfere,
        )
        .unwrap();
    } else {
        execute_conflict_save_with_progress(&repo, &buffer, Path::new("file.txt"), |progress| {
            if progress == kagi_domain::conflict_family::ConflictProgress::WorktreeWritten {
                interfere();
            }
        })
        .unwrap();
    }
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    assert!(!index.has_conflicts());
    let entry = index.get_path(Path::new("file.txt"), 0).unwrap();
    assert_eq!(repo.find_blob(entry.id).unwrap().content(), b"APPROVED\n");
    assert_eq!(entry.mode, if cfg!(unix) { 0o100755 } else { 0o100644 });
    assert_eq!(
        std::fs::read(path.join("file.txt")).unwrap(),
        b"WRITER TWO\n"
    );
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read(path.join(".git/MERGE_HEAD")).unwrap(),
        merge_head
    );
}

#[test]
fn save_stages_frozen_result_after_two_external_worktree_writers() {
    assert_frozen_staging(false);
}

#[test]
fn continue_stages_frozen_result_after_two_external_worktree_writers() {
    assert_frozen_staging(true);
}

fn assert_configured_filemode(continue_all: bool, trust_filemode: bool, ours: u32, worktree: u32) {
    let dir = fixture();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.filemode", trust_filemode)
        .unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new("file.txt"), 2).unwrap();
    entry.mode = ours;
    index.add(&entry).unwrap();
    index.write().unwrap();
    apply_file_mode(&path.join("file.txt"), Some(worktree));
    let backend = crate::Backend::open(path).unwrap();
    let revision = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "APPROVED\n")
        .unwrap();
    if continue_all {
        stage_conflict_resolution_with_worktree_hook(&repo, &session, &buffer, &revision, || {})
            .unwrap();
    } else {
        execute_conflict_save_with_progress(&repo, &buffer, Path::new("file.txt"), |_| {}).unwrap();
    }
    index.read(true).unwrap();
    assert!(!index.has_conflicts());
    let staged = index.get_path(Path::new("file.txt"), 0).unwrap();
    let expected = if !trust_filemode {
        ours
    } else if cfg!(unix) && worktree & 0o111 != 0 {
        0o100755
    } else {
        0o100644
    };
    assert_eq!(staged.mode, expected);
    assert_eq!(repo.find_blob(staged.id).unwrap().content(), b"APPROVED\n");
}

#[test]
fn save_frozen_result_honors_configured_filemode() {
    for trust_filemode in [false, true] {
        assert_configured_filemode(false, trust_filemode, 0o100755, 0o644);
        assert_configured_filemode(false, trust_filemode, 0o100644, 0o755);
    }
}

#[test]
fn continue_frozen_result_honors_configured_filemode() {
    for trust_filemode in [false, true] {
        assert_configured_filemode(true, trust_filemode, 0o100755, 0o644);
        assert_configured_filemode(true, trust_filemode, 0o100644, 0o755);
    }
}

#[test]
fn frozen_result_uses_theirs_mode_when_ours_is_absent_and_filemode_is_untrusted() {
    let dir = fixture();
    let repo = Repository::open(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.filemode", false)
        .unwrap();
    let path = Path::new("file.txt");
    let mut index = repo.index().unwrap();
    let mut theirs = index.get_path(path, 3).unwrap();
    theirs.mode = 0o100755;
    index.conflict_remove(path).unwrap();
    index.add(&theirs).unwrap();
    let frozen = freeze_text_resolution(&repo, &index, path, "APPROVED\n", Some(0o644)).unwrap();
    assert_eq!(frozen.mode, 0o100755);
    assert_eq!(repo.find_blob(frozen.oid).unwrap().content(), b"APPROVED\n");
}

fn assert_frozen_clean_filters(continue_all: bool) {
    for (attributes, autocrlf, expected) in [
        (None, false, b"APPROVED\r\n".as_slice()),
        (Some("* text=auto\n"), false, b"APPROVED\n".as_slice()),
        (Some("* text eol=lf\n"), false, b"APPROVED\n".as_slice()),
        (None, true, b"APPROVED\n".as_slice()),
    ] {
        let dir = fixture();
        let path = dir.path();
        let repo = Repository::open(path).unwrap();
        repo.config()
            .unwrap()
            .set_bool("core.autocrlf", autocrlf)
            .unwrap();
        if let Some(attributes) = attributes {
            write_file(path, ".gitattributes", attributes);
        }
        let revision = crate::Backend::open(path)
            .unwrap()
            .conflict_snapshot()
            .unwrap()
            .unwrap()
            .observation
            .revision;
        let session = detect_conflict_session(&repo).unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .set_manual_text(Path::new("file.txt"), "APPROVED\r\n")
            .unwrap();
        let interfere = || write_file(path, "file.txt", "UNAPPROVED\r\n");
        if continue_all {
            stage_conflict_resolution_with_worktree_hook(
                &repo, &session, &buffer, &revision, interfere,
            )
            .unwrap();
        } else {
            execute_conflict_save_with_progress(
                &repo,
                &buffer,
                Path::new("file.txt"),
                |progress| {
                    if progress == kagi_domain::conflict_family::ConflictProgress::WorktreeWritten {
                        interfere();
                    }
                },
            )
            .unwrap();
        }
        let entry = repo
            .index()
            .unwrap()
            .get_path(Path::new("file.txt"), 0)
            .unwrap();
        assert_eq!(repo.find_blob(entry.id).unwrap().content(), expected);
        assert_eq!(
            std::fs::read(path.join("file.txt")).unwrap(),
            b"UNAPPROVED\r\n"
        );
    }
}

#[test]
fn save_cleans_frozen_bytes_without_rereading_external_worktree_edits() {
    assert_frozen_clean_filters(false);
}

#[test]
fn continue_cleans_frozen_bytes_without_rereading_external_worktree_edits() {
    assert_frozen_clean_filters(true);
}
