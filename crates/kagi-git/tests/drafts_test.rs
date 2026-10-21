//! Integration tests for commit-message draft autosave (T-COMMIT-007 / ADR-0042).
//!
//! Every test sets KAGI_LOG_DIR to a tempdir so drafts land in isolated storage
//! and never touch `$HOME/.kagi`. KAGI_LOG_DIR is a process-global env var, so
//! all tests in this file are serialised with ENV_LOCK (same pattern as the
//! oplog integration tests).

use std::path::Path;
use std::sync::Mutex;

use kagi_domain::github::IssueCreateFields;
use kagi_git::drafts::{
    clear_draft, clear_issue_draft_if_version, flush_issue_draft_if_version, flush_issue_drafts,
    issue_draft_version as addressed_version, load_draft, load_issue_draft as load_addressed,
    queue_issue_draft as queue_addressed, save_draft, IssueDraftLoad, IssueDraftRecord,
    PrCommentDraftSession,
};

/// The repository the drafts below are written to, and where a New Issue
/// draft for it is stored.
const BASE: &str = "github.com/acme/widgets";
const NEW: &str = ":issue:github.com/acme/widgets:new";
const REPLY_7: &str = ":issue:github.com/acme/widgets:7";

fn queue_issue_draft(
    repo: &Path,
    number: Option<u64>,
    title: &str,
    body: &str,
    fields: &IssueCreateFields,
) -> u64 {
    queue_addressed(repo, BASE, number, title, body, fields)
}

fn issue_draft_version(repo: &Path, number: Option<u64>) -> u64 {
    addressed_version(repo, BASE, number)
}

fn load_issue_record(repo: &Path, number: Option<u64>) -> Option<IssueDraftRecord> {
    load_addressed(repo, BASE, number).record
}

/// The loaded draft's text; tests that pick labels or assignees read the
/// whole record.
fn load_issue_draft(repo: &Path, number: Option<u64>) -> Option<(String, String)> {
    load_issue_record(repo, number).map(|draft| (draft.title, draft.body))
}

fn no_picks() -> IssueCreateFields {
    IssueCreateFields::default()
}

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
fn pr_comment_drafts_share_atomic_queue_with_isolated_keys_and_version_guards() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-comments");
        let clone = Path::new("/tmp/kagi-it/pr-comments-other-clone");
        let scope = PrCommentDraftSession::new(repo);
        let other_clone = PrCommentDraftSession::new(clone);
        let other = "github.com/other/widgets";
        queue_issue_draft(repo, Some(7), "", "Issue reply", &no_picks());
        let old = scope.queue(BASE, 7, "old PR comment 日本語");
        assert_eq!(
            scope.load(BASE, 7).as_deref(),
            Some("old PR comment 日本語")
        );
        let current = scope.queue(BASE, 7, "newer PR comment");
        let sibling = scope.queue(BASE, 8, "PR 8");
        scope.queue(other, 7, "other base repository");
        other_clone.queue(BASE, 7, "other clone");
        assert_eq!(scope.clear_if_version(BASE, 7, old), None);
        assert_eq!(other_clone.clear_if_version(BASE, 7, current), None);
        assert_eq!(scope.clear_if_version(BASE, 8, current), None);
        assert_eq!(scope.clear_if_version(other, 7, current), None);
        assert!(!other_clone.flush_if_version(BASE, 7, current).unwrap());
        assert!(!scope.flush_if_version(BASE, 8, current).unwrap());
        assert!(!scope.flush_if_version(other, 7, current).unwrap());
        assert!(!scope.flush_if_version(BASE, 7, old).unwrap());
        flush_issue_drafts().expect("shared shutdown flush");
        let stored = load_draft(repo, ":pr:github.com/acme/widgets:7").unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&stored.message).unwrap(),
            ["", "newer PR comment"]
        );
        assert_eq!(scope.load(BASE, 7).as_deref(), Some("newer PR comment"));
        let cleared = scope.clear_if_version(BASE, 7, current).unwrap();
        assert!(
            scope.load(BASE, 7).is_none(),
            "pending clear wins over disk"
        );
        assert_eq!(scope.clear_if_version(BASE, 7, current), None);
        assert!(scope.flush_if_version(BASE, 7, cleared).unwrap());
        assert!(load_draft(repo, ":pr:github.com/acme/widgets:7").is_none());
        assert_eq!(load_issue_draft(repo, Some(7)).unwrap().1, "Issue reply");
        assert_eq!(scope.load(BASE, 8).as_deref(), Some("PR 8"));
        assert_eq!(
            scope.load(other, 7).as_deref(),
            Some("other base repository")
        );
        assert_eq!(other_clone.load(BASE, 7).as_deref(), Some("other clone"));
        assert!(!scope.flush_if_version(BASE, 8, sibling).unwrap());
    });
}

