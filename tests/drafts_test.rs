//! Integration tests for commit-message draft autosave (T-COMMIT-007 / ADR-0042).
//!
//! Every test sets KAGI_LOG_DIR to a tempdir so drafts land in isolated storage
//! and never touch `$HOME/.kagi`. KAGI_LOG_DIR is a process-global env var, so
//! all tests in this file are serialised with ENV_LOCK (same pattern as the
//! oplog integration tests).

use std::path::Path;
use std::sync::Mutex;

use kagi::git::drafts::{clear_draft, load_draft, save_draft};

/// Serialize all env-var-using tests to prevent KAGI_LOG_DIR races.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` with KAGI_LOG_DIR pointing at a fresh tempdir, restoring the previous
/// value afterwards. Serialised against other env-mutating tests.
fn with_log_dir<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", dir.path());
    let result = f(dir.path());
    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
    result
}

#[test]
fn round_trip_preserves_message_mode_and_branch() {
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/repo");
        save_draft(repo, "feature/x", "subject\n\nbody line", "template").expect("save");

        let d = load_draft(repo, "feature/x").expect("draft present");
        assert_eq!(d.repo, "/tmp/kagi-it/repo");
        assert_eq!(d.branch, "feature/x");
        assert_eq!(d.message, "subject\n\nbody line");
        assert_eq!(d.mode, "template");
        assert!(d.updated > 0, "updated timestamp should be set");
    });
}

#[test]
fn drafts_are_isolated_by_branch_and_repo() {
    with_log_dir(|_| {
        let repo_a = Path::new("/tmp/kagi-it/a");
        let repo_b = Path::new("/tmp/kagi-it/b");

        save_draft(repo_a, "main", "a-main", "plain").expect("a/main");
        save_draft(repo_a, "topic", "a-topic", "plain").expect("a/topic");
        save_draft(repo_b, "main", "b-main", "plain").expect("b/main");

        assert_eq!(load_draft(repo_a, "main").unwrap().message, "a-main");
        assert_eq!(load_draft(repo_a, "topic").unwrap().message, "a-topic");
        assert_eq!(load_draft(repo_b, "main").unwrap().message, "b-main");
    });
}

#[test]
fn clear_deletes_the_branch_draft() {
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/repo");
        save_draft(repo, "main", "draft body", "plain").expect("save");
        assert!(load_draft(repo, "main").is_some());

        clear_draft(repo, "main").expect("clear");
        assert!(load_draft(repo, "main").is_none());
    });
}

#[test]
fn saving_blank_message_clears_existing_draft() {
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/repo");
        save_draft(repo, "main", "non-empty", "plain").expect("save");
        assert!(load_draft(repo, "main").is_some());

        save_draft(repo, "main", "  \t\n ", "plain").expect("save blank");
        assert!(
            load_draft(repo, "main").is_none(),
            "blank save should delete"
        );
    });
}

#[test]
fn corrupt_draft_file_loads_as_none() {
    with_log_dir(|log_dir| {
        // Save a valid draft first to materialise the filename, then corrupt it.
        let repo = Path::new("/tmp/kagi-it/repo");
        save_draft(repo, "main", "valid", "plain").expect("save");

        // The single file in <log_dir>/drafts/ is our draft; overwrite it.
        let drafts = log_dir.join("drafts");
        let file = std::fs::read_dir(&drafts)
            .expect("read drafts dir")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .expect("a draft file exists");
        std::fs::write(&file, "}{ broken json \x00 not parseable").expect("corrupt");

        assert!(
            load_draft(repo, "main").is_none(),
            "corrupt draft must be ignored, not crash"
        );
    });
}

#[test]
fn load_without_any_draft_returns_none() {
    with_log_dir(|_| {
        assert!(load_draft(Path::new("/tmp/kagi-it/empty"), "main").is_none());
    });
}

#[test]
fn clear_with_no_existing_file_is_ok() {
    with_log_dir(|_| {
        // Clearing a branch that never had a draft must succeed silently.
        clear_draft(Path::new("/tmp/kagi-it/repo"), "main").expect("no-op clear ok");
    });
}
