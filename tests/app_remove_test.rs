//! Window-free vertical proof: production public approval/job/delivery API.
use kagi::app::*;
use kagi_domain::remove::RemoveFaultPoint as Fault;
use kagi_git::backend::remove::Recording;
use kagi_git::oplog::{read_oplog_tail, OpOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());
struct EnvVarRestore {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}
impl EnvVarRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}
impl Drop for EnvVarRestore {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}
struct Fixture {
    _guard: MutexGuard<'static, ()>,
    _dir: TempDir,
    repo: PathBuf,
    linked: PathBuf,
    log: PathBuf,
    old_log: Option<std::ffi::OsString>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.old_log {
            Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
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
}
impl Fixture {
    fn new(config: Option<&str>) -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("main");
        let linked = root.join("linked");
        let log = root.join("log");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("source"), b"recover these bytes\n").unwrap();
        if let Some(config) = config {
            std::fs::create_dir(repo.join(".kagi")).unwrap();
            std::fs::write(repo.join(".kagi/worktree.toml"), config).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", &log);
        Self {
            _guard: guard,
            _dir: dir,
            repo,
            linked,
            log,
            old_log,
        }
    }
    fn request(&self) -> RemoveRequest {
        RemoveRequest {
            owner: Attachment {
                path: self.repo.clone(),
                generation: 1,
            },
            name: "linked".into(),
            delete_branch: true,
        }
    }
    fn ready(&self, sessions: &mut Sessions) -> PlanToken {
        let job = plan_remove(sessions, self.request(), RemovePolicy::default());
        apply_plan(sessions, job.run());
        match sessions.plan_state() {
            PlanState::Ready { token, .. } => token.clone(),
            state => panic!("{state:?}"),
        }
    }
    fn job(&self, sessions: &mut Sessions) -> RemoveJob {
        let token = self.ready(sessions);
        let approved = approve(sessions, token, RemovePolicy::default()).unwrap();
        prepare_remove(sessions, approved, LegacyBusy(false)).unwrap()
    }
}
fn outcome(completion: &RemoveCompletion) -> &OpOutcome {
    &completion.report().recording.entry().outcome
}
const COPY: &str = "[[pre_remove]]\ntype = 'copy'\nfrom = 'source'\nto = 'copied'\n";