#[test]
fn pr_comment_live_scopes_restore_independently_and_clear_only_the_posted_slot() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-live-scopes");
        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        let first_version = first.queue(BASE, 7, "first owner's draft 日本語");
        let second_version = second.queue(BASE, 7, "second owner's draft");
        assert_ne!(first_version, second_version);
        assert_eq!(first.version(BASE, 7), first_version);
        assert_eq!(second.version(BASE, 7), second_version);
        assert_eq!(second.clear_if_version(BASE, 7, first_version), None);
        assert_eq!(first.clear_if_version(BASE, 7, second_version), None);
        assert!(!second.flush_if_version(BASE, 7, first_version).unwrap());
        assert!(!first.flush_if_version(BASE, 7, second_version).unwrap());
        flush_issue_drafts().expect("persist both slots");
        drop((first, second));

        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        assert_eq!(
            first.load(BASE, 7).as_deref(),
            Some("first owner's draft 日本語")
        );
        assert_eq!(
            second.load(BASE, 7).as_deref(),
            Some("second owner's draft")
        );
        let posted = first.version(BASE, 7);
        let edited = second.queue(BASE, 7, "second owner's newer draft");
        assert_eq!(first.version(BASE, 7), posted, "other input is independent");
        assert_eq!(second.version(BASE, 7), edited);
        let clear = first.clear_if_version(BASE, 7, posted).expect("first post");
        assert!(first.load(BASE, 7).is_none());
        assert_eq!(
            second.load(BASE, 7).as_deref(),
            Some("second owner's newer draft")
        );
        assert_eq!(second.clear_if_version(BASE, 7, clear), None);
        assert!(!second.flush_if_version(BASE, 7, clear).unwrap());
        assert!(first.flush_if_version(BASE, 7, clear).unwrap());
        assert!(second.flush_if_version(BASE, 7, edited).unwrap());
        drop((first, second));

        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        assert!(first.load(BASE, 7).is_none(), "cleared slot stays cleared");
        assert_eq!(
            second.load(BASE, 7).as_deref(),
            Some("second owner's newer draft")
        );
    });
}

#[test]
fn pr_comment_cloned_scope_keeps_its_slot_until_last_drop() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-leases");
        let first = PrCommentDraftSession::new(repo);
        let version = first.queue(BASE, 7, "admitted post");
        let admitted = first.clone();
        drop(first);
        let second = PrCommentDraftSession::new(repo);
        assert!(second.load(BASE, 7).is_none());
        second.queue(BASE, 7, "new session");
        assert_eq!(admitted.repo(), repo);
        assert_eq!(admitted.version(BASE, 7), version);
        assert_eq!(admitted.load(BASE, 7).as_deref(), Some("admitted post"));
        drop(admitted);

        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(reopened.load(BASE, 7).as_deref(), Some("admitted post"));
        assert_eq!(second.load(BASE, 7).as_deref(), Some("new session"));
        drop(second);
        let reopened_second = PrCommentDraftSession::new(repo);
        assert_eq!(
            reopened_second.load(BASE, 7).as_deref(),
            Some("new session")
        );
        assert_ne!(reopened.version(BASE, 7), reopened_second.version(BASE, 7));
        flush_issue_drafts().expect("persist reclaimed slots");
    });
}

#[test]
fn pr_comment_scopes_recover_failed_pending_writes_after_lease_reclamation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/pr-pending-recovery");
        let blocked = log_dir.join("drafts");
        std::fs::write(&blocked, "not a directory").expect("block persistence");
        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        let first_version = first.queue(BASE, 7, "first retained failure");
        let second_version = second.queue(BASE, 7, "second retained failure");
        assert!(first.flush_if_version(BASE, 7, first_version).is_err());
        assert!(second.flush_if_version(BASE, 7, second_version).is_err());
        drop((first, second));

        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        assert_eq!(
            first.load(BASE, 7).as_deref(),
            Some("first retained failure")
        );
        assert_eq!(
            second.load(BASE, 7).as_deref(),
            Some("second retained failure")
        );
        std::fs::remove_file(blocked).expect("unblock persistence");
        assert!(first.flush_if_version(BASE, 7, first_version).unwrap());
        assert!(second.flush_if_version(BASE, 7, second_version).unwrap());
        drop((first, second));

        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        assert_eq!(
            first.load(BASE, 7).as_deref(),
            Some("first retained failure")
        );
        assert_eq!(
            second.load(BASE, 7).as_deref(),
            Some("second retained failure")
        );
    });
}

