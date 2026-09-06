//! #523: actual Git GC, persisted receipts and explicit recovery retirement.
use kagi_domain::remove::RemoveFaultPoint;
use kagi_git::backend::{recording::Recording, Backend};
use kagi_git::oplog::{append_oplog, read_oplog_tail, Actor, OpLogEntry, OpOutcome};
use kagi_git::{Operation, OperationOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());
struct Fixture {
    _lock: MutexGuard<'static, ()>,
    _root: tempfile::TempDir,
    repo: PathBuf,
    log: PathBuf,
    previous: Option<std::ffi::OsString>,
}
impl Fixture {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().canonicalize().unwrap().join("main");
        let log = root.path().join("log");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("tracked"), b"committed\n").unwrap();
        std::fs::write(repo.join(".gitignore"), b"secret\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        let previous = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", &log);
        Self {
            _lock: lock,
            _root: root,
            repo,
            log,
            previous,
        }
    }
    fn backend(&self) -> Backend {
        let mut backend = Backend::open(&self.repo).unwrap();
        // The mandatory backup must work independently of optional snapshots.
        backend.set_auto_snapshot(false);
        backend
    }
    fn discard(&self, content: &[u8]) -> OpLogEntry {
        std::fs::write(self.repo.join("tracked"), content).unwrap();
        let mut backend = self.backend();
        let op = Operation::Discard {
            paths: vec!["tracked".into()],
        };
        let plan = backend.plan(&op).unwrap();
        let report = backend.run_recorded(&op, &plan);
        let OperationOutcome::Discard(outcome) = report.result.unwrap() else {
            panic!("not discard")
        };
        assert!(!outcome.is_partial());
        let entry = report.recording.entry().clone();
        assert_eq!(
            entry.backup_refs,
            vec![outcome.backups[0].reference.clone()]
        );
        assert!(matches!(report.recording, Recording::Appended { .. }));
        assert_eq!(
            std::fs::read(self.repo.join("tracked")).unwrap(),
            b"committed\n"
        );
        entry
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
fn git(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn gc(repo: &Path) {
    // A sibling naked blob proves the fixture really exercises pruning.
    let raw = git2::Repository::open(repo).unwrap();
    let naked = raw.blob(b"unreferenced GC control #523\n").unwrap();
    drop(raw);
    git(repo, &["reflog", "expire", "--expire=now", "--all"]);
    git(repo, &["gc", "--prune=now"]);
    assert!(git2::Repository::open(repo)
        .unwrap()
        .find_blob(naked)
        .is_err());
}

#[test]
fn discard_receipt_recovers_from_ref_after_gc_without_auto_snapshot() {
    let f = Fixture::new();
    let content = b"unique discarded bytes\0\xff\n";
    let receipt = f.discard(content);
    let persisted = read_oplog_tail(1).remove(0);
    assert_eq!(persisted.backup_refs, receipt.backup_refs);
    gc(&f.repo);
    let name = &persisted.backup_refs[0];
    assert_eq!(f.backend().read_backup(name).unwrap(), content);
    assert_eq!(git(&f.repo, &["cat-file", "blob", name]), content);
    let oid = git2::Repository::open(&f.repo)
        .unwrap()
        .refname_to_id(name)
        .unwrap();
    assert!(
        f.backend().read_backup(&oid.to_string()).is_err(),
        "restore uses the ref, not bare OID"
    );
    assert!(f.backend().read_backup("refs/heads/main").is_err());
}

#[test]
fn remove_success_partial_and_unknown_keep_persisted_refs_after_gc() {
    for fault in [
        None,
        Some(RemoveFaultPoint::FailAfterBackupBeforeDelete),
        Some(RemoveFaultPoint::PanicAfterDeletionStarted),
    ] {
        let f = Fixture::new();
        std::fs::create_dir(f.repo.join(".kagi")).unwrap();
        std::fs::write(
            f.repo.join(".kagi/worktree.toml"),
            "[[pre_remove]]\ntype='copy'\nfrom='secret'\nto='copied'\n",
        )
        .unwrap();
        git(&f.repo, &["add", ".kagi"]);
        git(&f.repo, &["commit", "-qm", "pre remove copy"]);
        let linked = f.repo.parent().unwrap().join("linked");
        git(
            &f.repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        // Ignored source has never been in a commit, index or snapshot.
        let bytes = b"unique pre_remove bytes absent from all trees\n";
        std::fs::write(f.repo.join("secret"), bytes).unwrap();
        let plan = Backend::plan_recorded_remove(&f.repo, "linked", true).unwrap();
        let report = Backend::run_recorded_remove(&plan, Actor::Human, fault);
        let entry = read_oplog_tail(1).remove(0);
        match fault {
            None => {
                assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
                assert!(!linked.exists());
            }
            Some(RemoveFaultPoint::FailAfterBackupBeforeDelete) => {
                assert!(matches!(entry.outcome, OpOutcome::Partial { .. }))
            }
            _ => assert!(matches!(entry.outcome, OpOutcome::Unknown { .. })),
        }
        assert_eq!(entry.backup_refs.len(), 1, "{:?}", report.progress);
        assert_eq!(entry.backup_refs, report.recording.entry().backup_refs);
        gc(&f.repo);
        assert_eq!(
            f.backend().read_backup(&entry.backup_refs[0]).unwrap(),
            bytes
        );
    }
}

#[test]
fn retiring_one_entry_cleans_only_its_refs_and_gc_reclaims_its_bytes() {
    let f = Fixture::new();
    let first = f.discard(b"first disposable content\n");
    let second = f.discard(b"retained content\n");
    let raw = git2::Repository::open(&f.repo).unwrap();
    let oid = raw.refname_to_id(&first.backup_refs[0]).unwrap();
    let backend = f.backend();
    let plan = backend.plan_forget_oplog_entry(&first).unwrap();
    assert_eq!(
        plan.backup_refs().collect::<Vec<_>>(),
        vec![first.backup_refs[0].as_str()]
    );
    let report = backend.execute_forget_oplog_entry(&plan);
    report.result.unwrap();
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert!(read_oplog_tail(100)
        .iter()
        .all(|e| e.backup_refs != first.backup_refs));
    assert!(raw.find_reference(&first.backup_refs[0]).is_err());
    gc(&f.repo);
    assert!(git2::Repository::open(&f.repo)
        .unwrap()
        .find_blob(oid)
        .is_err());
    assert_eq!(
        backend.read_backup(&second.backup_refs[0]).unwrap(),
        b"retained content\n"
    );
    assert!(raw.find_reference("refs/heads/main").is_ok());
}

#[test]
fn shared_ref_survives_until_its_last_entry_is_retired() {
    let f = Fixture::new();
    let first = f.discard(b"shared recovery\n");
    let mut other = first.clone();
    other.op = "shared-recovery-receipt".into();
    append_oplog(&other).unwrap();
    // Valid JSON whitespace must not hide a surviving reference from cleanup.
    let log_path = f.log.join("operations.jsonl");
    let content = std::fs::read_to_string(&log_path).unwrap();
    std::fs::write(
        &log_path,
        content.replace("\"backup_refs\":[", "\"backup_refs\": ["),
    )
    .unwrap();
    let other = read_oplog_tail(1).remove(0);
    let backend = f.backend();
    let plan = backend.plan_forget_oplog_entry(&first).unwrap();
    assert_eq!(plan.backup_refs().count(), 0);
    backend.execute_forget_oplog_entry(&plan).result.unwrap();
    gc(&f.repo);
    assert_eq!(
        backend.read_backup(&first.backup_refs[0]).unwrap(),
        b"shared recovery\n"
    );
    let plan = backend.plan_forget_oplog_entry(&other).unwrap();
    backend.execute_forget_oplog_entry(&plan).result.unwrap();
    assert!(backend.read_backup(&first.backup_refs[0]).is_err());
}

#[test]
fn log_or_ref_drift_refuses_cleanup_and_untrusted_cleanup_is_recorded() {
    let f = Fixture::new();
    let first = f.discard(b"safe bytes\n");
    let mut backend = f.backend();
    let plan = backend.plan_forget_oplog_entry(&first).unwrap();
    f.discard(b"later bytes\n");
    assert!(backend.execute_forget_oplog_entry(&plan).result.is_err());
    assert_eq!(
        backend.read_backup(&first.backup_refs[0]).unwrap(),
        b"safe bytes\n"
    );
    let plan = backend.plan_forget_oplog_entry(&first).unwrap();
    let raw = git2::Repository::open(&f.repo).unwrap();
    let replacement = raw.blob(b"replacement").unwrap();
    raw.reference(&first.backup_refs[0], replacement, true, "fixture drift")
        .unwrap();
    assert!(backend.execute_forget_oplog_entry(&plan).result.is_err());
    assert_eq!(
        raw.refname_to_id(&first.backup_refs[0]).unwrap(),
        replacement
    );
    let plan = backend.plan_forget_oplog_entry(&first).unwrap();
    backend.set_trust_for_test(kagi_git::trust::RepoTrust::Untrusted);
    let report = backend.execute_forget_oplog_entry(&plan);
    assert!(report.result.is_err());
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Failed { .. }
    ));
    assert!(read_oplog_tail(100)
        .iter()
        .any(|e| e.backup_refs == first.backup_refs));
}

#[test]
fn append_failure_retains_attempted_receipt_ref_through_gc() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.log.join("operations.jsonl")).unwrap();
    std::fs::write(f.repo.join("tracked"), b"survive log failure\n").unwrap();
    let mut backend = f.backend();
    let op = Operation::Discard {
        paths: vec!["tracked".into()],
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    report.result.unwrap();
    let Recording::Failed { attempted, .. } = report.recording else {
        panic!("expected failure")
    };
    assert_eq!(attempted.backup_refs.len(), 1);
    gc(&f.repo);
    assert_eq!(
        backend.read_backup(&attempted.backup_refs[0]).unwrap(),
        b"survive log failure\n"
    );
}

