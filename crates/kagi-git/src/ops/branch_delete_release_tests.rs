use super::*;
use crate::backend::ExecutionPolicy;
use crate::{Backend, Operation};

fn fixture(log: bool) -> (tempfile::TempDir, Repository, git2::Oid) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.logAllRefUpdates", log)
        .unwrap();
    repo.set_head("refs/heads/main").unwrap();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tip = {
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "base", &tree, &[])
            .unwrap()
    };
    repo.reference("refs/heads/victim", tip, false, "fixture branch")
        .unwrap();
    (dir, repo, tip)
}

#[test]
fn commit_failure_after_reflog_removal_records_partial_and_recovery() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    let log = repo.path().join("logs/refs/heads/victim");
    assert!(log.exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    assert!(plan.blockers.is_empty());
    FAULT.with(|fault| fault.set(Some(DeleteFault::CommitFailure)));
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report
        .result
        .unwrap_err()
        .to_string()
        .contains("injected branch ref transaction failure"));
    assert!(!log.exists());
    assert_eq!(
        repo.find_reference("refs/heads/victim").unwrap().target(),
        Some(tip)
    );
    let entry = report.recording.entry();
    let crate::oplog::OpOutcome::Partial { after, error } = &entry.outcome else {
        panic!("{entry:?}");
    };
    assert!(after.dirty.contains("reflog removed"));
    assert!(error.contains("transaction failure"));
    assert_eq!(entry.backup_refs.len(), 1);
    assert_eq!(
        repo.find_reference(&entry.backup_refs[0]).unwrap().target(),
        Some(tip)
    );
    let persisted = crate::oplog::read_oplog_tail_for_repo(dir.path(), 10);
    assert!(persisted.iter().any(|e| e.id == entry.id
        && matches!(e.outcome, crate::oplog::OpOutcome::Partial { .. })
        && e.backup_refs == entry.backup_refs));
}

#[test]
fn branch_without_reflog_deletes_and_retains_tip() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(false);
    assert!(!repo.path().join("logs/refs/heads/victim").exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_ok());
    assert!(matches!(
        report.recording.entry().outcome,
        crate::oplog::OpOutcome::Success { .. }
    ));
    assert!(repo.find_reference("refs/heads/victim").is_err());
    assert_eq!(
        repo.find_reference(&report.recording.entry().backup_refs[0])
            .unwrap()
            .target(),
        Some(tip)
    );
}

#[test]
fn directly_written_ref_without_reflog_is_deletable() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    std::fs::write(repo.path().join("refs/heads/raw"), format!("{tip}\n")).unwrap();
    assert!(!repo.path().join("logs/refs/heads/raw").exists());
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("raw").unwrap();
    let report = backend.run_recorded(&Operation::DeleteBranch { name: "raw".into() }, &plan);
    assert!(report.result.is_ok());
    assert!(repo.find_reference("refs/heads/raw").is_err());
}

#[test]
fn reflog_errors_other_than_not_found_keep_branch_and_fail() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let (dir, repo, tip) = fixture(true);
    let path = repo.path().join("logs/refs/heads/victim");
    let mut backend = Backend::open_with_policy(dir.path(), ExecutionPolicy::human(false)).unwrap();
    let plan = backend.plan_delete_branch("victim").unwrap();
    FAULT.with(|fault| fault.set(Some(DeleteFault::ReflogFailure)));
    let report = backend.run_recorded(
        &Operation::DeleteBranch {
            name: "victim".into(),
        },
        &plan,
    );
    assert!(report.result.is_err());
    assert_eq!(
        repo.find_reference("refs/heads/victim").unwrap().target(),
        Some(tip)
    );
    assert!(path.is_file());
}