#[test]
fn pr_comment_scopes_capture_root_and_reject_foreign_root_tokens() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|original| {
        let repo = Path::new("/tmp/kagi-it/pr-root");
        let first = PrCommentDraftSession::new(repo);
        let posted = first.queue(BASE, 7, "original storage");
        let other_root = tempfile::tempdir().expect("other root");
        std::env::set_var("KAGI_LOG_DIR", other_root.path());
        let second = PrCommentDraftSession::new(repo);
        assert!(second.load(BASE, 7).is_none());
        let other = second.queue(BASE, 7, "other storage");
        assert_eq!(second.clear_if_version(BASE, 7, posted), None);
        assert_eq!(first.clear_if_version(BASE, 7, other), None);
        assert!(!second.flush_if_version(BASE, 7, posted).unwrap());
        assert!(!first.flush_if_version(BASE, 7, other).unwrap());
        assert!(first.flush_if_version(BASE, 7, posted).unwrap());
        assert!(second.flush_if_version(BASE, 7, other).unwrap());
        assert_eq!(first.load(BASE, 7).as_deref(), Some("original storage"));
        assert_eq!(second.load(BASE, 7).as_deref(), Some("other storage"));
        let clear = first.clear_if_version(BASE, 7, posted).unwrap();
        assert!(first.flush_if_version(BASE, 7, clear).unwrap());
        assert_eq!(second.load(BASE, 7).as_deref(), Some("other storage"));
        drop((first, second));
        let second = PrCommentDraftSession::new(repo);
        assert_eq!(second.load(BASE, 7).as_deref(), Some("other storage"));
        std::env::set_var("KAGI_LOG_DIR", original);
        let first = PrCommentDraftSession::new(repo);
        assert!(first.load(BASE, 7).is_none());
    });
}

#[test]
fn pr_comment_concurrent_claims_keep_the_same_clone_independent() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-concurrent");
        let barrier = std::sync::Barrier::new(2);
        let (first, second) = std::thread::scope(|threads| {
            let first = threads.spawn(|| {
                let scope = PrCommentDraftSession::new(repo);
                barrier.wait();
                scope.queue(BASE, 7, "first thread");
                scope
            });
            let second = threads.spawn(|| {
                let scope = PrCommentDraftSession::new(repo);
                barrier.wait();
                scope.queue(BASE, 7, "second thread");
                scope
            });
            (
                first.join().expect("first claim"),
                second.join().expect("second claim"),
            )
        });
        assert_ne!(first.version(BASE, 7), second.version(BASE, 7));
        assert_eq!(first.load(BASE, 7).as_deref(), Some("first thread"));
        assert_eq!(second.load(BASE, 7).as_deref(), Some("second thread"));
        flush_issue_drafts().expect("persist both concurrent claims");
    });
}

#[test]
fn pr_comment_slot_zero_restores_legacy_files_but_rejects_foreign_record_identity() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/pr-legacy");
        let branch = ":pr:github.com/acme/widgets:7";
        let message = serde_json::json!(["", "legacy PR draft"]).to_string();
        save_draft(repo, branch, &message, "issue-composer").expect("pre-scope draft");
        let scope = PrCommentDraftSession::new(repo);
        assert_eq!(scope.load(BASE, 7).as_deref(), Some("legacy PR draft"));
        let file = std::fs::read_dir(log_dir.join("drafts"))
            .expect("draft directory")
            .next()
            .expect("draft entry")
            .expect("read draft entry")
            .path();
        let original = std::fs::read(&file).expect("draft bytes");
        let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
        record["repo"] = serde_json::json!("/another/clone");
        std::fs::write(&file, record.to_string()).expect("wrong clone metadata");
        assert!(scope.load(BASE, 7).is_none());
        record = serde_json::from_slice(&original).unwrap();
        record["branch"] = serde_json::json!(":pr-slot:1:github.com/acme/widgets:7");
        std::fs::write(&file, record.to_string()).expect("wrong scoped metadata");
        assert!(scope.load(BASE, 7).is_none());
        std::fs::write(file, original).expect("restore original record");
        assert_eq!(scope.load(BASE, 7).as_deref(), Some("legacy PR draft"));
    });
}

