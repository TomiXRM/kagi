//! Window-free execution-policy and mandatory-boundary regressions (#494/#502).
use kagi_git::backend::ExecutionPolicy;
use kagi_git::{Actor, AmendMode, Backend, CommitId, GitError, Operation};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());
fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(path)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    git(p, &["init", "-q", "-b", "topic"]);
    git(p, &["config", "user.name", "Test"]);
    git(p, &["config", "user.email", "test@example.com"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::write(p.join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "base"]);
    git(p, &["branch", "base"]);
    std::fs::write(p.join("b.txt"), "other\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "topic"]);
    git(p, &["checkout", "-q", "base"]);
    std::fs::write(p.join("c.txt"), "base advance\n").unwrap();
    git(p, &["add", "."]);
    git(p, &["commit", "-qm", "advance base"]);
    git(p, &["checkout", "-q", "topic"]);
    tmp
}
fn unchanged(path: &Path) -> (String, String, Vec<u8>, Vec<u8>) {
    (
        git(path, &["show-ref"]),
        git(path, &["status", "--porcelain"]),
        std::fs::read(path.join(".git/index")).unwrap(),
        std::fs::read(path.join("a.txt")).unwrap(),
    )
}

#[test]
fn snapshot_policy_matches_direct_and_worker_for_rewriting_families() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    for enabled in [false, true] {
        for worker in [false, true] {
            for family in ["reset", "amend", "rebase"] {
                let tmp = fixture();
                let policy = ExecutionPolicy::human(enabled);
                let mut backend = Backend::open_with_policy(tmp.path(), policy).unwrap();
                let op = match family {
                    "reset" => Operation::ResetCurrentToHead {
                        target: CommitId(git(tmp.path(), &["rev-parse", "base"])),
                    },
                    "amend" => Operation::Amend {
                        mode: AmendMode::MessageOnly,
                        message: Some("changed message".into()),
                    },
                    _ => Operation::RebaseCurrentOnto {
                        onto: "base".into(),
                    },
                };
                let plan = backend.plan(&op).unwrap();
                assert!(plan.blockers.is_empty(), "{family}: {:?}", plan.blockers);
                let expected = enabled && plan.destructive;
                if worker {
                    let mut worker = kagi_git::worker::RepoWorker::spawn(tmp.path()).unwrap();
                    worker
                        .submit_with_policy(op, plan, policy)
                        .unwrap()
                        .recv()
                        .unwrap()
                        .unwrap();
                    worker.shutdown();
                } else {
                    backend.run(&op, &plan).unwrap();
                }
                assert_eq!(
                    backend.list_snapshots().unwrap().len(),
                    usize::from(expected),
                    "{family}, enabled={enabled}, worker={worker}"
                );
            }
        }
    }
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn explicit_cli_mcp_policy_records_actor_and_does_not_read_gui_settings() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    std::fs::write(
        log.path().join("settings.json"),
        r#"{"auto_snapshot":"false"}"#,
    )
    .unwrap();
    for policy in [ExecutionPolicy::cli(), ExecutionPolicy::mcp()] {
        let tmp = fixture();
        let mut backend = Backend::discover_with_policy(tmp.path(), policy).unwrap();
        let op = Operation::Amend {
            mode: AmendMode::MessageOnly,
            message: Some("API amend".into()),
        };
        let plan = backend.plan(&op).unwrap();
        let report = backend.run_recorded(&op, &plan);
        report.result.unwrap();
        assert_eq!(backend.list_snapshots().unwrap().len(), 1);
        assert_eq!(kagi_git::read_oplog_tail(1)[0].actor, policy.actor);
        assert_ne!(policy.actor, Actor::Human);
    }
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn untrusted_snapshot_delete_and_absorb_leave_refs_index_and_worktree_unchanged() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    let mut backend = Backend::open(tmp.path()).unwrap();
    let snapshot = backend.create_snapshot("keep me").unwrap();
    std::fs::write(tmp.path().join("a.txt"), "alpha\nBETA\ngamma\n").unwrap();
    let plan = backend.plan_absorb(10).unwrap();
    assert!(!plan.has_blockers());
    let before = unchanged(tmp.path());
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    assert!(backend
        .delete_snapshot(&snapshot.id)
        .unwrap_err()
        .is_untrusted());
    assert!(backend.execute_absorb(&plan).unwrap_err().is_untrusted());
    assert_eq!(unchanged(tmp.path()), before);
    assert_eq!(backend.list_snapshots().unwrap().len(), 1);
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn absorb_snapshot_toggle_and_stale_preflight_are_owned_by_boundary() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    for enabled in [false, true] {
        let tmp = fixture();
        let backend =
            Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(enabled)).unwrap();
        std::fs::write(tmp.path().join("a.txt"), "alpha\nBETA\ngamma\n").unwrap();
        let plan = backend.plan_absorb(10).unwrap();
        let outcome = backend.execute_absorb(&plan).unwrap();
        assert_eq!(git(tmp.path(), &["rev-parse", "HEAD"]), outcome.new_head);
        assert_eq!(
            backend.list_snapshots().unwrap().len(),
            usize::from(enabled)
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "alpha\nBETA\ngamma\n"
        );
        let before = unchanged(tmp.path());
        assert!(backend.execute_absorb(&plan).is_err());
        assert_eq!(unchanged(tmp.path()), before);
        assert_eq!(
            backend.list_snapshots().unwrap().len(),
            usize::from(enabled)
        );
    }
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn run_refuses_plan_with_omitted_safety_requirements_before_savepoint() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    let mut backend = Backend::open(tmp.path()).unwrap();
    let op = Operation::Amend {
        mode: AmendMode::MessageOnly,
        message: Some("changed".into()),
    };
    let mut plan = backend.plan(&op).unwrap();
    assert!(plan.destructive);
    plan.destructive = false;
    let before = unchanged(tmp.path());
    assert!(matches!(
        backend.run(&op, &plan),
        Err(GitError::Preflight(_))
    ));
    assert_eq!(unchanged(tmp.path()), before);
    assert!(backend.list_snapshots().unwrap().is_empty());
    let op = Operation::MergeBranch {
        target: "base".into(),
    };
    let mut plan = backend.plan(&op).unwrap();
    assert!(plan.worktree_digest.is_some());
    plan.worktree_digest = None;
    assert!(matches!(
        backend.run(&op, &plan),
        Err(GitError::Preflight(_))
    ));
    assert_eq!(unchanged(tmp.path()), before);
    let op = Operation::CreateBranch {
        name: "new-branch".into(),
        at: CommitId(git(tmp.path(), &["rev-parse", "HEAD"])),
    };
    let plan = backend.plan(&op).unwrap();
    let different = Operation::CreateBranch {
        name: "wrong-branch".into(),
        at: CommitId(git(tmp.path(), &["rev-parse", "HEAD"])),
    };
    assert!(matches!(
        backend.run(&different, &plan),
        Err(GitError::Preflight(_))
    ));
    assert_eq!(unchanged(tmp.path()), before);
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn one_worker_applies_new_policy_each_time_and_refuses_stale_requests() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    let backend = Backend::open(tmp.path()).unwrap();
    let mut worker = kagi_git::worker::RepoWorker::spawn(tmp.path()).unwrap();
    for enabled in [false, true] {
        let op = Operation::Amend {
            mode: AmendMode::MessageOnly,
            message: Some(format!("enabled={enabled}")),
        };
        let plan = backend.plan(&op).unwrap();
        let policy = ExecutionPolicy {
            actor: Actor::Mcp,
            auto_snapshot: enabled,
        };
        // #643 A1: the worker now replies with the whole `RunReport`. The outer
        // `Result` says whether the backend opened; the operation's own result
        // is inside it, alongside the recording.
        worker
            .submit_with_policy(op.clone(), plan.clone(), policy)
            .unwrap()
            .recv()
            .unwrap()
            .expect("the backend must open")
            .result
            .unwrap();
        assert_eq!(
            backend.list_snapshots().unwrap().len(),
            usize::from(enabled)
        );
        let before = unchanged(tmp.path());
        assert!(worker
            .submit_with_policy(op, plan, policy)
            .unwrap()
            .recv()
            .unwrap()
            .expect("the backend still opens; the stale request is what is refused")
            .result
            .is_err());
        assert_eq!(unchanged(tmp.path()), before);
        assert_eq!(kagi_git::read_oplog_tail(1)[0].actor, Actor::Mcp);
    }
    worker.shutdown();
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn optional_snapshot_off_does_not_disable_restore_recovery() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    let mut backend = Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
    let saved = backend.create_snapshot("restore target").unwrap();
    std::fs::write(tmp.path().join("a.txt"), "precious edits\n").unwrap();
    let op = Operation::RestoreSnapshot { id: saved.id };
    let plan = backend.plan(&op).unwrap();
    let outcome = backend.run(&op, &plan).unwrap();
    let kagi_git::OperationOutcome::RestoreSnapshot { savepoint } = outcome else {
        panic!("restore outcome")
    };
    assert_eq!(backend.list_snapshots().unwrap().len(), 2);
    let reference = format!("refs/kagi/snapshots/{savepoint}:a.txt");
    assert_eq!(git(tmp.path(), &["show", &reference]), "precious edits");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "alpha\nbeta\ngamma\n"
    );
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn post_plan_dirty_checkout_refuses_before_creating_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    let previous = CommitId(git(tmp.path(), &["rev-parse", "HEAD"]));
    std::fs::write(tmp.path().join("a.txt"), "second\n").unwrap();
    git(tmp.path(), &["add", "a.txt"]);
    git(tmp.path(), &["commit", "-qm", "change checkout path"]);
    let mut backend = Backend::open(tmp.path()).unwrap();
    let op = Operation::CreateBranchWithCheckout {
        name: "must-not-exist".into(),
        at: previous,
        checkout_after: true,
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty());
    std::fs::write(tmp.path().join("a.txt"), "precious dirty edits\n").unwrap();
    let before = unchanged(tmp.path());
    assert!(matches!(
        backend.run(&op, &plan),
        Err(GitError::Preflight(_))
    ));
    assert_eq!(unchanged(tmp.path()), before);
    assert!(backend.list_snapshots().unwrap().is_empty());
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn absorb_index_write_failure_records_partial_actual_head_and_recovery_oid() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let tmp = fixture();
    std::fs::write(tmp.path().join("a.txt"), "alpha\nBETA\ngamma\n").unwrap();
    let backend = Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_absorb(10).unwrap();
    assert!(!plan.is_noop());
    let old_head = backend.head_commit_id().unwrap().0;
    let old_index = std::fs::read(tmp.path().join(".git/index")).unwrap();
    let old_worktree = std::fs::read(tmp.path().join("a.txt")).unwrap();
    std::fs::write(tmp.path().join(".git/index.lock"), b"fixture lock").unwrap();
    let error = backend.execute_absorb(&plan).unwrap_err();
    assert!(error.to_string().contains("index.write failed"), "{error}");
    let actual_head = backend.head_commit_id().unwrap().0;
    assert_ne!(
        actual_head, old_head,
        "executor advanced the ref before failing"
    );
    assert_eq!(
        std::fs::read(tmp.path().join(".git/index")).unwrap(),
        old_index
    );
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        old_worktree
    );
    assert!(backend.list_snapshots().unwrap().is_empty());
    let entries = kagi_git::oplog::read_oplog_tail(100);
    assert_eq!(entries.len(), 1);
    let kagi_git::oplog::OpOutcome::Partial { after, error } = &entries[0].outcome else {
        panic!(
            "post-ref-update failure must be Partial: {:?}",
            entries[0].outcome
        )
    };
    assert_eq!(after.head, backend.current_state().unwrap().head);
    assert!(error.contains("index.write failed"));
    assert!(after.dirty.contains(&format!("before={old_head}")));
    assert!(after.dirty.contains(&format!("rebuilt={actual_head}")));
    assert!(after.dirty.contains("ref_updated=true"));
    assert!(after
        .dirty
        .contains("index_write_started=true; index_written=false"));
    assert_eq!(git(tmp.path(), &["rev-parse", &old_head]), old_head);
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn create_branch_checkbox_plan_matches_combined_operation_and_executes_once() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    for checkout_after in [false, true] {
        let log = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", log.path());
        let tmp = fixture();
        let mut backend =
            Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
        let at = CommitId(git(tmp.path(), &["rev-parse", "base"]));
        // The same planner used by the GUI checkbox, with the combined adapter
        // request (a plain CreateBranch incorrectly changes the fresh title).
        let plan = backend
            .plan_create_branch_with_checkout("created", &at, checkout_after)
            .unwrap();
        let op = Operation::CreateBranchWithCheckout {
            name: "created".into(),
            at: at.clone(),
            checkout_after,
        };
        assert_eq!(plan.title, backend.plan(&op).unwrap().title);
        backend.run(&op, &plan).unwrap();
        assert_eq!(git(tmp.path(), &["rev-parse", "created"]), at.0);
        assert_eq!(
            git(tmp.path(), &["branch", "--show-current"]),
            if checkout_after { "created" } else { "topic" }
        );
        assert_eq!(tmp.path().join("c.txt").exists(), checkout_after);
        let entries = kagi_git::oplog::read_oplog_tail(100);
        assert_eq!(entries.len(), 1, "no second checkout operation");
        assert_eq!(entries[0].op, "create-branch");
        assert!(matches!(
            entries[0].outcome,
            kagi_git::oplog::OpOutcome::Success { .. }
        ));
    }
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn worktree_config_identity_changes_are_refused_before_creation_or_copy() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let _guard = ENV.lock().unwrap();
    let config = "[[post_create]]\ntype='copy'\nfrom='a.txt'\nto='copied.txt'\n";
    for open_existing in [false, true] {
        for (before, after) in [
            (None, Some(config)),
            (Some(config), None),
            (
                Some(config),
                Some("# changed\n[[post_create]]\ntype='copy'\nfrom='a.txt'\nto='other.txt'\n"),
            ),
        ] {
            let log = tempfile::tempdir().unwrap();
            std::env::set_var("KAGI_LOG_DIR", log.path());
            let tmp = fixture();
            // Keep config drift independent of worktree/index drift: this test
            // must fail because of required config identity, not a dirty gate.
            std::fs::write(tmp.path().join(".git/info/exclude"), ".kagi/\n").unwrap();
            std::fs::create_dir(tmp.path().join(".kagi")).unwrap();
            let config_path = tmp.path().join(".kagi/worktree.toml");
            if let Some(content) = before {
                std::fs::write(&config_path, content).unwrap();
            }
            let destination = tempfile::tempdir().unwrap();
            let linked = destination.path().join("linked");
            let mut backend =
                Backend::open_with_policy(tmp.path(), ExecutionPolicy::human(false)).unwrap();
            let op = if open_existing {
                Operation::OpenWorktreeForBranch {
                    branch: "base".into(),
                    path: linked.display().to_string(),
                }
            } else {
                Operation::CreateWorktree {
                    branch: "created".into(),
                    path: linked.display().to_string(),
                    start: backend.head_commit_id().unwrap(),
                }
            };
            let plan = backend.plan(&op).unwrap();
            assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
            assert_eq!(
                kagi_git::ops::plan_worktree_config_sha(&plan).is_some(),
                before.is_some()
            );
            let unchanged_before = unchanged(tmp.path());
            match after {
                Some(content) => std::fs::write(&config_path, content).unwrap(),
                None => std::fs::remove_file(&config_path).unwrap(),
            }
            assert_eq!(unchanged(tmp.path()), unchanged_before);
            let error = backend.run(&op, &plan).unwrap_err();
            assert!(matches!(error, GitError::Preflight(_)), "{error}");
            assert!(
                error
                    .to_string()
                    .contains("plan safety requirements differ"),
                "{error}"
            );
            assert!(!linked.exists(), "no unreviewed post_create step ran");
            assert_eq!(unchanged(tmp.path()), unchanged_before);
            let entries = kagi_git::oplog::read_oplog_tail(100);
            assert_eq!(entries.len(), 1);
            assert!(matches!(
                entries[0].outcome,
                kagi_git::oplog::OpOutcome::Failed { .. }
            ));
        }
    }
    std::env::remove_var("KAGI_LOG_DIR");
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
