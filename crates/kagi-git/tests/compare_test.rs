//! Integration tests for ADR-0026 compare backends.
//!
//! All repositories are built in tempdirs with `git init -b main`.  The compare
//! APIs are read-only, so each test asserts `git status --porcelain` is
//! unchanged before and after the call.

use std::path::Path;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    compare_commit_to_workdir, compare_commit_to_workdir_file_diff, compare_commits,
    compare_file_diff, ChangeKind, CommitId, DiffLineKind,
};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_command, git_output, write_file};

fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git_fixture::init_repo(dir, "main");

    write_file(dir, "base.txt", "one\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial"]);

    Repository::open(dir).expect("failed to open repo")
}

fn head_id(dir: &Path) -> CommitId {
    CommitId(git_output(dir, &["rev-parse", "HEAD"]))
}

/// Untrimmed `git status --porcelain`: callers match whole lines such as
/// `" M base.txt"`, whose leading status column a trim would strip.
fn status_porcelain(dir: &Path) -> String {
    let output = git_command(dir)
        .args(["status", "--porcelain"])
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git status --porcelain exited with {:?}",
        output.status.code()
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn compare_commits_lists_files_and_diff_without_mutation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();
    let base = head_id(dir);

    write_file(dir, "base.txt", "two\n");
    write_file(dir, "added.txt", "new\n");
    git(dir, &["add", "base.txt", "added.txt"]);
    git(dir, &["commit", "-m", "update files"]);
    let head = head_id(dir);

    let before = status_porcelain(dir);
    let files = compare_commits(&repo, &base, &head).expect("compare_commits failed");
    let after = status_porcelain(dir);
    assert_eq!(before, after, "compare_commits must be read-only");

    assert_eq!(files.len(), 2);
    assert!(files
        .iter()
        .any(|f| { f.path == Path::new("base.txt") && f.change == ChangeKind::Modified }));
    assert!(files
        .iter()
        .any(|f| { f.path == Path::new("added.txt") && f.change == ChangeKind::Added }));

    let diff = compare_file_diff(&repo, &base, &head, Path::new("base.txt"))
        .expect("compare_file_diff failed");
    let added = diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .any(|l| l.kind == DiffLineKind::Added && l.content == "two\n");
    let removed = diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .any(|l| l.kind == DiffLineKind::Removed && l.content == "one\n");
    assert!(added, "new content should appear in compare diff");
    assert!(removed, "old content should appear in compare diff");
    assert_eq!(before, status_porcelain(dir), "file diff must be read-only");
}

#[test]
fn compare_commit_to_workdir_includes_staged_unstaged_untracked_without_mutation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();
    let base = head_id(dir);

    write_file(dir, "base.txt", "worktree\n");
    write_file(dir, "staged.txt", "staged\n");
    git(dir, &["add", "staged.txt"]);
    write_file(dir, "untracked.txt", "untracked\n");

    let before = status_porcelain(dir);
    assert!(
        before.contains(" M base.txt"),
        "fixture should be unstaged dirty"
    );
    assert!(
        before.contains("A  staged.txt"),
        "fixture should be staged dirty"
    );
    assert!(
        before.contains("?? untracked.txt"),
        "fixture should have untracked file"
    );

    let files = compare_commit_to_workdir(&repo, &base).expect("compare_commit_to_workdir failed");
    let after = status_porcelain(dir);
    assert_eq!(before, after, "workdir compare must be read-only");

    assert!(files
        .iter()
        .any(|f| { f.path == Path::new("base.txt") && f.change == ChangeKind::Modified }));
    assert!(files
        .iter()
        .any(|f| { f.path == Path::new("staged.txt") && f.change == ChangeKind::Added }));
    assert!(files
        .iter()
        .any(|f| { f.path == Path::new("untracked.txt") && f.change == ChangeKind::Added }));

    let diff = compare_commit_to_workdir_file_diff(&repo, &base, Path::new("untracked.txt"))
        .expect("compare workdir file diff failed");
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .any(|l| l.kind == DiffLineKind::Added && l.content == "untracked\n"),
        "untracked file content should appear in workdir compare diff"
    );
    assert_eq!(
        before,
        status_porcelain(dir),
        "workdir file diff must be read-only"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