#[test]
fn pr_comment_single_reopen_recovers_orphan_without_reverse_migration_cycles() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-single-recovery");
        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        let posted = first.queue(BASE, 7, "posted slot zero");
        second.queue(BASE, 7, "remaining slot one");
        flush_issue_drafts().expect("persist both scopes");
        first
            .clear_if_version(BASE, 7, posted)
            .expect("post completed");
        drop((first, second));

        let reopened = PrCommentDraftSession::new(repo);
        let before_load = reopened.version(BASE, 7);
        assert_eq!(
            reopened.load(BASE, 7).as_deref(),
            Some("remaining slot one")
        );
        assert_eq!(reopened.version(BASE, 7), before_load);
        // Claim the old source, then close the recovery destination before its
        // queued write. The source must not import back and form a cycle.
        let reverse = PrCommentDraftSession::new(repo);
        drop(reopened);
        assert!(reverse.load(BASE, 7).is_none());
        flush_issue_drafts().expect("destination precedes source without recursion cycle");
        assert!(
            reverse.load(BASE, 7).is_none(),
            "later load must not import"
        );
        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(
            reopened.load(BASE, 7).as_deref(),
            Some("remaining slot one")
        );
        drop((reopened, reverse));
        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(
            reopened.load(BASE, 7).as_deref(),
            Some("remaining slot one")
        );
        let sibling = PrCommentDraftSession::new(repo);
        assert!(
            sibling.load(BASE, 7).is_none(),
            "source is not offered twice"
        );
    });
}

#[test]
fn pr_comment_existing_scope_never_imports_a_later_closed_sibling() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-no-late-import");
        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        second.queue(BASE, 7, "sibling draft");
        assert!(first.load(BASE, 7).is_none(), "live sibling is private");
        drop(second);
        assert!(first.load(BASE, 7).is_none(), "Graph reload cannot import");
        drop(first);
        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(reopened.load(BASE, 7).as_deref(), Some("sibling draft"));
        flush_issue_drafts().expect("persist newly recovered draft");
    });
}

#[test]
fn pr_comment_orphan_source_survives_destination_failure_and_flushes_in_order() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/pr-recovery-write-failure");
        let first = PrCommentDraftSession::new(repo);
        let second = PrCommentDraftSession::new(repo);
        let posted = first.queue(BASE, 7, "posted");
        second.queue(BASE, 7, "durable orphan");
        flush_issue_drafts().expect("persist both scopes");
        let branch = ":pr:github.com/acme/widgets:7";
        let destination = drafts_dir_files(log_dir)
            .into_iter()
            .find(|(_, bytes)| {
                serde_json::from_slice::<serde_json::Value>(bytes).unwrap()["branch"] == branch
            })
            .map(|(name, _)| log_dir.join("drafts").join(name))
            .expect("slot zero file");
        let clear = first.clear_if_version(BASE, 7, posted).unwrap();
        first
            .flush_if_version(BASE, 7, clear)
            .expect("persist posted clear");
        drop((first, second));
        std::fs::create_dir(&destination).expect("block destination replacement");

        let recovered = PrCommentDraftSession::new(repo);
        assert_eq!(recovered.load(BASE, 7).as_deref(), Some("durable orphan"));
        let source = PrCommentDraftSession::new(repo);
        let source_version = source.version(BASE, 7);
        assert!(source.flush_if_version(BASE, 7, source_version).is_err());
        let source_branch = ":pr-slot:1:github.com/acme/widgets:7";
        assert!(
            load_draft(repo, source_branch).is_some(),
            "failed move keeps source"
        );
        assert!(flush_issue_drafts().is_err());
        assert!(
            load_draft(repo, source_branch).is_some(),
            "quit flush also keeps source"
        );
        std::fs::remove_dir(destination).expect("unblock destination");
        assert!(source.flush_if_version(BASE, 7, source_version).unwrap());
        assert!(load_draft(repo, source_branch).is_none());
        assert!(
            load_draft(repo, branch).is_some(),
            "destination persisted first"
        );
        drop((recovered, source));
        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(reopened.load(BASE, 7).as_deref(), Some("durable orphan"));
    });
}

