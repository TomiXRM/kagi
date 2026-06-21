//! Integration tests for the per-file diffstat backend (W16-DIFFSTAT / T-DIFFSTAT-002).
//!
//! Each test builds a small Git repository in a `tempfile::TempDir` via the
//! `git` CLI, then asserts the additions/deletions aggregated by
//! `kagi_git::{commit_diffstat, staged_diffstat, unstaged_diffstat}`.
//!
//! Scenarios: add / modify / delete / binary / rename, for commit-vs-parent,
//! staged (HEAD→index), and unstaged (index→workdir).

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    commit_diffstat, commit_log, find_stat, staged_diffstat, unstaged_diffstat, ChangeKind,
    CommitId,
};

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

fn write_bytes(dir: &Path, name: &str, content: &[u8]) {
    std::fs::write(dir.join(name), content).expect("write_bytes failed");
}

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "base.txt", "l1\nl2\nl3\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Repository::open(dir).expect("failed to open repo")
}

fn head_commit_id(repo: &Repository) -> CommitId {
    let commits = commit_log(repo, 1).expect("commit_log failed");
    commits.into_iter().next().expect("no commits in repo").id
}

// ────────────────────────────────────────────────────────────
// commit_diffstat
// ────────────────────────────────────────────────────────────

#[test]
fn commit_added_file_counts_all_additions() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "added.txt", "x\ny\nz\n");
    git(dir, &["add", "added.txt"]);
    git(dir, &["commit", "-m", "add file"]);

    let stats = commit_diffstat(&repo, &head_commit_id(&repo)).unwrap();
    let s = find_stat(&stats, Path::new("added.txt")).expect("added.txt missing");
    assert_eq!(s.change, ChangeKind::Added);
    assert_eq!(s.additions, 3);
    assert_eq!(s.deletions, 0);
    assert!(!s.is_binary);
}

#[test]
fn commit_modified_file_counts_add_and_del() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // base.txt: l1\nl2\nl3 → change l2, append l4 (1 add of new l2, 1 del old l2, +l4).
    write_file(dir, "base.txt", "l1\nCHANGED\nl3\nl4\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "modify"]);

    let stats = commit_diffstat(&repo, &head_commit_id(&repo)).unwrap();
    let s = find_stat(&stats, Path::new("base.txt")).expect("base.txt missing");
    assert_eq!(s.change, ChangeKind::Modified);
    // l2 → CHANGED is 1 del + 1 add; l4 is 1 add ⇒ +2 -1.
    assert_eq!(s.additions, 2);
    assert_eq!(s.deletions, 1);
    assert!(!s.is_binary);
}

#[test]
fn commit_deleted_file_counts_all_deletions() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["rm", "base.txt"]);
    git(dir, &["commit", "-m", "delete base"]);

    let stats = commit_diffstat(&repo, &head_commit_id(&repo)).unwrap();
    let s = find_stat(&stats, Path::new("base.txt")).expect("base.txt missing");
    assert_eq!(s.change, ChangeKind::Deleted);
    assert_eq!(s.additions, 0);
    assert_eq!(s.deletions, 3);
    assert!(!s.is_binary);
}

#[test]
fn commit_binary_file_is_flagged_zero_counts() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // NUL bytes guarantee git treats this as binary.
    write_bytes(dir, "blob.bin", &[0u8, 159, 146, 150, 0, 1, 2, 3]);
    git(dir, &["add", "blob.bin"]);
    git(dir, &["commit", "-m", "add binary"]);

    let stats = commit_diffstat(&repo, &head_commit_id(&repo)).unwrap();
    let s = find_stat(&stats, Path::new("blob.bin")).expect("blob.bin missing");
    assert!(s.is_binary, "binary file must be flagged");
    assert_eq!(s.additions, 0);
    assert_eq!(s.deletions, 0);
}

#[test]
fn commit_renamed_file_collapses_to_renamed() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["mv", "base.txt", "renamed.txt"]);
    git(dir, &["commit", "-m", "rename"]);

    let stats = commit_diffstat(&repo, &head_commit_id(&repo)).unwrap();
    // New-side path is renamed.txt; change carries the old path.
    let s = find_stat(&stats, Path::new("renamed.txt")).expect("renamed.txt missing");
    match &s.change {
        ChangeKind::Renamed { from } => assert_eq!(from, Path::new("base.txt")),
        other => panic!("expected Renamed, got {other:?}"),
    }
    // Pure rename ⇒ no content change.
    assert_eq!(s.additions, 0);
    assert_eq!(s.deletions, 0);
}

// ────────────────────────────────────────────────────────────
// staged_diffstat / unstaged_diffstat
// ────────────────────────────────────────────────────────────

#[test]
fn staged_diffstat_reports_index_changes() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "staged_new.txt", "a\nb\n");
    git(dir, &["add", "staged_new.txt"]);

    let stats = staged_diffstat(&repo).unwrap();
    let s = find_stat(&stats, Path::new("staged_new.txt")).expect("staged_new.txt missing");
    assert_eq!(s.change, ChangeKind::Added);
    assert_eq!(s.additions, 2);
    assert_eq!(s.deletions, 0);
}

#[test]
fn unstaged_diffstat_reports_workdir_changes() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Modify a tracked file without staging.
    write_file(dir, "base.txt", "l1\nl2\nl3\nl4\nl5\n");

    let stats = unstaged_diffstat(&repo).unwrap();
    let s = find_stat(&stats, Path::new("base.txt")).expect("base.txt missing");
    assert_eq!(s.change, ChangeKind::Modified);
    // Two lines appended ⇒ +2 -0.
    assert_eq!(s.additions, 2);
    assert_eq!(s.deletions, 0);
}

#[test]
fn unstaged_diffstat_excludes_untracked_files() {
    // Untracked files are intentionally NOT diffstatted: line stats require
    // reading every file, which made a bulk untracked drop (e.g. 300 images)
    // freeze the UI on each reload. They show in the commit panel as new ("A")
    // without a +/- bar instead. Only tracked modifications get bars.
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "untracked.txt", "one\ntwo\nthree\n");

    let stats = unstaged_diffstat(&repo).unwrap();
    assert!(
        find_stat(&stats, Path::new("untracked.txt")).is_none(),
        "untracked files must be excluded from unstaged_diffstat (perf)"
    );
}
