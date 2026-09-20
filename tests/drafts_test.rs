//! Integration tests for commit-message draft autosave (T-COMMIT-007 / ADR-0042).
//!
//! Every test sets KAGI_LOG_DIR to a tempdir so drafts land in isolated storage
//! and never touch `$HOME/.kagi`. KAGI_LOG_DIR is a process-global env var, so
//! all tests in this file are serialised with ENV_LOCK (same pattern as the
//! oplog integration tests).

use std::path::Path;
use std::sync::Mutex;

use kagi_git::drafts::{
    clear_draft, clear_issue_draft_if_version, flush_issue_drafts, issue_draft_version, load_draft,
    load_issue_draft, queue_issue_draft, save_draft,
};

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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        assert!(load_draft(Path::new("/tmp/kagi-it/empty"), "main").is_none());
    });
}

#[test]
fn clear_with_no_existing_file_is_ok() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        // Clearing a branch that never had a draft must succeed silently.
        clear_draft(Path::new("/tmp/kagi-it/repo"), "main").expect("no-op clear ok");
    });
}

#[test]
fn issue_draft_round_trip_and_pending_latest_value() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/issue");
        let title = "日本語 \"タイトル\"";
        let body = "本文\n\n```rust\nlet path = \"C:\\\\tmp\";\n```\n🙂";
        queue_issue_draft(repo, None, "old", "old body");
        queue_issue_draft(repo, None, title, body);
        assert!(!log_dir.join("drafts").exists(), "queue must not write");
        assert_eq!(
            load_issue_draft(repo, None),
            Some((title.to_owned(), body.to_owned()))
        );
        flush_issue_drafts().expect("flush latest");
        assert_eq!(
            load_issue_draft(repo, None),
            Some((title.to_owned(), body.to_owned()))
        );
        let stored = load_draft(repo, ":issue:new").expect("common draft record");
        assert_eq!(stored.mode, "issue-composer");
        assert_eq!(
            serde_json::from_str::<(String, String)>(&stored.message).expect("tuple payload"),
            (title.to_owned(), body.to_owned())
        );
    });
}

#[test]
fn issue_drafts_isolate_repo_number_and_commit_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let a = Path::new("/tmp/kagi-it/issue-a");
        let b = Path::new("/tmp/kagi-it/issue-b");
        save_draft(a, "issue/new", "commit", "plain").expect("commit draft");
        queue_issue_draft(a, None, "new", "new body");
        queue_issue_draft(a, Some(7), "", "reply seven");
        queue_issue_draft(a, Some(8), "", "reply eight");
        queue_issue_draft(b, Some(7), "", "other repo reply");
        flush_issue_drafts().expect("flush all");
        assert_eq!(
            load_issue_draft(a, None),
            Some(("new".into(), "new body".into()))
        );
        assert_eq!(
            load_issue_draft(a, Some(7)),
            Some(("".into(), "reply seven".into()))
        );
        assert_eq!(
            load_issue_draft(a, Some(8)),
            Some(("".into(), "reply eight".into()))
        );
        assert_eq!(
            load_issue_draft(b, Some(7)),
            Some(("".into(), "other repo reply".into()))
        );
        assert_eq!(
            load_draft(a, "issue/new").expect("commit retained").message,
            "commit"
        );
        assert!(load_issue_draft(b, None).is_none());
    });
}

#[test]
fn issue_clear_replaces_pending_body_and_delayed_flush_cannot_restore_it() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-clear");
        queue_issue_draft(repo, None, "title", "saved body");
        flush_issue_drafts().expect("initial save");
        queue_issue_draft(repo, None, "title", "unsaved later body");
        queue_issue_draft(repo, None, "", "");
        assert!(
            load_issue_draft(repo, None).is_none(),
            "pending clear hides file"
        );
        flush_issue_drafts().expect("clear");
        // A timer queued before Create succeeded must flush current memory,
        // not carry a captured copy of the submitted body to the filesystem.
        std::thread::spawn(flush_issue_drafts)
            .join()
            .expect("late timer thread")
            .expect("late flush");
        assert!(load_issue_draft(repo, None).is_none());
        assert!(load_draft(repo, ":issue:new").is_none());
    });
}

#[test]
fn issue_queue_keeps_its_original_storage_when_environment_changes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|original| {
        let second = tempfile::tempdir().expect("second storage");
        let repo = Path::new("/tmp/kagi-it/same-repo");
        queue_issue_draft(repo, None, "original", "original body");
        std::env::set_var("KAGI_LOG_DIR", second.path());
        assert!(load_issue_draft(repo, None).is_none());
        queue_issue_draft(repo, None, "second", "second body");
        flush_issue_drafts().expect("flush both fixed destinations");
        assert_eq!(
            load_issue_draft(repo, None),
            Some(("second".into(), "second body".into()))
        );
        std::env::set_var("KAGI_LOG_DIR", original);
        assert_eq!(
            load_issue_draft(repo, None),
            Some(("original".into(), "original body".into()))
        );
    });
}