#[test]
fn pr_comment_first_load_discovers_orphan_disk_slots_without_prior_claims() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/pr-restarted-disk-recovery");
        let orphan = ":pr-slot:4:github.com/acme/widgets:7";
        let message = serde_json::json!(["", "restarted orphan"]).to_string();
        save_draft(repo, orphan, &message, "issue-composer").expect("prior process draft");
        let scope = PrCommentDraftSession::new(repo);
        let before_load = scope.version(BASE, 7);
        assert_eq!(scope.load(BASE, 7).as_deref(), Some("restarted orphan"));
        assert_eq!(scope.version(BASE, 7), before_load);
        assert!(
            load_draft(repo, orphan).is_some(),
            "source preserved before flush"
        );
        flush_issue_drafts().expect("persist recovery before removing orphan");
        assert!(load_draft(repo, orphan).is_none());
        drop(scope);
        let reopened = PrCommentDraftSession::new(repo);
        assert_eq!(reopened.load(BASE, 7).as_deref(), Some("restarted orphan"));
    });
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
fn legacy_outer_record_preserves_surrogate_pairs() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/repo");
        save_draft(repo, "main", "seed", "plain").expect("locate draft storage");
        let file = std::fs::read_dir(log_dir.join("drafts"))
            .expect("draft directory")
            .next()
            .expect("draft entry")
            .expect("read entry")
            .path();
        std::fs::write(
            file,
            r#"{"repo":"/tmp/kagi-it/repo","branch":"main","message":"score \uD834\uDD1E\n\"quoted\"\\path\t\u0001","mode":"template","updated":42}"#,
        )
        .expect("write existing-format record");

        let draft = load_draft(repo, "main").expect("load existing draft");
        let expected = "score \u{1D11E}\n\"quoted\"\\path\t\u{1}";
        assert_eq!(draft.message, expected);
        assert_eq!(draft.repo, "/tmp/kagi-it/repo");
        assert_eq!(draft.branch, "main");
        assert_eq!(draft.mode, "template");
        assert_eq!(draft.updated, 42);
        save_draft(repo, &draft.branch, &draft.message, &draft.mode).expect("resave draft");
        assert_eq!(load_draft(repo, "main").expect("reload").message, expected);
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
        queue_issue_draft(repo, None, "old", "old body", &no_picks());
        queue_issue_draft(repo, None, title, body, &no_picks());
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
        let stored = load_draft(repo, NEW).expect("common draft record");
        assert_eq!(stored.mode, "issue-composer");
        assert_eq!(
            serde_json::from_str::<(String, String)>(&stored.message).expect("tuple payload"),
            (title.to_owned(), body.to_owned())
        );
    });
}

#[test]
fn whitespace_only_issue_draft_clears_and_legacy_whitespace_loads_empty() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-whitespace");
        queue_issue_draft(repo, None, "title", "saved body", &no_picks());
        flush_issue_drafts().expect("save initial draft");

        queue_issue_draft(repo, None, " \t ", "\n  \r\n", &no_picks());
        assert!(
            load_issue_draft(repo, None).is_none(),
            "pending whitespace-only tuple is an empty draft"
        );
        flush_issue_drafts().expect("delete whitespace-only draft");
        assert!(load_draft(repo, NEW).is_none());

        let legacy = serde_json::json!([" \t", "\n  "]).to_string();
        save_draft(repo, REPLY_7, &legacy, "issue-composer").expect("legacy whitespace tuple");
        assert!(
            load_issue_draft(repo, Some(7)).is_none(),
            "legacy whitespace-only tuple reads as empty"
        );
    });
}

#[test]
fn meaningful_issue_body_preserves_surrounding_whitespace() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-whitespace-preserved");
        let title = " \t ";
        let body = "  meaningful body  \n\n  ";
        queue_issue_draft(repo, Some(7), title, body, &no_picks());
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some((title.into(), body.into()))
        );
        flush_issue_drafts().expect("persist meaningful body exactly");
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some((title.into(), body.into()))
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
        queue_issue_draft(a, None, "new", "new body", &no_picks());
        queue_issue_draft(a, Some(7), "", "reply seven", &no_picks());
        queue_issue_draft(a, Some(8), "", "reply eight", &no_picks());
        queue_issue_draft(b, Some(7), "", "other repo reply", &no_picks());
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
        queue_issue_draft(repo, None, "title", "saved body", &no_picks());
        flush_issue_drafts().expect("initial save");
        queue_issue_draft(repo, None, "title", "unsaved later body", &no_picks());
        queue_issue_draft(repo, None, "", "", &no_picks());
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
        assert!(load_draft(repo, NEW).is_none());
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
        queue_issue_draft(repo, None, "original", "original body", &no_picks());
        std::env::set_var("KAGI_LOG_DIR", second.path());
        assert!(load_issue_draft(repo, None).is_none());
        queue_issue_draft(repo, None, "second", "second body", &no_picks());
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
        queue_issue_draft(repo, None, "title", "retry body", &no_picks());
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
        assert!(load_draft(repo, NEW).is_some());
    });
}