#[test]
fn normal_receipt_identity_and_invalidation() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let done = f.job(&mut s).run();
    assert!(matches!(outcome(&done), OpOutcome::Success { .. }));
    assert!(!f.linked.exists());
    assert_eq!(
        done.report().recording.entry().worktree.as_deref(),
        f.linked.to_str()
    );
    assert_eq!(read_oplog_tail(100).len(), 1);
    let deliveries = apply(&mut s, done);
    assert_eq!(deliveries.len(), 3);
    assert!(matches!(&deliveries[1], Delivery::RemovedTarget(path) if path == &f.linked));
    assert!(!s.has_leases());
    assert!(s.is_stale(&f.repo));
    assert!(s.is_stale(&f.linked));
    s.read_applied(&f.repo);
    assert!(!s.is_stale(&f.repo));
}
#[test]
fn revision_replan_failure_never_revives_ready() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let old = f.ready(&mut s);
    let late = plan_remove(&mut s, f.request(), RemovePolicy::default()).run();
    let mut bad = f.request();
    bad.owner.path = f.repo.join("missing");
    let fail = plan_remove(&mut s, bad, RemovePolicy::default()).run();
    apply_plan(&mut s, fail);
    apply_plan(&mut s, late);
    assert!(matches!(s.plan_state(), PlanState::Error { .. }));
    assert!(approve(&mut s, old, RemovePolicy::default()).is_err());
    assert!(f.linked.exists());
}
#[test]
fn refused_dirty_locked_main_missing() {
    let f = Fixture::new(None);
    std::fs::write(f.linked.join("dirty"), "keep").unwrap();
    for name in ["linked", "main", "missing"] {
        let mut s = Sessions::new();
        let mut request = f.request();
        request.name = name.into();
        let plan = plan_remove(&mut s, request, RemovePolicy::default()).run();
        apply_plan(&mut s, plan);
        let PlanState::Ready { token, .. } = s.plan_state() else {
            panic!()
        };
        let token = token.clone();
        let approved = approve(&mut s, token, RemovePolicy::default()).unwrap();
        let done = prepare_remove(&mut s, approved, LegacyBusy(false))
            .unwrap()
            .run();
        assert!(matches!(outcome(&done), OpOutcome::Refused { .. }));
    }
    assert!(f.linked.join("dirty").exists());
}
#[test]
fn preflight_drift_dirty_lock_config_head() {
    for change in ["dirty", "lock", "config", "head"] {
        let f = Fixture::new(None);
        let mut s = Sessions::new();
        let job = f.job(&mut s);
        match change {
            "dirty" => std::fs::write(f.linked.join("dirty"), "keep").unwrap(),
            "lock" => git(&f.repo, &["worktree", "lock", f.linked.to_str().unwrap()]),
            "config" => {
                std::fs::create_dir(f.linked.join(".kagi")).unwrap();
                std::fs::write(f.linked.join(".kagi/worktree.toml"), COPY).unwrap();
            }
            _ => git(
                &f.linked,
                &[
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "moved",
                ],
            ),
        }
        let completion = job.run();
        let OpOutcome::Refused { blockers } = outcome(&completion) else {
            panic!("runtime drift must be recorded as Refused");
        };
        assert!(!blockers.is_empty());
        if change == "dirty" {
            assert!(blockers.join("; ").contains("target changed after plan"));
        }
        assert!(f.linked.exists());
    }
}
#[test]
fn same_locator_aba_refused() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    git(&f.repo, &["worktree", "remove", f.linked.to_str().unwrap()]);
    git(
        &f.repo,
        &["worktree", "add", f.linked.to_str().unwrap(), "linked"],
    );
    assert!(matches!(outcome(&job.run()), OpOutcome::Refused { .. }));
    assert!(f.linked.exists());
}
#[test]
fn open_failure_still_records() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    let original = f.repo.join(".git");
    let moved = f.repo.join("hidden-git");
    std::fs::rename(&original, &moved).unwrap();
    let done = job.run();
    std::fs::rename(moved, original).unwrap();
    assert!(matches!(outcome(&done), OpOutcome::Failed { .. }));
    assert_eq!(read_oplog_tail(10).len(), 1);
}
#[test]
fn duplicate_and_both_legacy_admission_directions() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let token = f.ready(&mut s);
    let approved = approve(&mut s, token.clone(), RemovePolicy::default()).unwrap();
    assert!(approve(&mut s, token, RemovePolicy::default()).is_err());
    assert!(matches!(
        prepare_remove(&mut s, approved, LegacyBusy(true)),
        Err(AdmissionError::Busy)
    ));
    let job = f.job(&mut s);
    assert!(s.has_leases());
    let token = f.ready(&mut s);
    let second = approve(&mut s, token, RemovePolicy::default()).unwrap();
    assert!(matches!(
        prepare_remove(&mut s, second, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    let done = job.run();
    assert!(!apply(&mut s, done.clone()).is_empty());
    assert!(apply(&mut s, done).is_empty());
    assert_eq!(read_oplog_tail(10).len(), 1);
}
#[test]
fn tab_switch_close_welcome_background_close_never_discards_completion() {
    for _event in [
        "switch",
        "owner close",
        "last tab Welcome",
        "background close",
        "reselect",
    ] {
        let f = Fixture::new(None);
        let mut s = Sessions::new();
        let job = f.job(&mut s);
        s.invalidate_plan(); // adapter detach invalidates approval, not execution
        let deliveries = apply(&mut s, job.run());
        assert!(!s.has_leases());
        assert!(
            matches!(&deliveries[2], Delivery::Completed { attachment, .. } if attachment.path == f.repo && attachment.generation == 1)
        );
        assert_eq!(read_oplog_tail(10).len(), 1);
    }
}
#[test]
fn window_close_may_close_host_predicate_tracks_remove_completion() {
    may_close_host_predicate_tracks_remove_completion();
}
#[test]
fn abandoned_job_records_and_releases_after_poll() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    drop(f.job(&mut s));
    assert_eq!(s.drain_abandoned().len(), 3);
    assert!(!s.has_leases());
    assert!(f.linked.exists());
    let entries = read_oplog_tail(10);
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].outcome, OpOutcome::Failed { .. }));
    assert_eq!(entries[0].worktree.as_deref(), f.linked.to_str());
}
#[test]
fn policy_and_session_mismatch_refuse_approval() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let token = f.ready(&mut s);
    let mut other = Sessions::new();
    let _ = f.ready(&mut other);
    assert!(approve(&mut other, token.clone(), RemovePolicy::default()).is_err());
    assert!(approve(
        &mut s,
        token,
        RemovePolicy {
            actor: kagi_git::Actor::Mcp
        }
    )
    .is_err());
    assert!(f.linked.exists());
}
#[test]
fn reconcile_failed_read_and_replayed_ack_do_not_unlock() {
    let f = Fixture::new(Some(COPY));
    let mut s = Sessions::new();
    let job = f
        .job(&mut s)
        .with_fault_for_test(Fault::PanicAfterDeletionStarted);
    let id = job.id();
    apply(&mut s, job.run());
    let original = f.repo.join(".git");
    let moved = f.repo.join("hidden-git");
    std::fs::rename(&original, &moved).unwrap();
    assert!(read_reconcile(&s, id).is_err());
    std::fs::rename(moved, original).unwrap();
    let read = read_reconcile(&s, id).unwrap();
    let replay = read.clone();
    acknowledge(&mut s, read).unwrap();
    assert!(acknowledge(&mut s, replay).is_err());
}
#[test]
fn quit_may_close_host_predicate_tracks_remove_completion() {
    may_close_host_predicate_tracks_remove_completion();
}