#[test]
fn failed_issue_flush_preserves_pending_value_for_retry() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/issue-retry");
        let drafts = log_dir.join("drafts");
        std::fs::write(&drafts, "not a directory").expect("block directory creation");
        queue_issue_draft(repo, None, "title", "retry body");
        assert!(flush_issue_drafts().is_err());
        assert_eq!(
            load_issue_draft(repo, None),
            Some(("title".into(), "retry body".into()))
        );
        std::fs::remove_file(&drafts).expect("remove blocker file");
        flush_issue_drafts().expect("retry retained value");
        assert_eq!(
            load_issue_draft(repo, None),
            Some(("title".into(), "retry body".into()))
        );
        assert!(load_draft(repo, ":issue:new").is_some());
    });
}

#[test]
fn issue_load_rejects_wrong_mode_and_malformed_payload() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-corrupt");
        save_draft(repo, ":issue:new", "[\"t\",\"b\"]", "plain").expect("wrong mode");
        assert!(load_issue_draft(repo, None).is_none());
        save_draft(repo, ":issue:new", "not a tuple", "issue-composer").expect("bad payload");
        assert!(load_issue_draft(repo, None).is_none());
    });
}

#[test]
fn posted_issue_draft_version_is_consumed_once_and_survives_flush() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/issue-token");
        let initial = issue_draft_version(repo, None);
        assert_ne!(initial, 0);
        assert_eq!(initial, issue_draft_version(repo, None));
        assert!(!log_dir.join("drafts").exists());
        let posted = queue_issue_draft(repo, None, "title", "submitted body");
        assert!(posted > initial);
        flush_issue_drafts().expect("save posted body");
        assert_eq!(posted, issue_draft_version(repo, None));
        assert!(clear_issue_draft_if_version(repo, None, posted));
        assert!(issue_draft_version(repo, None) > posted);
        assert!(!clear_issue_draft_if_version(repo, None, posted));
        assert!(load_issue_draft(repo, None).is_none());
        flush_issue_drafts().expect("persist clear");
        std::thread::spawn(flush_issue_drafts)
            .join()
            .expect("late autosave thread")
            .expect("late autosave flush");
        assert!(load_draft(repo, ":issue:new").is_none());
    });
}

#[test]
fn reopened_editor_new_version_protects_draft_from_old_post_completion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-reopened");
        let posted = queue_issue_draft(repo, Some(7), "", "submitted reply");
        flush_issue_drafts().expect("save before close");
        // Closing a tab does not erase the storage token. A reopened editor
        // reads it, then input obtains a fresh token even for identical text.
        assert_eq!(issue_draft_version(repo, Some(7)), posted);
        let edited = queue_issue_draft(repo, Some(7), "", "submitted reply");
        assert!(edited > posted);
        assert!(!clear_issue_draft_if_version(repo, Some(7), posted));
        assert_eq!(issue_draft_version(repo, Some(7)), edited);
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some(("".into(), "submitted reply".into()))
        );
        flush_issue_drafts().expect("persist reopened draft");
        assert!(!clear_issue_draft_if_version(repo, Some(7), posted));
        assert!(load_issue_draft(repo, Some(7)).is_some());
    });
}

#[test]
fn issue_version_clear_targets_original_storage_and_checks_issue_identity() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|original| {
        let repo = Path::new("/tmp/kagi-it/issue-storage-token");
        let posted = queue_issue_draft(repo, Some(7), "", "original reply");
        flush_issue_drafts().expect("save original");
        let second = tempfile::tempdir().expect("second storage");
        std::env::set_var("KAGI_LOG_DIR", second.path());
        let other = queue_issue_draft(repo, Some(7), "", "other storage reply");
        assert!(!clear_issue_draft_if_version(repo, Some(8), posted));
        assert!(!clear_issue_draft_if_version(
            Path::new("/different"),
            Some(7),
            posted
        ));
        assert!(clear_issue_draft_if_version(repo, Some(7), posted));
        assert_eq!(issue_draft_version(repo, Some(7)), other);
        flush_issue_drafts().expect("flush clear to original destination");
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some(("".into(), "other storage reply".into()))
        );
        std::env::set_var("KAGI_LOG_DIR", original);
        assert!(load_issue_draft(repo, Some(7)).is_none());
    });
}

#[path = "support/isolated.rs"]
mod test_support;
