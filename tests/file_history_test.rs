//! Integration tests for the File History data layer (ADR-0089).
//!
//! These shell out to the real `git` binary into a `TempDir` and exercise
//! `kagi::git::file_history` end-to-end (add / modify / rename / delete /
//! binary / WIP / unicode paths / limit).  No real repository is ever touched.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use kagi::git::{file_history, FileChangeType, FileHistoryEntryKind, FileHistoryRequest};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

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

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn init_repo(tmp: &TempDir) -> PathBuf {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);
    d.to_path_buf()
}

fn req(dir: &Path, path: &str, follow: bool, wip: bool, limit: usize) -> FileHistoryRequest {
    FileHistoryRequest {
        repo_dir: dir.to_path_buf(),
        file_path: PathBuf::from(path),
        follow_renames: follow,
        include_wip: wip,
        limit,
    }
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[test]
fn add_then_modify_twice_newest_first() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "foo.txt", "a\n");
    git(&d, &["add", "foo.txt"]);
    git(&d, &["commit", "-qm", "add foo"]);

    write_file(&d, "foo.txt", "a\nb\n");
    git(&d, &["add", "foo.txt"]);
    git(&d, &["commit", "-qm", "modify foo 1"]);

    write_file(&d, "foo.txt", "a\nb\nc\n");
    git(&d, &["add", "foo.txt"]);
    git(&d, &["commit", "-qm", "modify foo 2"]);

    let h = file_history(&req(&d, "foo.txt", true, false, 0)).expect("file_history");
    assert_eq!(h.entries.len(), 3);
    assert_eq!(h.current_path, PathBuf::from("foo.txt"));

    // newest-first
    assert_eq!(
        h.entries[0].commit.as_ref().unwrap().subject,
        "modify foo 2"
    );
    assert_eq!(h.entries[0].change.change_type, FileChangeType::Modified);
    assert_eq!(
        h.entries[1].commit.as_ref().unwrap().subject,
        "modify foo 1"
    );
    assert_eq!(h.entries[1].change.change_type, FileChangeType::Modified);
    assert_eq!(h.entries[2].commit.as_ref().unwrap().subject, "add foo");
    assert_eq!(h.entries[2].change.change_type, FileChangeType::Added);

    // All commit entries.
    assert!(h
        .entries
        .iter()
        .all(|e| e.kind == FileHistoryEntryKind::Commit));
}

#[test]
fn insertions_deletions_parsed() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "foo.txt", "1\n2\n3\n");
    git(&d, &["add", "foo.txt"]);
    git(&d, &["commit", "-qm", "add"]);

    // Remove one line, add two → +2 -1 relative to previous.
    write_file(&d, "foo.txt", "1\n3\nx\ny\n");
    git(&d, &["add", "foo.txt"]);
    git(&d, &["commit", "-qm", "edit"]);

    let h = file_history(&req(&d, "foo.txt", true, false, 0)).expect("file_history");
    let edit = &h.entries[0].change;
    // line "2" removed (-1); lines "x" and "y" added (+2).
    assert_eq!(edit.insertions, Some(2));
    assert_eq!(edit.deletions, Some(1));
    assert!(!edit.is_binary);

    let add = &h.entries[1].change;
    assert_eq!(add.insertions, Some(3));
    assert_eq!(add.deletions, Some(0));
}

#[test]
fn rename_followed_with_before_after() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "old.txt", "content\nline2\n");
    git(&d, &["add", "old.txt"]);
    git(&d, &["commit", "-qm", "add old"]);

    git(&d, &["mv", "old.txt", "new.txt"]);
    git(&d, &["commit", "-qm", "rename old to new"]);

    let h = file_history(&req(&d, "new.txt", true, false, 0)).expect("file_history");
    assert_eq!(h.entries.len(), 2, "history should span both names");
    assert_eq!(h.current_path, PathBuf::from("new.txt"));

    let rename = &h.entries[0];
    assert_eq!(rename.change.change_type, FileChangeType::Renamed);
    assert_eq!(rename.change.path_before, Some(PathBuf::from("old.txt")));
    assert_eq!(rename.change.path_after, PathBuf::from("new.txt"));

    // The pre-rename commit is still present (the Added under the old name).
    assert_eq!(h.entries[1].change.change_type, FileChangeType::Added);
    assert_eq!(h.entries[1].change.path_after, PathBuf::from("old.txt"));
}

