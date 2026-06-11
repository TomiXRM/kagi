//! Integration tests for commit_changed_files (T011).
//!
//! Each test builds a small Git repository in a `tempfile::TempDir` using the
//! `git` CLI, then asserts the result of `kagi::git::commit_changed_files`.
//!
//! Scenarios covered:
//! - root commit (all files Added)
//! - added file
//! - modified file
//! - deleted file
//! - renamed file (via `git mv` + `find_similar`)
//! - merge commit (first-parent diff only; second-parent changes excluded)

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{ChangeKind, CommitId, commit_changed_files, commit_log};

// ────────────────────────────────────────────────────────────
// Test helpers
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
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

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Initialise a repo and make an initial commit, return Repository.
fn init_repo(tmp: &TempDir) -> Repository {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "initial commit"]);

    Repository::open(dir).expect("failed to open repo")
}

/// Return the HEAD commit id by reading the repo log.
fn head_commit_id(repo: &Repository) -> CommitId {
    let commits = commit_log(repo, 1).expect("commit_log failed");
    commits.into_iter().next().expect("no commits in repo").id
}

// ────────────────────────────────────────────────────────────
// Test: root commit — all files are Added
// ────────────────────────────────────────────────────────────

#[test]
fn test_root_commit_all_added() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    write_file(dir, "a.txt", "a\n");
    write_file(dir, "b.txt", "b\n");
    git(dir, &["add", "a.txt", "b.txt"]);
    git(dir, &["commit", "-m", "root commit"]);

    let repo = Repository::open(dir).expect("failed to open repo");
    let id = head_commit_id(&repo);

    let files = commit_changed_files(&repo, &id).expect("commit_changed_files failed");

    assert_eq!(files.len(), 2, "root commit should have 2 added files");
    for f in &files {
        assert_eq!(
            f.change,
            ChangeKind::Added,
            "root commit file should be Added, got {:?} for {:?}",
            f.change,
            f.path
        );
    }
}

// ────────────────────────────────────────────────────────────
// Test: added file
// ────────────────────────────────────────────────────────────

#[test]
fn test_added_file() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "new.txt", "new content\n");
    git(dir, &["add", "new.txt"]);
    git(dir, &["commit", "-m", "add new.txt"]);

    let id = head_commit_id(&repo);
    let files = commit_changed_files(&repo, &id).expect("commit_changed_files failed");

    assert_eq!(files.len(), 1, "should have exactly 1 changed file");
    assert_eq!(files[0].path.to_str().unwrap(), "new.txt");
    assert_eq!(files[0].change, ChangeKind::Added);
}

// ────────────────────────────────────────────────────────────
// Test: modified file
// ────────────────────────────────────────────────────────────

#[test]
fn test_modified_file() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "base.txt", "modified content\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "modify base.txt"]);

    let id = head_commit_id(&repo);
    let files = commit_changed_files(&repo, &id).expect("commit_changed_files failed");

    assert_eq!(files.len(), 1, "should have exactly 1 changed file");
    assert_eq!(files[0].path.to_str().unwrap(), "base.txt");
    assert_eq!(files[0].change, ChangeKind::Modified);
}

// ────────────────────────────────────────────────────────────
// Test: deleted file
// ────────────────────────────────────────────────────────────

#[test]
fn test_deleted_file() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    git(dir, &["rm", "base.txt"]);
    git(dir, &["commit", "-m", "delete base.txt"]);

    let id = head_commit_id(&repo);
    let files = commit_changed_files(&repo, &id).expect("commit_changed_files failed");

    assert_eq!(files.len(), 1, "should have exactly 1 changed file");
    assert_eq!(files[0].path.to_str().unwrap(), "base.txt");
    assert_eq!(files[0].change, ChangeKind::Deleted);
}

// ────────────────────────────────────────────────────────────
// Test: renamed file
// ────────────────────────────────────────────────────────────

#[test]
fn test_renamed_file() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Use git mv so the similarity is 100% and find_similar detects the rename.
    git(dir, &["mv", "base.txt", "renamed.txt"]);
    git(dir, &["commit", "-m", "rename base.txt to renamed.txt"]);

    let id = head_commit_id(&repo);
    let files = commit_changed_files(&repo, &id).expect("commit_changed_files failed");

    assert_eq!(
        files.len(),
        1,
        "renamed file should be 1 entry (not 2), got: {:?}",
        files
    );
    assert_eq!(files[0].path.to_str().unwrap(), "renamed.txt");
    match &files[0].change {
        ChangeKind::Renamed { from } => {
            assert_eq!(
                from.to_str().unwrap(),
                "base.txt",
                "Renamed.from should be base.txt, got {:?}",
                from
            );
        }
        other => panic!("expected Renamed, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────
// Test: merge commit — only first-parent diff
// ────────────────────────────────────────────────────────────
//
// Topology:
//   A (base.txt)
//   B (on main: modify base.txt)  ← first parent of M
//   C (on feature: add feature.txt) ← second parent of M
//   M (merge commit)
//
// Expected diff of M vs B (first parent):
//   - feature.txt Added  (brought in from the feature branch)
// NOT expected:
//   - base.txt Modified  (that change is in B vs A, not M vs B)
//
// ────────────────────────────────────────────────────────────

#[test]
fn test_merge_commit_first_parent_only() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    // A: initial commit
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "A: initial"]);

    // B: modify base.txt on main
    write_file(dir, "base.txt", "base modified\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "B: modify base.txt"]);

    // branch off from A for feature work
    // We need to branch from the initial commit, so find A's sha
    let repo = Repository::open(dir).expect("repo open failed");
    let all_commits = commit_log(&repo, 10_000).expect("commit_log failed");
    let a_id = all_commits
        .iter()
        .find(|c| c.summary == "A: initial")
        .map(|c| c.id.0.clone())
        .expect("A commit not found");
    drop(all_commits);
    drop(repo);

    // C: create feature branch from A, add feature.txt
    git(dir, &["checkout", "-b", "feature", &a_id]);
    write_file(dir, "feature.txt", "feature content\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-m", "C: add feature.txt"]);

    // M: merge feature into main (first parent = B)
    git(dir, &["checkout", "main"]);
    git(dir, &["merge", "--no-ff", "feature", "-m", "M: merge feature"]);

    let repo = Repository::open(dir).expect("repo open failed");
    let commits = commit_log(&repo, 10_000).expect("commit_log failed");
    let merge_commit = commits
        .iter()
        .find(|c| c.summary.starts_with("M: merge feature"))
        .expect("merge commit not found");
    assert_eq!(
        merge_commit.parents.len(),
        2,
        "merge commit must have 2 parents"
    );

    let files = commit_changed_files(&repo, &merge_commit.id)
        .expect("commit_changed_files failed");

    // Only feature.txt should appear (the diff of M vs its first parent B).
    // base.txt was modified in B vs A, but M vs B shows it as unchanged.
    assert_eq!(
        files.len(),
        1,
        "merge commit first-parent diff should have 1 file, got: {:?}",
        files
    );
    assert_eq!(files[0].path.to_str().unwrap(), "feature.txt");
    assert_eq!(files[0].change, ChangeKind::Added);
}
