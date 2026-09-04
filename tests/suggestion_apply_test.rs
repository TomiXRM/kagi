//! Integration tests for the PR review "suggested change" local apply
//! (#351, ADR-0172).
//!
//! Verifies the full plan → confirm → preflight → execute → verify → oplog
//! path through `Backend::run`:
//! - applying a suggestion replaces EXACTLY the anchored range in the
//!   working-tree file (nothing staged / committed);
//! - a stale range (the working-tree content at the range changed AFTER the
//!   plan was built) makes execute REFUSE — never edit the wrong lines;
//! - a successful apply is recorded in the oplog as `op="apply-suggestion"`.
//!
//! All writes are confined to `TempDir` repositories. `KAGI_LOG_DIR` is
//! process-global, so oplog tests serialize on `ENV_LOCK`.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use tempfile::TempDir;

use kagi_git::oplog::{read_oplog_tail, OpOutcome};
use kagi_git::{Backend, Operation, Suggestion};

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

/// Repo with `src/lib.rs` (3 lines) committed on `main`, HEAD attached, clean.
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "one\ntwo\nthree\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
}

fn suggestion(start: u32, end: u32, replacement: &str) -> Suggestion {
    Suggestion {
        path: "src/lib.rs".into(),
        start_line: start,
        end_line: end,
        replacement: replacement.into(),
    }
}

// ── applying replaces exactly the anchored range ─────────────

#[test]
fn apply_replaces_exactly_the_anchored_range() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    let mut backend = Backend::open(repo.path()).expect("open");

    // Suggest replacing line 2 ("two") with "TWO".
    let s = suggestion(2, 2, "TWO");
    let expected = backend.capture_suggestion_context(&s).expect("capture");
    assert_eq!(expected, vec!["two".to_string()]);

    let op = Operation::ApplySuggestion {
        suggestion: s,
        expected_original: expected,
    };
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty(), "fresh suggestion has no blockers");
    backend.run(&op, &plan).expect("run");

    // Only line 2 changed; lines 1 and 3 untouched; trailing newline kept.
    let after = std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap();
    assert_eq!(after, "one\nTWO\nthree\n");

    // Working tree only — nothing was staged.
    let staged = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(repo.path())
        .env("HOME", repo.path())
        .output()
        .unwrap();
    assert!(
        staged.stdout.is_empty(),
        "apply must not stage anything (working tree only)"
    );

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── stale range → execute REFUSES (TOCTOU guard) ─────────────

#[test]
fn stale_range_after_plan_makes_execute_refuse() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    let mut backend = Backend::open(repo.path()).expect("open");

    let s = suggestion(2, 2, "TWO");
    let expected = backend.capture_suggestion_context(&s).expect("capture");
    let op = Operation::ApplySuggestion {
        suggestion: s,
        expected_original: expected,
    };
    let plan = backend.plan(&op).expect("plan");
    assert!(plan.blockers.is_empty());

    // TOCTOU: the anchored line changes AFTER the plan was built and confirmed.
    std::fs::write(repo.path().join("src/lib.rs"), "one\nEDITED\nthree\n").unwrap();

    let err = backend
        .run(&op, &plan)
        .expect_err("stale range must refuse");
    assert!(
        err.to_string().contains("stale range"),
        "refusal names the stale range, got: {err}"
    );

    // MUTATION GUARD: the file the user edited is left untouched by the refusal.
    let after = std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap();
    assert_eq!(after, "one\nEDITED\nthree\n", "refusal must not write");

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── a successful apply is recorded in the oplog ──────────────

#[test]
fn apply_is_recorded_in_oplog() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    build_repo(repo.path());
    let mut backend = Backend::open(repo.path()).expect("open");

    let s = suggestion(1, 3, "single\n");
    let expected = backend.capture_suggestion_context(&s).expect("capture");
    let op = Operation::ApplySuggestion {
        suggestion: s,
        expected_original: expected,
    };
    let plan = backend.plan(&op).expect("plan");
    backend.run(&op, &plan).expect("run");

    // Multi-line range 1..=3 collapsed to one line.
    let after = std::fs::read_to_string(repo.path().join("src/lib.rs")).unwrap();
    assert_eq!(after, "single\n");

    let tail = read_oplog_tail(10);
    let e = tail
        .iter()
        .find(|e| e.op == "apply-suggestion")
        .expect("apply-suggestion recorded in oplog");
    assert!(
        matches!(e.outcome, OpOutcome::Success { .. }),
        "successful apply logs Success"
    );

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}
