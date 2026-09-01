//! Read-only conflict preview for a PR (ADR-0145).
//!
//! Two properties matter: it reports the conflicts git would actually produce,
//! and it changes nothing. The second is the reason this can be a tab you click
//! by accident.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use kagi_git::{pr_conflict_files, pr_conflict_text, CommitId, PrConflictKind};
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();

    assert_eq!(files.len(), 1, "{files:?}");
    let f = &files[0];
    assert_eq!(f.path, PathBuf::from("shared.txt"));
    assert_eq!(f.kind, PrConflictKind::BothModified);
    let text = pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &f.path)
        .unwrap()
        .expect("text");
    // The marker text is what git would have written, so both sides are in it
    // and the untouched context survives.
    assert!(text.contains("BASE VERSION"), "{text}");
    assert!(text.contains("PR VERSION"), "{text}");
    assert!(text.contains("<<<<<<<"), "{text}");
    assert!(text.contains(">>>>>>>"), "{text}");
    assert!(text.contains("one"), "context is missing");
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    let text = pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &files[0].path)
        .unwrap()
        .expect("text");
    let model = kagi_domain::resolution::HunkModel::from_marker_text(&text);
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, PrConflictKind::DeleteModify);
    assert_eq!(
        pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &files[0].path).unwrap(),
        None,
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].kind, PrConflictKind::BothAdded);
    let text = pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &files[0].path)
        .unwrap()
        .expect("text");
    assert!(text.contains("base's version"));
    assert!(text.contains("pr's version"));
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
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
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

/// The mistake this function invites, and the one that shipped: passing
/// `merge-base(base, head)` instead of the base branch's tip.
///
/// A merge-base is by definition an ancestor of `head`, so merging into it is
/// a fast-forward and *cannot* conflict. Handed one, the preview truthfully
/// reports nothing — and the caller that made the substitution sees "no
/// conflicts" on every PR in the repository, which is exactly what happened.
/// This pins both halves so the difference stays visible.
#[test]
fn the_base_tip_and_the_merge_base_give_different_answers() {
    let (_t, p) = setup();
    write(&p, "shared.txt", "one\nPR\nthree\n");
    git(&p, &["commit", "-qam", "pr"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "shared.txt", "one\nBASE\nthree\n");
    git(&p, &["commit", "-qam", "base"]);

    let repo = Repository::open(&p).unwrap();
    let merge_base = CommitId(out(&p, &["merge-base", "base", "pr"]));
    assert_ne!(
        merge_base.0,
        out(&p, &["rev-parse", "base"]),
        "precondition: the branches must have diverged"
    );

    // The real question: against the branch tip, they conflict.
    let against_tip = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(against_tip.len(), 1, "{against_tip:?}");

    // The vacuous one: against the merge-base, nothing ever conflicts.
    let against_merge_base = pr_conflict_files(&repo, &merge_base, &id(&p, "pr")).unwrap();
    assert!(
        against_merge_base.is_empty(),
        "merging into an ancestor is a fast-forward; got {against_merge_base:?}"
    );
}

/// A binary file conflicting on both sides.
///
/// Reproduction for a hard abort (not a catchable panic — it took the whole
/// app down): libgit2 produces no merged buffer for a binary conflict, and
/// `git2`'s `MergeFileResult::content()` hands the null pointer straight to
/// `slice::from_raw_parts`.
#[test]
fn a_binary_conflict_does_not_abort() {
    let (_t, p) = setup();
    // Two different byte sequences, both with an embedded NUL so git calls
    // them binary.
    std::fs::write(p.join("blob.bin"), [0u8, 1, 2, 3, 0, 9]).unwrap();
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "pr adds a binary"]);
    git(&p, &["checkout", "-q", "base"]);
    std::fs::write(p.join("blob.bin"), [0u8, 9, 8, 7, 0, 1]).unwrap();
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "base adds a different binary"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].path, PathBuf::from("blob.bin"));
    assert_eq!(files[0].kind, PrConflictKind::Binary);
    assert_eq!(
        pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &files[0].path).unwrap(),
        None,
        "a binary conflict has no text — and asking for it used to abort"
    );
}

/// The marker text is the whole file, so the view can show context around a
/// conflict rather than the clashing lines alone — the thing that made the
/// first version impossible to judge from.
#[test]
fn the_marker_text_carries_the_unconflicted_context() {
    let (_t, p) = setup();
    // A file with a lot of quiet context and one clash in the middle.
    let base_body = "keep 1\nkeep 2\nkeep 3\nMIDDLE-base\nkeep 4\nkeep 5\n";
    let pr_body = "keep 1\nkeep 2\nkeep 3\nMIDDLE-pr\nkeep 4\nkeep 5\n";
    write(&p, "ctx.txt", pr_body);
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "pr"]);
    git(&p, &["checkout", "-q", "base"]);
    write(&p, "ctx.txt", base_body);
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "base"]);

    let repo = Repository::open(&p).unwrap();
    let files = pr_conflict_files(&repo, &id(&p, "base"), &id(&p, "pr")).unwrap();
    let f = files.iter().find(|f| f.path.ends_with("ctx.txt")).unwrap();
    let text = pr_conflict_text(&repo, &id(&p, "base"), &id(&p, "pr"), &f.path)
        .unwrap()
        .expect("text");

    let model = kagi_domain::resolution::HunkModel::from_marker_text(&text);
    let passthrough: Vec<String> = model
        .regions
        .iter()
        .filter_map(|r| match r {
            kagi_domain::resolution::Region::Passthrough(l) => Some(l.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    for l in ["keep 1", "keep 2", "keep 3", "keep 4", "keep 5"] {
        assert!(
            passthrough.iter().any(|p| p == l),
            "context line {l:?} is missing: {passthrough:?}"
        );
    }
}