#[test]
fn keyed_issue_flush_does_not_borrow_another_storage_error() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|working| {
        let blocked_root = tempfile::tempdir().expect("blocked storage parent");
        let blocked = blocked_root.path().join("not-a-directory");
        std::fs::write(&blocked, "block directory creation").expect("create blocker");
        let failed_repo = Path::new("/tmp/kagi-it/issue-keyed-failed");
        std::env::set_var("KAGI_LOG_DIR", &blocked);
        let failed = queue_issue_draft(failed_repo, None, "failed", "retained body", &no_picks());

        let saved_repo = Path::new("/tmp/kagi-it/issue-keyed-saved");
        std::env::set_var("KAGI_LOG_DIR", working);
        let stale = queue_issue_draft(saved_repo, Some(7), "", "stale reply", &no_picks());
        assert!(!flush_issue_draft_if_version(saved_repo, Some(8), stale)
            .expect("wrong Issue is not an error"));
        assert_eq!(
            load_issue_draft(saved_repo, Some(7)),
            Some((String::new(), "stale reply".into())),
            "wrong Issue must not consume the pending value"
        );
        let saved = queue_issue_draft(saved_repo, Some(7), "", "saved reply", &no_picks());
        assert!(!flush_issue_draft_if_version(saved_repo, Some(7), stale)
            .expect("superseded timer is not an error"));
        assert_eq!(
            load_issue_draft(saved_repo, Some(7)),
            Some((String::new(), "saved reply".into())),
            "stale version must not persist or consume the latest value"
        );
        assert!(flush_issue_draft_if_version(saved_repo, Some(7), saved)
            .expect("unrelated valid destination must flush"));
        assert!(!flush_issue_draft_if_version(saved_repo, Some(7), saved)
            .expect("an already-flushed version is a no-op"));
        assert_eq!(
            load_issue_draft(saved_repo, Some(7)),
            Some((String::new(), "saved reply".into()))
        );

        assert!(flush_issue_draft_if_version(failed_repo, None, failed).is_err());
        std::env::set_var("KAGI_LOG_DIR", &blocked);
        assert_eq!(
            load_issue_draft(failed_repo, None),
            Some(("failed".into(), "retained body".into()))
        );

        std::fs::remove_file(&blocked).expect("remove blocker");
        assert!(flush_issue_draft_if_version(failed_repo, None, failed)
            .expect("retry retained failed destination"));
        assert_eq!(
            load_issue_draft(failed_repo, None),
            Some(("failed".into(), "retained body".into()))
        );
    });
}

#[test]
fn issue_load_rejects_wrong_mode_and_malformed_payload() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-corrupt");
        save_draft(repo, NEW, "[\"t\",\"b\"]", "plain").expect("wrong mode");
        assert!(load_issue_draft(repo, None).is_none());
        save_draft(repo, NEW, "not a tuple", "issue-composer").expect("bad payload");
        assert!(load_issue_draft(repo, None).is_none());
    });
}

fn picks(labels: &[&str], assignees: &[&str]) -> IssueCreateFields {
    IssueCreateFields {
        labels: labels.iter().map(|s| s.to_string()).collect(),
        assignees: assignees.iter().map(|s| s.to_string()).collect(),
    }
}

/// Every file in the drafts directory, by name, with its bytes.
fn drafts_dir_files(log_dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(log_dir.join("drafts"))
        .expect("draft directory")
        .map(|entry| {
            let path = entry.expect("read entry").path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).expect("read draft file"),
            )
        })
        .collect();
    files.sort();
    files
}