#[test]
fn delete_entry_present() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "doomed.txt", "bye\n");
    git(&d, &["add", "doomed.txt"]);
    git(&d, &["commit", "-qm", "add doomed"]);

    git(&d, &["rm", "-q", "doomed.txt"]);
    git(&d, &["commit", "-qm", "delete doomed"]);

    let h = file_history(&req(&d, "doomed.txt", true, false, 0)).expect("file_history");
    assert!(
        h.entries
            .iter()
            .any(|e| e.change.change_type == FileChangeType::Deleted),
        "expected a Deleted entry, got: {:?}",
        h.entries
            .iter()
            .map(|e| e.change.change_type)
            .collect::<Vec<_>>()
    );
    assert_eq!(h.entries[0].change.change_type, FileChangeType::Deleted);
}

#[test]
fn binary_change_flags_is_binary() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    std::fs::write(d.join("blob.bin"), [0u8, 1, 2, 3, 0, 255, 10, 0]).expect("write binary");
    git(&d, &["add", "blob.bin"]);
    git(&d, &["commit", "-qm", "add binary"]);

    let h = file_history(&req(&d, "blob.bin", true, false, 0)).expect("file_history");
    let change = &h.entries[0].change;
    assert!(change.is_binary, "binary file should be flagged");
    assert_eq!(change.insertions, None);
    assert_eq!(change.deletions, None);
}

#[test]
fn wip_entry_at_top() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "live.txt", "committed\n");
    git(&d, &["add", "live.txt"]);
    git(&d, &["commit", "-qm", "add live"]);

    // Uncommitted modification.
    write_file(&d, "live.txt", "committed\nuncommitted\n");

    let h = file_history(&req(&d, "live.txt", true, true, 0)).expect("file_history");
    assert_eq!(h.entries.len(), 2, "WIP + 1 commit");
    assert_eq!(h.entries[0].kind, FileHistoryEntryKind::Wip);
    assert!(h.entries[0].commit.is_none());
    assert_eq!(h.entries[0].change.change_type, FileChangeType::Modified);
    assert_eq!(h.entries[1].kind, FileHistoryEntryKind::Commit);
}

#[test]
fn no_wip_when_clean() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "clean.txt", "x\n");
    git(&d, &["add", "clean.txt"]);
    git(&d, &["commit", "-qm", "add clean"]);

    let h = file_history(&req(&d, "clean.txt", true, true, 0)).expect("file_history");
    assert_eq!(h.entries.len(), 1);
    assert_eq!(h.entries[0].kind, FileHistoryEntryKind::Commit);
}

#[test]
fn unicode_and_space_path() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    let name = "my file 日本語.txt";
    write_file(&d, name, "hello\n");
    git(&d, &["add", name]);
    git(&d, &["commit", "-qm", "add unicode"]);

    write_file(&d, name, "hello\nworld\n");
    git(&d, &["add", name]);
    git(&d, &["commit", "-qm", "edit unicode"]);

    let h = file_history(&req(&d, name, true, false, 0)).expect("file_history");
    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.current_path, PathBuf::from(name));
    assert_eq!(h.entries[0].change.path_after, PathBuf::from(name));
}

#[test]
fn limit_respected() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    for i in 0..5 {
        write_file(&d, "many.txt", &format!("rev {i}\n"));
        git(&d, &["add", "many.txt"]);
        git(&d, &["commit", "-qm", &format!("commit {i}")]);
    }

    let h = file_history(&req(&d, "many.txt", true, false, 2)).expect("file_history");
    assert_eq!(h.entries.len(), 2, "limit should cap commit entries");
    assert_eq!(h.entries[0].commit.as_ref().unwrap().subject, "commit 4");
}

#[test]
fn commit_summary_fields_populated() {
    let tmp = TempDir::new().unwrap();
    let d = init_repo(&tmp);

    write_file(&d, "meta.txt", "data\n");
    git(&d, &["add", "meta.txt"]);
    git(
        &d,
        &[
            "commit",
            "-qm",
            "subject line\n\nbody paragraph\nsecond body line",
        ],
    );

    let h = file_history(&req(&d, "meta.txt", true, false, 0)).expect("file_history");
    let c = h.entries[0].commit.as_ref().unwrap();
    assert_eq!(c.subject, "subject line");
    assert_eq!(c.body.as_deref(), Some("body paragraph\nsecond body line"));
    assert_eq!(c.author_name, "Test");
    assert_eq!(c.author_email, "test@example.com");
    assert_eq!(c.committer_name, "Test");
    assert_eq!(c.full_hash.len(), 40);
    assert!(!c.short_hash.is_empty());
    assert!(c.author_date.starts_with("20"));
}
