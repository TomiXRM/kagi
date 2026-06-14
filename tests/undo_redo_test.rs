//! Integration tests for operation Undo / Redo — T-UNDOREDO-001 (ADR-0081).
//!
//! GitKraken-style undo/redo implemented as SAFE, reflog-backed branch-ref
//! moves through the plan → preflight → execute → verify pipeline. No commit is
//! ever destroyed; `git reset --hard`/clean/force are never used.
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `commit_undo_then_redo` | commit → undo (HEAD to parent) → redo (HEAD forward); commit stays in reflog |
//! | 2 | `merge_undo_then_redo` | merge → undo (HEAD to pre-merge) → redo; merge commit stays in reflog |
//! | 3 | `undo_preserves_working_tree_changes` | uncommitted edits survive an undo (soft/mixed, never --hard) |
//! | 4 | `plan_undo_stale_entry_is_blocked` | branch moved since the op → plan raises a blocker; execute refuses |
//! | 5 | `undo_redo_pipeline_via_domain_history` | drives the domain OperationHistory + Backend together |

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use kagi::git::{Backend, CommitId, HistoryEntry, OperationHistory, OperationKind};

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
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// True when `sha` is reachable from the branch reflog (i.e. NOT destroyed).
fn in_reflog(dir: &Path, sha: &str) -> bool {
    let out = Command::new("git")
        .args(["reflog", "--no-abbrev", "--format=%H"])
        .current_dir(dir)
        .output()
        .expect("reflog failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|l| l.trim() == sha)
}

/// `git cat-file -e <sha>` — true when the object exists in the ODB.
fn object_exists(dir: &Path, sha: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", sha])
        .current_dir(dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct LocalRepo {
    _tmp: TempDir,
    path: PathBuf,
}

fn setup_local() -> LocalRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "base.txt", "base content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    LocalRepo { _tmp: tmp, path }
}

fn entry(kind: OperationKind, branch: &str, before: &str, after: &str) -> HistoryEntry {
    HistoryEntry {
        kind,
        branch: branch.to_string(),
        before: CommitId(before.to_string()),
        after: CommitId(after.to_string()),
        summary: "test op".to_string(),
    }
}

// ────────────────────────────────────────────────────────────
// Test 1: commit → undo → redo
// ────────────────────────────────────────────────────────────

#[test]
fn commit_undo_then_redo() {
    let repo = setup_local();
    let dir = &repo.path;

    let before = head_sha(dir); // parent

    // Make a second commit (this is the operation we will undo).
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "add feature"]);
    let after = head_sha(dir);
    assert_ne!(before, after, "second commit must move HEAD");

    let e = entry(OperationKind::Commit, "main", &before, &after);
    let backend = Backend::open(dir).expect("open");

    // ── UNDO: HEAD moves back to the parent ──────────────────
    let plan = backend.plan_undo(&e).expect("plan_undo");
    assert!(
        plan.blockers.is_empty(),
        "undo blockers: {:?}",
        plan.blockers
    );
    backend.execute_undo(&e).expect("execute_undo");
    assert_eq!(
        head_sha(dir),
        before,
        "HEAD must be at the parent after undo"
    );

    // The undone commit is NOT destroyed — still in the ODB and the reflog.
    assert!(
        object_exists(dir, &after),
        "undone commit must remain in ODB"
    );
    assert!(
        in_reflog(dir, &after),
        "undone commit must remain in reflog"
    );

    // ── REDO: HEAD moves forward to the commit again ─────────
    let backend = Backend::open(dir).expect("reopen");
    let plan = backend.plan_redo(&e).expect("plan_redo");
    assert!(
        plan.blockers.is_empty(),
        "redo blockers: {:?}",
        plan.blockers
    );
    backend.execute_redo(&e).expect("execute_redo");
    assert_eq!(
        head_sha(dir),
        after,
        "HEAD must be back at the commit after redo"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: merge → undo → redo
// ────────────────────────────────────────────────────────────

#[test]
fn merge_undo_then_redo() {
    let repo = setup_local();
    let dir = &repo.path;

    // Create a feature branch with a commit.
    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_file(dir, "feature.txt", "feat\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "feature work"]);

    // Back on main, add a divergent commit so the merge is a real merge commit.
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main work\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "main work"]);

    let before = head_sha(dir); // pre-merge HEAD

    // Non-fast-forward merge → a merge commit.
    git(
        dir,
        &["merge", "--no-ff", "-q", "-m", "merge feature", "feature"],
    );
    let after = head_sha(dir);
    assert_ne!(before, after, "merge must move HEAD");
    // Sanity: the merge commit has two parents.
    let parents = rev_parse(dir, "HEAD^@");
    assert!(
        parents.lines().count() >= 2,
        "merge commit should have 2 parents, got: {:?}",
        parents
    );

    let e = entry(OperationKind::Merge, "main", &before, &after);

    // ── UNDO: HEAD back to the pre-merge commit ──────────────
    let backend = Backend::open(dir).expect("open");
    assert!(backend.plan_undo(&e).expect("plan").blockers.is_empty());
    backend.execute_undo(&e).expect("execute_undo");
    assert_eq!(head_sha(dir), before, "HEAD must be pre-merge after undo");

    // The merge commit must survive (not destroyed).
    assert!(
        object_exists(dir, &after),
        "merge commit must remain in ODB"
    );
    assert!(in_reflog(dir, &after), "merge commit must remain in reflog");

    // ── REDO: re-apply the merge ─────────────────────────────
    let backend = Backend::open(dir).expect("reopen");
    assert!(backend.plan_redo(&e).expect("plan").blockers.is_empty());
    backend.execute_redo(&e).expect("execute_redo");
    assert_eq!(
        head_sha(dir),
        after,
        "HEAD must be the merge commit after redo"
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: undo preserves uncommitted working-tree changes
// ────────────────────────────────────────────────────────────

#[test]
fn undo_preserves_working_tree_changes() {
    let repo = setup_local();
    let dir = &repo.path;

    let before = head_sha(dir);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "add feature"]);
    let after = head_sha(dir);

    // Unrelated uncommitted edit in the working tree.
    write_file(dir, "scratch.txt", "work in progress\n");

    let e = entry(OperationKind::Commit, "main", &before, &after);
    let backend = Backend::open(dir).expect("open");

    // Plan should WARN (not block) on the dirty tree.
    let plan = backend.plan_undo(&e).expect("plan_undo");
    assert!(plan.blockers.is_empty(), "dirty WT must not block undo");

    backend.execute_undo(&e).expect("execute_undo");
    assert_eq!(head_sha(dir), before);

    // The uncommitted file must survive verbatim — nothing hard-reset away.
    assert_eq!(
        read_file(dir, "scratch.txt"),
        "work in progress\n",
        "working-tree changes must be preserved by undo"
    );
}

// ────────────────────────────────────────────────────────────
// Test 4: stale entry is detected and refused
// ────────────────────────────────────────────────────────────

#[test]
fn plan_undo_stale_entry_is_blocked() {
    let repo = setup_local();
    let dir = &repo.path;

    let before = head_sha(dir);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "add feature"]);
    let after = head_sha(dir);

    // Move the branch on AFTER recording — the entry is now stale (branch tip
    // no longer equals `after`).
    write_file(dir, "more.txt", "more\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "more work"]);

    let e = entry(OperationKind::Commit, "main", &before, &after);
    let backend = Backend::open(dir).expect("open");

    let plan = backend.plan_undo(&e).expect("plan_undo");
    assert!(
        !plan.blockers.is_empty(),
        "stale entry (branch moved) must produce a blocker"
    );

    // Execute must also refuse rather than corrupt state.
    let err = backend.execute_undo(&e).expect_err("stale undo must error");
    let msg = format!("{}", err);
    assert!(
        msg.to_lowercase().contains("stale") || msg.to_lowercase().contains("expected"),
        "error should explain staleness, got: {}",
        msg
    );
    // HEAD untouched by the refused execute.
    assert_ne!(head_sha(dir), before);
}

