//! Integration tests for oplog (T017).
//!
//! All tests must set KAGI_LOG_DIR to a tempdir.  Writing to $HOME/.kagi is
//! **explicitly forbidden** in tests.
//!
//! KAGI_LOG_DIR is a process-global env var shared by all test threads.
//! We serialize all tests in this file with ENV_LOCK to prevent races.

use std::path::PathBuf;
use std::sync::Mutex;
use kagi::git::oplog::{OpLogEntry, OpOutcome, append_oplog};
use kagi::git::ops::StateSummary;

/// Serialize all env-var-using tests to prevent KAGI_LOG_DIR races.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make_summary(head: &str, dirty: &str) -> StateSummary {
    StateSummary { head: head.to_string(), dirty: dirty.to_string() }
}

fn make_entry(op: &str, ts: i64, outcome: OpOutcome) -> OpLogEntry {
    OpLogEntry {
        timestamp: ts,
        op: op.to_string(),
        repo: "/test/repo".to_string(),
        before: make_summary("branch: main", "clean"),
        outcome,
    }
}

/// Helper: create a tempdir.  The returned TempDir must be kept alive.
fn with_tempdir() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_str().unwrap().to_string();
    (dir, path)
}

// ── Test 1: 5 entries written → 5 lines ──────────────────────

#[test]
fn five_ops_produce_five_jsonl_lines() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (dir, log_dir) = with_tempdir();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let ops = [
        ("checkout",      OpOutcome::Success { after: make_summary("branch: feature", "clean") }),
        ("create-branch", OpOutcome::Success { after: make_summary("branch: main", "clean") }),
        ("stash-push",    OpOutcome::Success { after: make_summary("branch: main", "clean") }),
        ("stash-apply",   OpOutcome::Success { after: make_summary("branch: main", "2 modified") }),
        ("cherry-pick",   OpOutcome::Success { after: make_summary("branch: main (+1 commit)", "clean") }),
    ];

    let mut last_path: Option<PathBuf> = None;
    for (i, (op, outcome)) in ops.into_iter().enumerate() {
        let entry = make_entry(op, (i as i64) + 1, outcome);
        let p = append_oplog(&entry).expect("write");
        last_path = Some(p);
    }

    let path = last_path.unwrap();
    let content = std::fs::read_to_string(&path).expect("read");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 5, "expected 5 JSON lines, got {}: {:?}", lines.len(), lines);

    // Each line should be parseable as a JSON object (starts with '{').
    for (i, line) in lines.iter().enumerate() {
        assert!(line.starts_with('{'), "line {} is not a JSON object: {}", i, line);
        assert!(line.ends_with('}'), "line {} does not end with '}}': {}", i, line);
    }

    // Verify op names are present.
    assert!(lines[0].contains("checkout"),      "line 0 op mismatch");
    assert!(lines[1].contains("create-branch"), "line 1 op mismatch");
    assert!(lines[2].contains("stash-push"),    "line 2 op mismatch");
    assert!(lines[3].contains("stash-apply"),   "line 3 op mismatch");
    assert!(lines[4].contains("cherry-pick"),   "line 4 op mismatch");

    // Restore env.
    match prev { Some(v) => std::env::set_var("KAGI_LOG_DIR", v), None => std::env::remove_var("KAGI_LOG_DIR") }
    drop(dir);
}

// ── Test 2: Refused entry is recorded ────────────────────────

#[test]
fn refused_entry_recorded() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (dir, log_dir) = with_tempdir();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = make_entry(
        "checkout",
        42,
        OpOutcome::Refused {
            blockers: vec![
                "Working tree has 1 modified".to_string(),
                "Branch 'x' does not exist".to_string(),
            ],
        },
    );

    let path = append_oplog(&entry).expect("write");
    let content = std::fs::read_to_string(&path).expect("read");
    let line = content.lines().next().expect("at least one line");

    assert!(line.contains("\"kind\":\"Refused\""), "kind missing");
    assert!(line.contains("Working tree has 1 modified"), "blocker 1 missing");
    assert!(line.contains("Branch"), "blocker 2 missing");
    assert!(line.contains("\"timestamp\":42"), "timestamp missing");

    match prev { Some(v) => std::env::set_var("KAGI_LOG_DIR", v), None => std::env::remove_var("KAGI_LOG_DIR") }
    drop(dir);
}

// ── Test 3: Failed entry is recorded ─────────────────────────

#[test]
fn failed_entry_recorded() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (dir, log_dir) = with_tempdir();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = make_entry(
        "stash-push",
        99,
        OpOutcome::Failed { error: "stash push failed: nothing to stash".to_string() },
    );

    let path = append_oplog(&entry).expect("write");
    let content = std::fs::read_to_string(&path).expect("read");
    let line = content.lines().next().expect("at least one line");

    assert!(line.contains("\"kind\":\"Failed\""), "kind missing");
    assert!(line.contains("stash push failed"), "error text missing");

    match prev { Some(v) => std::env::set_var("KAGI_LOG_DIR", v), None => std::env::remove_var("KAGI_LOG_DIR") }
    drop(dir);
}

// ── Test 4: Special chars in fields are escaped ───────────────

#[test]
fn special_chars_escaped_in_output() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (dir, log_dir) = with_tempdir();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = OpLogEntry {
        timestamp: 0,
        op: "checkout".to_string(),
        repo: "/path/with \"quotes\"".to_string(),
        before: StateSummary {
            head: "branch: feat/with\\backslash".to_string(),
            dirty: "1 modified\nfile".to_string(),
        },
        outcome: OpOutcome::Success {
            after: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
        },
    };

    let path = append_oplog(&entry).expect("write");
    let line = std::fs::read_to_string(&path).expect("read");
    let line = line.trim_end();

    // Double quotes in repo path must be escaped.
    assert!(line.contains("\\\"quotes\\\""), "quote escaping failed");
    // Backslash in head must be escaped.
    assert!(line.contains("\\\\backslash"), "backslash escaping failed");
    // Newline in dirty string must be escaped.
    assert!(line.contains("\\n"), "newline escaping failed");
    // The line itself must not contain a literal newline (it's a JSONL record).
    assert!(!line.contains('\n'), "literal newline in jsonl line");

    match prev { Some(v) => std::env::set_var("KAGI_LOG_DIR", v), None => std::env::remove_var("KAGI_LOG_DIR") }
    drop(dir);
}

// ── Test 5: No write when KAGI_LOG_DIR not set and HOME unavailable ──
// (We test the KAGI_LOG_DIR path explicitly; we don't touch $HOME)

#[test]
fn explicit_kagi_log_dir_overrides_home() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (dir, log_dir) = with_tempdir();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", &log_dir);

    let entry = make_entry("cherry-pick", 7, OpOutcome::Success {
        after: make_summary("branch: main (+1 commit)", "clean"),
    });
    let path = append_oplog(&entry).expect("write");

    // Path must be inside the tempdir, not anywhere in $HOME.
    let expected_prefix = PathBuf::from(&log_dir);
    assert!(
        path.starts_with(&expected_prefix),
        "path {:?} does not start with tempdir {:?}",
        path, expected_prefix
    );

    match prev { Some(v) => std::env::set_var("KAGI_LOG_DIR", v), None => std::env::remove_var("KAGI_LOG_DIR") }
    drop(dir);
}
