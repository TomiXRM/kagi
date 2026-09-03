//! ADR-0149 / #329 / #333 acceptance tests.
//!
//! These pin the two coupled changes:
//!   * `Backend::run` is now the sole oplog writer for the enforced pipeline —
//!     calling it directly (NO UI) produces exactly one oplog entry per op.
//!   * `OpLogEntry` carries `id` / `parent` / `actor` / `worktree`, with a
//!     monotonic sequence and back-compat for old lines.
//!
//! The oplog is a single JSONL file at `$KAGI_LOG_DIR/operations.jsonl`; every
//! test points `KAGI_LOG_DIR` at its own tempdir and serializes on `ENV_LOCK`
//! (the var is process-global).

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use tempfile::TempDir;

use kagi_git::oplog::{append_oplog, read_oplog_tail, Actor, OpLogEntry, OpOutcome};
use kagi_git::ops::StateSummary;
use kagi_git::{
    oplog_outcome_from, Backend, CommitId, DiscardOutcome, Operation, OperationOutcome,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

/// Repo with one commit on `main`, HEAD attached, clean.
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
}

fn head_commit_id(backend: &Backend) -> CommitId {
    backend.head_commit_id().expect("head commit id")
}

/// Run a `create-branch` op through `Backend::run` and return the resulting
/// oplog tail (newest first).
fn run_create_branch(repo: &Path, name: &str) {
    let mut backend = Backend::open(repo).expect("open");
    let at = head_commit_id(&backend);
    let op = Operation::CreateBranch {
        name: name.to_string(),
        at,
    };
    let plan = backend.plan(&op).expect("plan");
    backend.run(&op, &plan).expect("run");
}

// ── #329: run records without any UI ─────────────────────────

#[test]
fn run_records_oplog_without_ui() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    // No KagiApp / record_op anywhere — just the backend.
    run_create_branch(repo.path(), "feature-x");

    let tail = read_oplog_tail(10);
    // MUTATION GUARD: if the run-side write is removed, this is 0 and fails.
    assert_eq!(tail.len(), 1, "run must write exactly one entry");
    let e = &tail[0];
    assert_eq!(e.op, "create-branch");
    assert_eq!(e.actor, Actor::Human, "GUI/default actor is Human");
    assert!(matches!(e.outcome, OpOutcome::Success { .. }));
    assert!(e.worktree.is_some(), "worktree recorded");

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #329: actor threads into run ─────────────────────────────

#[test]
fn run_records_actor_set_on_backend() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    let mut backend = Backend::open(repo.path()).expect("open");
    backend.set_actor(Actor::Mcp);
    let at = head_commit_id(&backend);
    let op = Operation::CreateBranch {
        name: "agent-branch".to_string(),
        at,
    };
    let plan = backend.plan(&op).expect("plan");
    backend.run(&op, &plan).expect("run");

    let tail = read_oplog_tail(1);
    assert_eq!(tail[0].actor, Actor::Mcp, "actor threaded through run");

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #333: id monotonic + parent chains across ≥3 entries ─────

#[test]
fn ids_are_monotonic_and_parent_chains() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    run_create_branch(repo.path(), "b1");
    run_create_branch(repo.path(), "b2");
    run_create_branch(repo.path(), "b3");

    let tail = read_oplog_tail(10); // newest first
    assert_eq!(tail.len(), 3);
    // Oldest → newest.
    let oldest = &tail[2];
    let mid = &tail[1];
    let newest = &tail[0];
    assert!(
        oldest.id < mid.id && mid.id < newest.id,
        "ids must be strictly increasing: {} {} {}",
        oldest.id,
        mid.id,
        newest.id
    );
    assert_eq!(oldest.parent, None, "first entry has no parent");
    assert_eq!(mid.parent, Some(oldest.id), "parent chains");
    assert_eq!(newest.parent, Some(mid.id), "parent chains");

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #333: old-format line still parses (golden, back-compat) ─