/// #903: a New Issue draft keeps its picked labels and assignees, and a file
/// written before #903 (`[title, body]`) still loads — with no picks — and is
/// replaced in place, not set aside as unreadable.
#[test]
fn issue_draft_keeps_picks_and_reads_the_title_body_format() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/issue-picks");
        save_draft(repo, NEW, r#"["old title","old body"]"#, "issue-composer")
            .expect("pre-#903 draft");
        assert_eq!(
            load_issue_record(repo, None),
            Some(IssueDraftRecord {
                title: "old title".into(),
                body: "old body".into(),
                fields: no_picks(),
            })
        );

        let chosen = picks(&["bug", "Bug"], &["octocat"]);
        queue_issue_draft(repo, None, "old title", "old body", &chosen);
        flush_issue_drafts().expect("save picks");
        let files = drafts_dir_files(log_dir);
        assert_eq!(
            files.len(),
            1,
            "readable draft replaced in place: {files:?}"
        );
        assert_eq!(
            load_issue_record(repo, None),
            Some(IssueDraftRecord {
                title: "old title".into(),
                body: "old body".into(),
                fields: chosen.clone(),
            })
        );

        // Picks without text are not a draft: they go with the text.
        queue_issue_draft(repo, None, "", " ", &chosen);
        flush_issue_drafts().expect("clear");
        assert!(load_issue_record(repo, None).is_none());
        assert!(load_draft(repo, NEW).is_none());
    });
}

/// #903: a file at an Issue draft key that does not parse — garbage, or a
/// picks payload with a non-string label — is never replaced or deleted.
/// Saving moves it aside to `<file>.corrupt`, a second one to
/// `<file>.corrupt.1`, and neither copy is overwritten.
#[test]
fn unreadable_issue_draft_is_moved_aside_not_overwritten() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|log_dir| {
        let repo = Path::new("/tmp/kagi-it/issue-unreadable");
        save_draft(repo, NEW, "seed", "plain").expect("locate draft storage");
        let (name, _) = drafts_dir_files(log_dir).remove(0);
        let file = log_dir.join("drafts").join(&name);
        let torn = b"{\"repo\":\"/tmp/kagi-it/issue-unread".to_vec();
        std::fs::write(&file, &torn).unwrap();
        assert!(load_issue_record(repo, None).is_none(), "lenient load");

        queue_issue_draft(repo, None, "new title", "new body", &no_picks());
        flush_issue_drafts().expect("save beside the unreadable file");
        assert_eq!(
            load_issue_draft(repo, None),
            Some(("new title".into(), "new body".into()))
        );

        let bad_picks = serde_json::json!({
            "repo": "/tmp/kagi-it/issue-unreadable",
            "branch": NEW,
            "message": r#"["t","b",[1],[]]"#,
            "mode": "issue-composer",
        })
        .to_string()
        .into_bytes();
        std::fs::write(&file, &bad_picks).unwrap();
        assert!(
            load_issue_record(repo, None).is_none(),
            "a malformed picks payload is not a draft"
        );
        // A clear must not delete it either.
        queue_issue_draft(repo, None, "", "", &no_picks());
        flush_issue_drafts().expect("clear beside the unreadable file");

        assert_eq!(
            drafts_dir_files(log_dir),
            vec![
                (format!("{name}.corrupt"), torn),
                (format!("{name}.corrupt.1"), bad_picks),
            ],
            "both unreadable files kept byte for byte; the cleared draft is gone"
        );
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
        let posted = queue_issue_draft(repo, None, "title", "submitted body", &no_picks());
        assert!(posted > initial);
        flush_issue_drafts().expect("save posted body");
        assert_eq!(posted, issue_draft_version(repo, None));
        assert!(clear_issue_draft_if_version(repo, None, posted).is_some());
        assert!(issue_draft_version(repo, None) > posted);
        assert!(clear_issue_draft_if_version(repo, None, posted).is_none());
        assert!(load_issue_draft(repo, None).is_none());
        flush_issue_drafts().expect("persist clear");
        std::thread::spawn(flush_issue_drafts)
            .join()
            .expect("late autosave thread")
            .expect("late autosave flush");
        assert!(load_draft(repo, NEW).is_none());
    });
}

