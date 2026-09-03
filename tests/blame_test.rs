//! Integration tests for line-level blame (issue #350, ADR-0162).
//!
//! Each test builds a small Git repository in a `tempfile::TempDir` with the
//! `git` CLI, then asserts `Backend::blame_file` per-line attribution and
//! `.git-blame-ignore-revs` handling (auto-detect → mark + count).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use kagi_git::Backend;

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
    assert!(status.success(), "git {} failed", args.join(" "));
}

/// Capture stdout of a git command (trimmed).
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(out.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn init_repo(tmp: &TempDir) -> (Backend, std::path::PathBuf) {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    (
        Backend::open(dir).expect("failed to open repo"),
        dir.to_path_buf(),
    )
}

#[test]
fn attributes_each_line_to_its_commit() {
    let tmp = TempDir::new().unwrap();
    let (backend, dir) = init_repo(&tmp);

    // Commit 1: two lines.
    write_file(&dir, "f.txt", "alpha\nbeta\n");
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-m", "add alpha and beta"]);
    let c1 = git_out(&dir, &["rev-parse", "HEAD"]);

    // Commit 2: append a third line only.
    write_file(&dir, "f.txt", "alpha\nbeta\ngamma\n");
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-m", "add gamma"]);
    let c2 = git_out(&dir, &["rev-parse", "HEAD"]);

    let blame = backend
        .blame_file(Path::new("f.txt"))
        .expect("blame failed");
    assert_eq!(blame.lines.len(), 3, "three attributed lines");

    // Lines 1 & 2 come from commit 1, line 3 from commit 2.
    assert_eq!(blame.lines[0].line_no, 1);
    assert_eq!(blame.lines[0].commit, c1);
    assert_eq!(blame.lines[1].commit, c1);
    assert_eq!(blame.lines[2].line_no, 3);
    assert_eq!(blame.lines[2].commit, c2);
    assert_eq!(blame.lines[2].summary, "add gamma");
    assert_eq!(blame.lines[2].author, "Test");

    // No ignore file → nothing ignored, no markers.
    assert_eq!(blame.ignored_revs, 0);
    assert!(blame.lines.iter().all(|l| l.mark().is_none()));
}

#[test]
fn ignore_revs_file_marks_and_counts_reformatting_commit() {
    let tmp = TempDir::new().unwrap();
    let (backend, dir) = init_repo(&tmp);

    // Commit 1: original content.
    write_file(&dir, "src.txt", "one\ntwo\n");
    git(&dir, &["add", "src.txt"]);
    git(&dir, &["commit", "-m", "initial"]);

    // Commit 2: a "bulk reformat" that rewrites every line (e.g. rustfmt).
    write_file(&dir, "src.txt", "  one  \n  two  \n");
    git(&dir, &["add", "src.txt"]);
    git(&dir, &["commit", "-m", "reformat: whitespace"]);
    let fmt_commit = git_out(&dir, &["rev-parse", "HEAD"]);

    // Without an ignore file, both lines blame to the reformat commit and
    // nothing is marked.
    let before = backend.blame_file(Path::new("src.txt")).expect("blame");
    assert!(before.lines.iter().all(|l| l.commit == fmt_commit));
    assert_eq!(before.ignored_revs, 0);
    assert!(before.lines.iter().all(|l| !l.ignored));

    // Add .git-blame-ignore-revs listing the formatting commit (with a comment).
    write_file(
        &dir,
        ".git-blame-ignore-revs",
        &format!("# bulk reformat\n{fmt_commit}\n"),
    );

    let after = backend.blame_file(Path::new("src.txt")).expect("blame");
    // Every line's commit is still the formatting commit (v1 marks, does not
    // re-attribute), but now they are flagged ignored...
    assert!(after.lines.iter().all(|l| l.ignored));
    // ...distinguished without colour: the `*` marker symbol.
    assert!(after.lines.iter().all(|l| l.mark() == Some('*')));
    // ...and the "N revisions ignored" count surfaces the single formatting rev.
    assert_eq!(after.ignored_revs, 1);
}