#[test]
fn malformed_log_and_foreign_reference_fail_closed() {
    let f = Fixture::new();
    let first = f.discard(b"keep recovery\n");
    let mut forged = first.clone();
    forged.backup_refs = vec!["refs/heads/main".into()];
    append_oplog(&forged).unwrap();
    let forged = read_oplog_tail(1).remove(0);
    assert!(f.backend().plan_forget_oplog_entry(&forged).is_err());
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(f.log.join("operations.jsonl"))
        .unwrap()
        .write_all(b"unreadable entry\n")
        .unwrap();
    assert!(f.backend().plan_forget_oplog_entry(&first).is_err());
    gc(&f.repo);
    assert_eq!(
        f.backend().read_backup(&first.backup_refs[0]).unwrap(),
        b"keep recovery\n"
    );
}

#[test]
fn backup_ref_creation_failure_preserves_worktree_and_ref_namespace() {
    let f = Fixture::new();
    let raw = git2::Repository::open(&f.repo).unwrap();
    let head = raw.head().unwrap().target().unwrap();
    // A valid existing ref blocks the required directory, without overwriting it.
    raw.reference(
        "refs/kagi/backups",
        head,
        false,
        "fixture namespace collision",
    )
    .unwrap();
    std::fs::write(f.repo.join("tracked"), b"must not discard\n").unwrap();
    let mut backend = f.backend();
    let op = Operation::Discard {
        paths: vec!["tracked".into()],
    };
    let plan = backend.plan(&op).unwrap();
    let report = backend.run_recorded(&op, &plan);
    assert!(report.result.is_err());
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Failed { .. }
    ));
    assert_eq!(
        std::fs::read(f.repo.join("tracked")).unwrap(),
        b"must not discard\n"
    );
    assert_eq!(raw.refname_to_id("refs/kagi/backups").unwrap(), head);
}

