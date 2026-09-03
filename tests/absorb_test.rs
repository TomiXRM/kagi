//! Integration tests for absorb — issue #345 / ADR-0151.
//!
//! Absorb folds uncommitted working-tree hunks into the mutable ancestor commit
//! that last touched those lines; ambiguous / immutable hunks stay in the tree.
//! All repos are created inside `TempDir`s (no network; the "pushed" case uses a
//! local bare remote).
//!
//! | # | Name | Acceptance criterion |
//! |---|------|----------------------|
//! | 1 | `test_absorb_single_hunk_to_correct_ancestor` | single hunk → correct ancestor; plan table; post-execute hunk gone from tree & present in target |
//! | 2 | `test_absorb_pushed_commit_never_target` | a pushed commit is never a target |
//! | 3 | `test_absorb_ambiguous_hunk_stays` | pure-addition hunk stays in the working tree |
//! | 4 | `test_absorb_recorded_in_oplog` | execute is recorded in the oplog |
//! | 5 | `test_absorb_preflight_head_moved` | HEAD moved since plan → preflight refuses |
//! | 6 | `test_absorb_protected_branch_blocked` | protected branch → blocker |

use std::path::Path;
use std::process::Command;

use kagi_domain::absorb::{HunkDisposition, KeepReason};
use kagi_git::{execute_absorb, plan_absorb, preflight_absorb, Backend};

const WINDOW: usize = 10;

// ── helpers ──────────────────────────────────────────────────

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
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write failed");
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