fn may_close_host_predicate_tracks_remove_completion() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    assert!(!s.may_close_host());
    apply(&mut s, job.run());
    assert!(s.may_close_host());
}
#[test]
fn owner_trust_refusal_at_each_fresh_open_is_recorded_without_mutation() {
    for fault in [Fault::UntrustedMain, Fault::UntrustedTarget] {
        let f = Fixture::new(None);
        let repo = git2::Repository::open(&f.repo).unwrap();
        let branch_before = repo
            .find_branch("linked", git2::BranchType::Local)
            .unwrap()
            .get()
            .target()
            .unwrap();
        let source_before = std::fs::read(f.linked.join("source")).unwrap();
        let mut s = Sessions::new();
        let done = f.job(&mut s).with_fault_for_test(fault).run();
        let OpOutcome::Refused { blockers } = outcome(&done) else {
            panic!("fresh-open owner trust must refuse");
        };
        assert!(blockers.join("; ").contains("not trusted"));
        assert!(
            f.linked.exists(),
            "untrusted repo must not mutate the worktree"
        );
        assert_eq!(
            std::fs::read(f.linked.join("source")).unwrap(),
            source_before
        );
        let repo = git2::Repository::open(&f.repo).unwrap();
        assert_eq!(
            repo.find_branch("linked", git2::BranchType::Local)
                .unwrap()
                .get()
                .target(),
            Some(branch_before)
        );
        assert!(repo.find_worktree("linked").is_ok());
        assert_eq!(read_oplog_tail(10).len(), 1);
    }
}
#[test]
fn initial_pre_remove_policy_refusal_is_failed_without_started_evidence() {
    let f = Fixture::new(Some("[[pre_remove]]\ntype = 'command'\nrun = 'true'\n"));
    let cfg = kagi_git::ops::load_worktree_config(&f.linked)
        .unwrap()
        .unwrap();
    kagi_git::ops::trust_worktree_config(&cfg).unwrap();
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    let _headless = EnvVarRestore::set("KAGI_OPEN_REPO", "headless-test");
    let done = job.run();
    assert!(matches!(outcome(&done), OpOutcome::Failed { .. }));
    assert!(f.linked.exists());
    assert_eq!(read_oplog_tail(10).len(), 1);
    assert!(done
        .report()
        .progress
        .observations
        .iter()
        .all(|observation| !observation.contains("step 0 started")));
}
#[test]
fn policy_refusal_after_trust_grant_is_partial() {
    let f = Fixture::new(Some("[[pre_remove]]\ntype = 'command'\nrun = 'true'\n"));
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    let _headless = EnvVarRestore::set("KAGI_OPEN_REPO", "headless-test");
    let done = job.run();
    assert!(matches!(outcome(&done), OpOutcome::Partial { .. }));
    assert!(done.report().progress.config_granted);
    assert!(f.linked.exists());
}
#[test]
fn policy_refusal_after_copy_is_partial() {
    let f = Fixture::new(Some(
        "[[pre_remove]]\ntype = 'copy'\nfrom = 'source'\nto = 'copied'\n\
         [[pre_remove]]\ntype = 'command'\nrun = 'true'\n",
    ));
    let cfg = kagi_git::ops::load_worktree_config(&f.linked)
        .unwrap()
        .unwrap();
    kagi_git::ops::trust_worktree_config(&cfg).unwrap();
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    let _headless = EnvVarRestore::set("KAGI_OPEN_REPO", "headless-test");
    let done = job.run();
    assert!(matches!(outcome(&done), OpOutcome::Partial { .. }));
    assert!(!done.report().progress.config_granted);
    assert_eq!(
        std::fs::read(f.linked.join("copied")).unwrap(),
        b"recover these bytes\n"
    );
    assert!(f.linked.exists());
}
#[test]
fn branch_moved_after_admin_prune_is_kept_and_receipt_is_partial() {
    let f = Fixture::new(None);
    let repo = git2::Repository::open(&f.repo).unwrap();
    let captured = repo
        .find_branch("linked", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap()
        .to_string();
    let mut s = Sessions::new();
    let done = f
        .job(&mut s)
        .with_fault_for_test(Fault::MoveBranchBeforeDelete)
        .run();
    let OpOutcome::Partial { after, error } = outcome(&done) else {
        panic!("a moved branch must be recorded as Partial");
    };
    let repo = git2::Repository::open(&f.repo).unwrap();
    let advanced = repo
        .find_branch("linked", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap()
        .to_string();
    assert_ne!(advanced, captured);
    assert!(error.contains(&format!("branch moved {captured}→{advanced}, kept")));
    assert!(after.dirty.contains(&format!("branch_tip={captured}")));
    assert!(!f.linked.exists());
    assert_eq!(read_oplog_tail(10).len(), 1);
}
#[test]
fn partial_and_executor_panic_jsonl_preserve_bytes() {
    for fault in [
        Fault::FailAfterBackupBeforeDelete,
        Fault::FailAfterDirectoryDelete,
        Fault::PanicAfterDeletionStarted,
    ] {
        let f = Fixture::new(Some(COPY));
        let mut s = Sessions::new();
        let job = f.job(&mut s).with_fault_for_test(fault);
        let id = job.id();
        let done = job.run();
        let entries = read_oplog_tail(10);
        assert_eq!(entries.len(), 1);
        let after = match &entries[0].outcome {
            OpOutcome::Partial { after, .. } | OpOutcome::Unknown { after, .. } => after,
            other => panic!("{other:?}"),
        };
        let backup = &done.report().progress.backups[0];
        assert!(after.dirty.contains(&backup.blob));
        let oid = after
            .dirty
            .split("copied=")
            .nth(1)
            .unwrap()
            .split([',', ';'])
            .next()
            .unwrap();
        let repo = git2::Repository::open(&f.repo).unwrap();
        assert_eq!(
            repo.find_blob(git2::Oid::from_str(oid).unwrap())
                .unwrap()
                .content(),
            b"recover these bytes\n"
        );
        assert!(after.dirty.contains(
            done.report()
                .progress
                .branch_tip
                .as_ref()
                .unwrap()
                .0
                .as_str()
        ));
        apply(&mut s, done);
        if fault == Fault::PanicAfterDeletionStarted {
            let read = read_reconcile(&s, id).unwrap();
            acknowledge(&mut s, read).unwrap();
        }
    }
}
#[test]
fn pre_remove_partial_and_termination_unknown() {
    let config = format!("{COPY}\n[[pre_remove]]\ntype='copy'\nfrom='missing'\nto='bad'\n");
    let f = Fixture::new(Some(&config));
    let mut s = Sessions::new();
    let done = f.job(&mut s).run();
    assert!(matches!(outcome(&done), OpOutcome::Partial { .. }));
    assert!(f.linked.join("copied").exists());
    assert!(f.linked.exists());
    apply(&mut s, done);
    // Separate fixture below: no mutation replay of the partial command.
}
#[test]
fn unknown_stop_keeps_lease_even_after_read_ack_attempt() {
    let f = Fixture::new(Some(COPY));
    let mut s = Sessions::new();
    let job = f
        .job(&mut s)
        .with_fault_for_test(Fault::PreRemoveTerminationUnknown);
    let id = job.id();
    let done = job.run();
    assert!(matches!(outcome(&done), OpOutcome::Unknown { .. }));
    apply(&mut s, done);
    assert!(s.has_leases());
    assert!(!s.may_close_host());
    assert!(read_reconcile(&s, id).is_err());
}
#[test]
fn before_mutation_panic_is_failed_once() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let done = f
        .job(&mut s)
        .with_fault_for_test(Fault::PanicBeforeMutation)
        .run();
    assert!(matches!(outcome(&done), OpOutcome::Failed { .. }));
    assert!(f.linked.exists());
    apply(&mut s, done);
    assert!(!s.has_leases());
    assert_eq!(read_oplog_tail(10).len(), 1);
}
#[test]
fn append_failure_does_not_reexecute_or_return_old_tail() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let job = f.job(&mut s);
    std::fs::create_dir_all(f.log.join("operations.jsonl")).unwrap();
    let done = job.run();
    assert!(matches!(done.report().recording, Recording::Failed { .. }));
    assert!(matches!(outcome(&done), OpOutcome::Success { .. }));
    assert!(!f.linked.exists());
    apply(&mut s, done);
    assert!(!s.has_leases());
}
#[test]
fn receipt_is_exact_entry_despite_later_append_and_unknown_roundtrip() {
    let f = Fixture::new(None);
    let mut s = Sessions::new();
    let done = f.job(&mut s).run();
    let original = done.report().recording.entry();
    let mut other = original.clone();
    other.op = "other".into();
    kagi_git::oplog::append_oplog(&other).unwrap();
    assert_ne!(original.id, read_oplog_tail(1)[0].id);
    other.outcome = OpOutcome::Unknown {
        after: original.before.clone(),
        evidence: "stage=PreRemove; \"after\": tricky\ntext".into(),
    };
    let (_, assigned) = kagi_git::oplog::append_oplog_receipt(&other).unwrap();
    assert_eq!(
        kagi_git::oplog::entry_to_json(&assigned),
        kagi_git::oplog::entry_to_json(&read_oplog_tail(1)[0])
    );
}
