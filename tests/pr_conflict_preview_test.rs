//! Read-only conflict preview for a PR (ADR-0145).
//!
//! Two properties matter: it reports the conflicts git would actually produce,
//! and it changes nothing. The second is the reason this can be a tab you click
//! by accident.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use kagi_git::{pr_conflict_preview, CommitId, PrConflictKind};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e.com")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn out(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(dir: &Path, n: &str, b: &str) {
    std::fs::write(dir.join(n), b).unwrap();
}

fn id(dir: &Path, rev: &str) -> CommitId {
    CommitId(out(dir, &["rev-parse", rev]))
}

/// `base` and `pr` diverge. What each does is set up per-test.
fn setup() -> (TempDir, PathBuf) {
    let t = TempDir::new().unwrap();
    let p = t.path().to_path_buf();
    git(&p, &["init", "-q", "-b", "base", "."]);
    git(&p, &["config", "user.name", "T"]);
    git(&p, &["config", "user.email", "t@e.com"]);
    git(&p, &["config", "commit.gpgsign", "false"]);
    write(&p, "shared.txt", "one\ntwo\nthree\n");
    write(&p, "quiet.txt", "untouched\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "root"]);
    git(&p, &["checkout", "-q", "-b", "pr"]);
    (t, p)
}

#[test]
fn a_clean_merge_reports_no_conflicts() {
    let (_t, p) = setup();
    write(&p, "new.txt", "added by the pr\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "pr adds a file"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert!(files.is_empty(), "expected no conflicts, got {files:?}");
}

#[test]
fn both_modified_reports_the_file_and_the_marker_text() {
    let (_t, p) = setup();
    write(&p, "shared.txt", "one\nPR VERSION\nthree\n");
    git(&p, &["commit", "-qam", "pr edits"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "shared.txt", "one\nBASE VERSION\nthree\n");
    git(&p, &["commit", "-qam", "base edits"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();

    assert_eq!(files.len(), 1, "{files:?}");
    let f = &files[0];
    assert_eq!(f.path, PathBuf::from("shared.txt"));
    assert_eq!(f.kind, PrConflictKind::BothModified);
    // The marker text is what git would have written, so both sides are in it
    // and the untouched context survives.
    assert!(f.marker_text.contains("BASE VERSION"), "{}", f.marker_text);
    assert!(f.marker_text.contains("PR VERSION"), "{}", f.marker_text);
    assert!(f.marker_text.contains("<<<<<<<"), "{}", f.marker_text);
    assert!(f.marker_text.contains(">>>>>>>"), "{}", f.marker_text);
    assert!(f.marker_text.contains("one"), "context is missing");
    // The file neither side touched must not be reported.
    assert!(!files.iter().any(|f| f.path == Path::new("quiet.txt")));
}

/// The marker text must be parseable by the same model the conflict editor
/// uses — otherwise the tab can only show a wall of text.
#[test]
fn the_marker_text_parses_into_hunks() {
    let (_t, p) = setup();
    write(&p, "shared.txt", "one\nPR\nthree\n");
    git(&p, &["commit", "-qam", "pr"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "shared.txt", "one\nBASE\nthree\n");
    git(&p, &["commit", "-qam", "base"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    let model = kagi_domain::resolution::HunkModel::from_marker_text(&files[0].marker_text);
    let hunks: Vec<_> = model
        .regions
        .iter()
        .filter_map(|r| match r {
            kagi_domain::resolution::Region::Hunk(h) => Some(h),
            _ => None,
        })
        .collect();
    assert_eq!(hunks.len(), 1, "{:?}", model.regions);
    assert_eq!(hunks[0].current, vec!["BASE".to_string()]);
    assert_eq!(hunks[0].incoming, vec!["PR".to_string()]);
}

#[test]
fn delete_versus_modify_is_reported_without_text() {
    let (_t, p) = setup();
    write(&p, "shared.txt", "one\nPR\nthree\n");
    git(&p, &["commit", "-qam", "pr edits"]);
    git(&p, &["checkout", "-q", "base"]);
    git(&p, &["rm", "-q", "shared.txt"]);
    git(&p, &["commit", "-qm", "base deletes"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, PrConflictKind::DeleteModify);
    assert!(
        files[0].marker_text.is_empty(),
        "a deleted side has no three-way text to show"
    );
}

#[test]
fn both_added_is_reported() {
    let (_t, p) = setup();
    write(&p, "same.txt", "pr's version\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "pr adds"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "same.txt", "base's version\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "base adds"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, PrConflictKind::BothAdded);
    assert!(files[0].marker_text.contains("base's version"));
    assert!(files[0].marker_text.contains("pr's version"));
}

/// The whole reason this is safe to open: it is a question, not a state change.
#[test]
fn previewing_changes_nothing_at_all() {
    let (_t, p) = setup();
    write(&p, "shared.txt", "one\nPR\nthree\n");
    git(&p, &["commit", "-qam", "pr"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "shared.txt", "one\nBASE\nthree\n");
    git(&p, &["commit", "-qam", "base"]);
    // An uncommitted edit, the thing most at risk from an accidental merge.
    write(&p, "quiet.txt", "MY UNCOMMITTED WORK\n");

    let head = out(&p, &["rev-parse", "HEAD"]);
    let status = out(&p, &["status", "--porcelain"]);
    let refs = out(&p, &["show-ref"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_preview(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert!(!files.is_empty(), "precondition: the merge must conflict");

    assert_eq!(out(&p, &["rev-parse", "HEAD"]), head, "HEAD moved");
    assert_eq!(
        out(&p, &["status", "--porcelain"]),
        status,
        "status changed"
    );
    assert_eq!(out(&p, &["show-ref"]), refs, "a ref moved");
    assert_eq!(
        std::fs::read_to_string(p.join("quiet.txt")).unwrap(),
        "MY UNCOMMITTED WORK\n",
        "uncommitted work was touched"
    );
    assert!(
        !p.join(".git/MERGE_HEAD").exists(),
        "a merge state was entered"
    );
    // Checked against `show-ref` above rather than the object database: the
    // both-added path does write one unreferenced empty blob, which `git gc`
    // collects and no command surfaces. Everything a user can observe is
    // unchanged, which is the property being claimed.
}