/// Content of `name` as of `rev`.
fn show(dir: &Path, rev: &str, name: &str) -> String {
    let out = Command::new("git")
        .args(["show", &format!("{rev}:{name}")])
        .current_dir(dir)
        .output()
        .expect("git show failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn status_porcelain(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .expect("git status failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Init a repo on non-protected branch `work` with an initial commit, so absorb
/// (which refuses protected branches) is allowed.
fn init_repo(dir: &Path) {
    git(dir, &["init", "-b", "work", "-q"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
}

// ── tests ────────────────────────────────────────────────────

#[test]
fn test_absorb_single_hunk_to_correct_ancestor() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    // Commit A: create a.txt (target for the later edit).
    write(dir, "a.txt", "alpha\nbeta\ngamma\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);
    let a_oid = rev(dir, "HEAD");

    // Commit B: unrelated file.
    write(dir, "b.txt", "one\ntwo\n");
    git(dir, &["add", "b.txt"]);
    git(dir, &["commit", "-qm", "add b.txt"]);

    // Uncommitted edit to a line owned by commit A.
    write(dir, "a.txt", "alpha\nBETA\ngamma\n");

    let repo = git2::Repository::open(dir).unwrap();
    let plan = plan_absorb(&repo, WINDOW).unwrap();

    // Distribution table: exactly one hunk, absorbed into commit A.
    assert_eq!(plan.assignments.len(), 1, "one hunk expected");
    assert_eq!(plan.absorb_count(), 1);
    assert_eq!(plan.targets_rewritten(), 1);
    assert!(!plan.has_blockers(), "blockers: {:?}", plan.blockers);
    let target = plan.assignments[0].target().expect("absorbed");
    assert_eq!(
        target.oid, a_oid,
        "hunk must map to the ancestor that owns the line"
    );

    execute_absorb(&repo, &plan).unwrap();

    // Post-execute: working tree clean (the hunk left the tree)...
    assert_eq!(status_porcelain(dir), "", "working tree should be clean");
    // ...and the edit is now inside the (rewritten) ancestor commit A == HEAD~1.
    assert_eq!(show(dir, "HEAD~1", "a.txt"), "alpha\nBETA\ngamma\n");
    // b.txt was replayed unchanged and still present at HEAD.
    assert_eq!(show(dir, "HEAD", "b.txt"), "one\ntwo\n");
}

#[test]
fn test_absorb_pushed_commit_never_target() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    // Commit A, then publish it to a bare remote so A is "pushed".
    write(dir, "a.txt", "alpha\nbeta\ngamma\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);
    let a_oid = rev(dir, "HEAD");

    let remote = tempfile::tempdir().unwrap();
    git(
        dir,
        &["init", "--bare", "-q", remote.path().to_str().unwrap()],
    );
    git(
        dir,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(dir, &["push", "-q", "-u", "origin", "work"]);

    // Commit B locally (NOT pushed) — a mutable target.
    write(dir, "b.txt", "one\ntwo\n");
    git(dir, &["add", "b.txt"]);
    git(dir, &["commit", "-qm", "add b.txt"]);
    let b_oid = rev(dir, "HEAD");

    // Edit a line owned by the PUSHED commit A, and a line owned by unpushed B.
    write(dir, "a.txt", "alpha\nBETA\ngamma\n");
    write(dir, "b.txt", "ONE\ntwo\n");

    let repo = git2::Repository::open(dir).unwrap();
    let plan = plan_absorb(&repo, WINDOW).unwrap();

    // No absorbed hunk may target the pushed commit A.
    for a in plan.absorbed() {
        assert_ne!(
            a.target().unwrap().oid,
            a_oid,
            "pushed commit A must never be an absorb target"
        );
    }
    // The a.txt hunk is kept as immutable; the b.txt hunk absorbs into B.
    let a_row = plan.assignments.iter().find(|r| r.file == "a.txt").unwrap();
    assert_eq!(
        a_row.disposition,
        HunkDisposition::Keep(KeepReason::Immutable),
        "hunk on pushed commit stays in the tree"
    );
    let b_row = plan.assignments.iter().find(|r| r.file == "b.txt").unwrap();
    assert_eq!(b_row.target().unwrap().oid, b_oid);

    execute_absorb(&repo, &plan).unwrap();

    // A's edit stays uncommitted; B's edit was folded in.
    assert!(
        status_porcelain(dir).contains("a.txt"),
        "pushed-commit edit must remain in the working tree"
    );
    assert!(!status_porcelain(dir).contains("b.txt"));
    assert_eq!(show(dir, "HEAD", "b.txt"), "ONE\ntwo\n");
}

#[test]
fn test_absorb_ambiguous_hunk_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    // A file long enough that a top-of-file modification and a bottom-of-file
    // append land in two separate hunks (>2×context apart).
    write(dir, "a.txt", "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);

    // Two uncommitted changes: a modification near the top (absorbable) and a
    // pure addition at the very bottom (no line to blame → kept).
    write(
        dir,
        "a.txt",
        "l1\nL2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\n",
    );

    let repo = git2::Repository::open(dir).unwrap();
    let plan = plan_absorb(&repo, WINDOW).unwrap();

    let kept: Vec<_> = plan.kept().collect();
    assert_eq!(kept.len(), 1, "the appended line has no ancestor to absorb");
    assert_eq!(
        kept[0].disposition,
        HunkDisposition::Keep(KeepReason::PureAddition)
    );
    assert_eq!(plan.absorb_count(), 1, "the modified line is absorbable");

    execute_absorb(&repo, &plan).unwrap();

    // The appended "l11" line remains uncommitted; the modification is folded.
    assert!(read(dir, "a.txt").contains("l11"));
    assert!(
        status_porcelain(dir).contains("a.txt"),
        "kept hunk stays in tree"
    );
    assert!(show(dir, "HEAD", "a.txt").contains("L2"));
    assert!(!show(dir, "HEAD", "a.txt").contains("l11"));
}

#[test]
fn test_absorb_recorded_in_oplog() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir);
    write(dir, "a.txt", "alpha\nbeta\ngamma\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);
    write(dir, "a.txt", "alpha\nBETA\ngamma\n");

    // Isolate the oplog to this test's temp dir.
    let logdir = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    let backend = Backend::open(dir).unwrap();
    let plan = backend.plan_absorb(WINDOW).unwrap();
    let outcome = backend.execute_absorb(&plan).unwrap();
    assert_eq!(outcome.absorbed_hunks, 1);

    let tail = kagi_git::read_oplog_tail(10);
    std::env::remove_var("KAGI_LOG_DIR");
    let entry = tail
        .iter()
        .find(|e| e.op == "absorb")
        .expect("absorb entry in oplog");
    assert!(matches!(entry.outcome, kagi_git::OpOutcome::Success { .. }));
}

#[test]
fn test_absorb_preflight_head_moved() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_repo(dir);
    write(dir, "a.txt", "alpha\nbeta\ngamma\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);
    write(dir, "a.txt", "alpha\nBETA\ngamma\n");

    let repo = git2::Repository::open(dir).unwrap();
    let plan = plan_absorb(&repo, WINDOW).unwrap();

    // HEAD moves after planning.
    write(dir, "c.txt", "c\n");
    git(dir, &["add", "c.txt"]);
    git(dir, &["commit", "-qm", "add c.txt"]);

    assert!(
        preflight_absorb(&repo, &plan).is_err(),
        "stale plan must be refused"
    );
}

#[test]
fn test_absorb_protected_branch_blocked() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // Default protected branch: main.
    git(dir, &["init", "-b", "main", "-q"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    write(dir, "a.txt", "alpha\nbeta\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "add a.txt"]);
    write(dir, "a.txt", "alpha\nBETA\n");

    let repo = git2::Repository::open(dir).unwrap();
    let plan = plan_absorb(&repo, WINDOW).unwrap();
    assert!(plan.has_blockers());
    assert!(plan.blockers.iter().any(|b| matches!(
        b,
        kagi_domain::absorb::AbsorbBlocker::ProtectedBranch { .. }
    )));
    assert!(preflight_absorb(&repo, &plan).is_err());
}

// small helper kept below the tests that use it.
fn rev(dir: &Path, r: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", r])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