#[test]
fn old_and_new_format_lines_both_parse() {
    let _guard = ENV_LOCK.lock().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    // One PRE-ADR-0149 line (no id/parent/actor/worktree) followed by one NEW
    // line that carries all four fields, in the same file.
    let old_line = "{\"timestamp\":1751234567,\"op\":\"checkout\",\"repo\":\"/tmp/r\",\"before\":{\"head\":\"branch: main\",\"dirty\":\"clean\"},\"outcome\":{\"kind\":\"Success\",\"after\":{\"head\":\"branch: dev\",\"dirty\":\"clean\"}}}";
    let new_line = "{\"id\":1,\"parent\":0,\"timestamp\":1751234568,\"op\":\"create-branch\",\"repo\":\"/tmp/r\",\"actor\":\"mcp\",\"worktree\":\"/tmp/r\",\"before\":{\"head\":\"branch: dev\",\"dirty\":\"clean\"},\"outcome\":{\"kind\":\"Success\",\"after\":{\"head\":\"branch: dev\",\"dirty\":\"clean\"}}}";
    let path = logdir.path().join("operations.jsonl");
    std::fs::write(&path, format!("{old_line}\n{new_line}\n")).unwrap();

    let tail = read_oplog_tail(10); // newest first
    assert_eq!(tail.len(), 2, "both lines parse");

    let newest = &tail[0];
    let oldest = &tail[1];

    // Old line: documented defaults — id from 0-based index, actor Human,
    // worktree None, parent None (it is first).
    // MUTATION GUARD: breaking the back-compat fallback breaks these.
    assert_eq!(oldest.op, "checkout");
    assert_eq!(oldest.id, 0, "old line id reconstructed from index");
    assert_eq!(oldest.parent, None);
    assert_eq!(oldest.actor, Actor::Human);
    assert_eq!(oldest.worktree, None);

    // New line: explicit fields round-trip.
    assert_eq!(newest.op, "create-branch");
    assert_eq!(newest.id, 1);
    assert_eq!(newest.parent, Some(0));
    assert_eq!(newest.actor, Actor::Mcp);
    assert_eq!(newest.worktree.as_deref(), Some("/tmp/r"));

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #333: new-schema fields round-trip through append + read ──

#[test]
fn new_fields_round_trip() {
    let _guard = ENV_LOCK.lock().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    let entry = OpLogEntry::new(
        "cherry-pick",
        "/tmp/repo",
        StateSummary {
            head: "branch: main".into(),
            dirty: "clean".into(),
        },
        OpOutcome::Success {
            after: StateSummary {
                head: "branch: main".into(),
                dirty: "clean".into(),
            },
        },
    )
    .with_actor(Actor::Cli)
    .with_worktree(Some("/tmp/repo".into()));
    append_oplog(&entry).expect("append");

    let tail = read_oplog_tail(1);
    assert_eq!(tail[0].actor, Actor::Cli);
    assert_eq!(tail[0].worktree.as_deref(), Some("/tmp/repo"));
    assert_eq!(tail[0].id, 0, "first append gets id 0");
    assert_eq!(tail[0].parent, None);

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── #329/#281: a partial discard maps to OpOutcome::Partial ──

#[test]
fn partial_discard_maps_to_partial_outcome() {
    let predicted = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let partial = DiscardOutcome {
        backups: Vec::new(),
        unverified: vec!["a.txt".into()],
        error: Some("write failed".into()),
    };
    let result: Result<OperationOutcome, kagi_git::GitError> =
        Ok(OperationOutcome::Discard(partial));
    // MUTATION GUARD: dropping the is_partial() branch makes this Success.
    match oplog_outcome_from(&result, &predicted) {
        OpOutcome::Partial { error, .. } => assert_eq!(error, "write failed"),
        other => panic!("expected Partial, got {other:?}"),
    }

    // A complete discard is Success.
    let complete: Result<OperationOutcome, kagi_git::GitError> = Ok(OperationOutcome::Discard(
        DiscardOutcome::complete(Vec::new()),
    ));
    assert!(matches!(
        oplog_outcome_from(&complete, &predicted),
        OpOutcome::Success { .. }
    ));
}