#[test]
fn reopened_editor_new_version_protects_draft_from_old_post_completion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-reopened");
        let posted = queue_issue_draft(repo, Some(7), "", "submitted reply", &no_picks());
        flush_issue_drafts().expect("save before close");
        // Closing a tab does not erase the storage token. A reopened editor
        // reads it, then input obtains a fresh token even for identical text.
        assert_eq!(issue_draft_version(repo, Some(7)), posted);
        let edited = queue_issue_draft(repo, Some(7), "", "submitted reply", &no_picks());
        assert!(edited > posted);
        assert!(clear_issue_draft_if_version(repo, Some(7), posted).is_none());
        assert_eq!(issue_draft_version(repo, Some(7)), edited);
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some(("".into(), "submitted reply".into()))
        );
        flush_issue_drafts().expect("persist reopened draft");
        assert!(clear_issue_draft_if_version(repo, Some(7), posted).is_none());
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
        let posted = queue_issue_draft(repo, Some(7), "", "original reply", &no_picks());
        flush_issue_drafts().expect("save original");
        let second = tempfile::tempdir().expect("second storage");
        std::env::set_var("KAGI_LOG_DIR", second.path());
        let other = queue_issue_draft(repo, Some(7), "", "other storage reply", &no_picks());
        assert!(clear_issue_draft_if_version(repo, Some(8), posted).is_none());
        assert!(clear_issue_draft_if_version(Path::new("/different"), Some(7), posted).is_none());
        assert!(clear_issue_draft_if_version(repo, Some(7), posted).is_some());
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

/// #940 review: a clone can address another repository after `gh repo
/// set-default`, where the same number is another Issue. A draft written for
/// one repository's #7 is not offered for the other's #7, and writing there
/// does not replace it.
#[test]
fn issue_draft_is_kept_per_repository_written_to() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let repo = Path::new("/tmp/kagi-it/issue-per-repository");
        let upstream = "github.com/upstream/widgets";
        queue_addressed(repo, upstream, Some(7), "", "for upstream", &no_picks());
        queue_addressed(repo, upstream, None, "title", "new upstream", &no_picks());
        flush_issue_drafts().expect("save upstream drafts");
        assert_eq!(
            load_addressed(repo, BASE, Some(7)),
            IssueDraftLoad::default()
        );
        assert_eq!(load_addressed(repo, BASE, None), IssueDraftLoad::default());

        queue_issue_draft(repo, Some(7), "", "for widgets", &no_picks());
        flush_issue_drafts().expect("save widgets draft");
        assert_eq!(
            load_addressed(repo, upstream, Some(7))
                .record
                .map(|d| d.body),
            Some("for upstream".to_string())
        );
        assert_eq!(
            load_issue_draft(repo, Some(7)),
            Some(("".into(), "for widgets".into()))
        );
    });
}

/// A clone whose remotes are `urls`, for drafts saved before #940.
fn clone_with_remotes(urls: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("clone dir");
    let repo = git2::Repository::init(dir.path()).expect("init");
    for (ix, url) in urls.iter().enumerate() {
        repo.remote(&format!("r{ix}"), url).expect("remote");
    }
    dir
}

/// A draft saved before #940 does not say which repository it was written
/// to. It moves, once, to the one repository the clone's remotes name; with
/// two to choose from it stays where it is, untouched, and the load says so.
#[test]
fn unaddressed_draft_moves_only_to_the_sole_remote_repository() {
    if !crate::test_support::run_isolated() {
        return;
    }
    with_log_dir(|_| {
        let two = clone_with_remotes(&[
            "https://github.com/acme/widgets.git",
            "https://github.com/upstream/widgets.git",
        ]);
        save_draft(
            two.path(),
            ":issue:7",
            r#"["","old reply"]"#,
            "issue-composer",
        )
        .expect("pre-#940 draft");
        let before = load_draft(two.path(), ":issue:7").expect("saved");
        assert_eq!(
            load_addressed(two.path(), BASE, Some(7)),
            IssueDraftLoad {
                record: None,
                kept_unaddressed: true,
            },
            "either remote may be where it was written"
        );
        flush_issue_drafts().expect("flush");
        assert_eq!(load_draft(two.path(), ":issue:7"), Some(before));

        let one = clone_with_remotes(&[
            "https://github.com/acme/widgets.git",
            "git@github.com:acme/widgets.git",
        ]);
        save_draft(
            one.path(),
            ":issue:7",
            r#"["","old reply"]"#,
            "issue-composer",
        )
        .expect("pre-#940 draft");
        assert_eq!(
            load_addressed(one.path(), "github.com/upstream/widgets", Some(7)),
            IssueDraftLoad {
                record: None,
                kept_unaddressed: true,
            },
            "not the repository the remotes name"
        );
        assert_eq!(
            load_addressed(one.path(), BASE, Some(7))
                .record
                .map(|d| d.body),
            Some("old reply".to_string())
        );
        flush_issue_drafts().expect("persist the move");
        assert!(load_draft(one.path(), ":issue:7").is_none(), "offered once");
        assert_eq!(
            load_addressed(one.path(), BASE, Some(7))
                .record
                .map(|d| d.body),
            Some("old reply".to_string())
        );
    });
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
