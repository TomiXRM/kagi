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
        worker
            .submit_with_policy(op.clone(), plan.clone(), policy)
            .unwrap()
            .recv()
            .unwrap()
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
            .is_err());
        assert_eq!(unchanged(tmp.path()), before);
        assert_eq!(kagi_git::read_oplog_tail(1)[0].actor, Actor::Mcp);
    }
    worker.shutdown();
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn optional_snapshot_off_does_not_disable_restore_recovery() {
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
