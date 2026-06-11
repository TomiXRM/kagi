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

use kagi::git::{ChangeKind, CommitId, DiffLineKind, commit_changed_files, commit_file_diff, commit_log};

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

// ────────────────────────────────────────────────────────────
// T012 tests: commit_file_diff
// ────────────────────────────────────────────────────────────

// ── Test: modified file — hunk ranges and line contents ──────

#[test]
fn test_file_diff_modified_hunk_content() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Modify base.txt: replace single line.
    write_file(dir, "base.txt", "modified content\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-m", "modify base.txt"]);

    let id = head_commit_id(&repo);
    let path = std::path::Path::new("base.txt");
    let file_diff = commit_file_diff(&repo, &id, path)
        .expect("commit_file_diff failed");

    assert!(!file_diff.is_binary, "text file must not be binary");
    assert!(
        !file_diff.hunks.is_empty(),
        "modified file must have at least one hunk"
    );

    let hunk = &file_diff.hunks[0];
    // old_range: 1 line removed, new_range: 1 line added (no context for single-line file).
    assert_eq!(hunk.old_range.1, 1, "old hunk should span 1 line");
    assert_eq!(hunk.new_range.1, 1, "new hunk should span 1 line");

    // Must have at least one Removed line (old content) and one Added line (new content).
    let removed: Vec<_> = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Removed)
        .collect();
    let added: Vec<_> = hunk
        .lines
        .iter()
        .filter(|l| l.kind == DiffLineKind::Added)
        .collect();

    assert!(!removed.is_empty(), "should have a Removed line");
    assert!(!added.is_empty(), "should have an Added line");

    // The removed line should contain the old content "base".
    assert!(
        removed[0].content.contains("base"),
        "removed line should contain old content 'base', got: {:?}",
        removed[0].content
    );
    // The added line should contain the new content "modified content".
    assert!(
        added[0].content.contains("modified content"),
        "added line should contain new content, got: {:?}",
        added[0].content
    );
}

// ── Test: added file — all lines are Added ───────────────────

#[test]
fn test_file_diff_added_all_lines_added() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    write_file(dir, "new.txt", "line one\nline two\nline three\n");
    git(dir, &["add", "new.txt"]);
    git(dir, &["commit", "-m", "add new.txt with three lines"]);

    let id = head_commit_id(&repo);
    let path = std::path::Path::new("new.txt");
    let file_diff = commit_file_diff(&repo, &id, path)
        .expect("commit_file_diff failed");

    assert!(!file_diff.is_binary, "text file must not be binary");
    assert!(
        !file_diff.hunks.is_empty(),
        "added file must have at least one hunk"
    );

    // Every content line in every hunk must be Added.
    for hunk in &file_diff.hunks {
        for line in &hunk.lines {
            assert_eq!(
                line.kind,
                DiffLineKind::Added,
                "added file: all lines must be Added, got {:?}: {:?}",
                line.kind,
                line.content
            );
        }
    }

    // Count total added lines.
    let total_added: usize = file_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind == DiffLineKind::Added)
        .count();
    assert_eq!(total_added, 3, "should have 3 added lines");
}

// ── Test: binary file — is_binary=true, hunks empty ─────────

#[test]
fn test_file_diff_binary() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Write a file with NUL bytes so git detects it as binary.
    let binary_content: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x00, 0x01, 0x02, 0x03];
    std::fs::write(dir.join("image.bin"), &binary_content).expect("write binary failed");
    git(dir, &["add", "image.bin"]);
    git(dir, &["commit", "-m", "add binary file"]);

    let id = head_commit_id(&repo);
    let path = std::path::Path::new("image.bin");
    let file_diff = commit_file_diff(&repo, &id, path)
        .expect("commit_file_diff failed");

    assert!(
        file_diff.is_binary,
        "binary file must have is_binary=true"
    );
    assert!(
        file_diff.hunks.is_empty(),
        "binary file must have no hunks, got: {:?}",
        file_diff.hunks
    );
}

// ── Test: Japanese file content — no panic ───────────────────

#[test]
fn test_file_diff_japanese_no_panic() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();

    // Write a file with Japanese Unicode content.
    write_file(
        dir,
        "japanese.txt",
        "こんにちは世界\n日本語のテスト\n",
    );
    git(dir, &["add", "japanese.txt"]);
    git(dir, &["commit", "-m", "add Japanese content"]);

    let id = head_commit_id(&repo);
    let path = std::path::Path::new("japanese.txt");

    // Must not panic.
    let file_diff = commit_file_diff(&repo, &id, path)
        .expect("commit_file_diff failed for Japanese content");

    assert!(!file_diff.is_binary, "Japanese UTF-8 file must not be binary");
    assert!(
        !file_diff.hunks.is_empty(),
        "Japanese file must have at least one hunk"
    );

    // All lines should be Added, and content should round-trip without panic.
    let all_added = file_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .all(|l| l.kind == DiffLineKind::Added);
    assert!(all_added, "all lines in a new file must be Added");

    // Verify the Japanese content survived the lossy UTF-8 decode.
    let first_line_content = &file_diff.hunks[0].lines[0].content;
    assert!(
        first_line_content.contains("こんにちは"),
        "Japanese content must survive round-trip, got: {:?}",
        first_line_content
    );
}
