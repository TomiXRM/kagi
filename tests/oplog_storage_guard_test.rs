//! The oplog must never fall back to a developer's HOME during test execution.

use kagi_git::oplog::{append_oplog, Actor, OpLogEntry, OpOutcome};
use kagi_git::ops::StateSummary;

#[path = "support/isolated.rs"]
mod test_support;

fn entry(op: &str) -> OpLogEntry {
    OpLogEntry {
        id: 0,
        parent: None,
        backup_refs: Vec::new(),
        actor: Actor::Human,
        worktree: None,
        timestamp: 1,
        op: op.to_string(),
        repo: "/test/repo".to_string(),
        before: StateSummary {
            head: "branch: main".to_string(),
            dirty: "clean".to_string(),
        },
        outcome: OpOutcome::Success {
            after: StateSummary {
                head: "branch: main".to_string(),
                dirty: "clean".to_string(),
            },
        },
    }
}

#[test]
fn test_runtime_requires_log_dir_and_honors_explicit_dir() {
    if !test_support::run_isolated() {
        return;
    }

    let home = tempfile::tempdir().expect("fake home");
    let logs = tempfile::tempdir().expect("explicit logs");
    let previous_home = std::env::var_os("HOME");
    let previous_log = std::env::var_os("KAGI_LOG_DIR");
    std::env::set_var("HOME", home.path());
    std::env::remove_var("KAGI_LOG_DIR");

    let err = append_oplog(&entry("test-runtime-no-log-dir"))
        .expect_err("test runtime must reject HOME fallback");
    assert!(
        matches!(err, kagi_git::GitError::Other(message) if message == "tests must set KAGI_LOG_DIR")
    );
    assert!(
        !home.path().join(".kagi/operations.jsonl").exists(),
        "the rejected append must not write under HOME"
    );

    std::env::set_var("KAGI_LOG_DIR", logs.path());
    let path = append_oplog(&entry("test-runtime-explicit-log-dir"))
        .expect("explicit KAGI_LOG_DIR must permit append");
    assert_eq!(path, logs.path().join("operations.jsonl"));

    match previous_log {
        Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
    match previous_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
}