#[test]
fn retiring_latest_entry_does_not_reuse_its_sequence_id() {
    let f = Fixture::new();
    let entry = f.discard(b"retire only entry\n");
    let backend = f.backend();
    let plan = backend.plan_forget_oplog_entry(&entry).unwrap();
    let report = backend.execute_forget_oplog_entry(&plan);
    report.result.unwrap();
    assert!(report.recording.entry().id > entry.id);
    let later = f.discard(b"next entry\n");
    assert!(later.id > report.recording.entry().id);
}

#[test]
fn busy_log_lock_preserves_entry_and_refs_without_blocking_cleanup() {
    let f = Fixture::new();
    let entry = f.discard(b"preserve while locked\n");
    let backend = f.backend();
    let plan = backend.plan_forget_oplog_entry(&entry).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.log.join("operations.jsonl.lock"))
        .unwrap();
    lock.lock().unwrap();
    let report = backend.execute_forget_oplog_entry(&plan);
    assert!(report.result.is_err());
    assert!(matches!(report.recording, Recording::Failed { .. }));
    assert_eq!(read_oplog_tail(100).len(), 1);
    assert_eq!(
        backend.read_backup(&entry.backup_refs[0]).unwrap(),
        b"preserve while locked\n"
    );
    drop(lock);
    backend.execute_forget_oplog_entry(&plan).result.unwrap();
    assert!(backend.read_backup(&entry.backup_refs[0]).is_err());
}