// ────────────────────────────────────────────────────────────
// Test 5: domain history + backend pipeline together
// ────────────────────────────────────────────────────────────

#[test]
fn undo_redo_pipeline_via_domain_history() {
    let repo = setup_local();
    let dir = &repo.path;

    let before = head_sha(dir);
    write_file(dir, "f.txt", "x\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "commit one"]);
    let after = head_sha(dir);

    // Drive the in-session history exactly as the app does.
    let mut history = OperationHistory::new();
    history.record(entry(OperationKind::Commit, "main", &before, &after));
    assert!(history.can_undo());
    assert!(!history.can_redo());

    // Undo: peek, run the backend move, then advance the cursor.
    let e = history.peek_undo().cloned().expect("peek_undo");
    Backend::open(dir)
        .unwrap()
        .execute_undo(&e)
        .expect("execute_undo");
    history.undo();
    assert_eq!(head_sha(dir), before);
    assert!(!history.can_undo());
    assert!(history.can_redo());

    // Redo.
    let e = history.peek_redo().cloned().expect("peek_redo");
    Backend::open(dir)
        .unwrap()
        .execute_redo(&e)
        .expect("execute_redo");
    history.redo();
    assert_eq!(head_sha(dir), after);
    assert!(history.can_undo());
    assert!(!history.can_redo());
}
