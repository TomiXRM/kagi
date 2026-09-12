//! ADR-0196 Wave 3: the generic run-pipeline family through the public
//! application API — admission, job, settlement, delivery. No window.
use kagi::app::*;
use kagi_git::oplog::{read_oplog_tail, OpOutcome};
use kagi_git::{Backend, GitError, Operation, StateSummary, Termination};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    repo: PathBuf,
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
fn git(repo: &Path, args: &[&str]) -> String {
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
    assert!(output.status.success(), "git {args:?}: {:?}", output.stderr);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
impl Fixture {
    fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["branch", "side"]);
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", root.join("log"));
        Self {
            _guard: guard,
            _dir: dir,
            repo,
            old_log,
        }
    }
    /// A checkout-to-`side` request owned by the one attached session.
    fn request(&self, s: &mut Sessions) -> RunRequest {
        let session = s.attach(self.repo.clone());
        let owner = s.attachment(session).expect("attached");
        let backend = Backend::open(&self.repo).unwrap();
        let plan = backend
            .plan(&Operation::Checkout {
                branch: "side".into(),
            })
            .expect("plan checkout");
        RunRequest {
            owner,
            name: "checkout",
            path: self.repo.clone(),
            repo: backend.write_repo_id().unwrap(),
            plan: Arc::new(plan),
        }
    }
}

fn checkout_side(repo: PathBuf, plan: Arc<kagi_git::OperationPlan>) -> RunExecute {
    Box::new(move || {
        let mut backend = Backend::open(&repo).map_err(|e| e.to_string())?;
        Ok(backend.run_recorded(
            &Operation::Checkout {
                branch: "side".into(),
            },
            &plan,
        ))
    })
}

fn completed(deliveries: &[Delivery]) -> (OwnerStamp, kagi_git::backend::recording::RunReport) {
    deliveries
        .iter()
        .find_map(|d| match d {
            Delivery::Completed { stamp, report, .. } => match &report.evidence {
                FamilyEvidence::Run(run) => Some((*stamp, run.clone())),
                _ => None,
            },
            _ => None,
        })
        .expect("a run completion is delivered to its owner")
}

/// Admission reserves the lease and mints the stamp; the job's receipt is what
/// arrives, under that stamp; settlement releases the lease and invalidates
/// the owner's worktree.
#[test]
fn run_write_is_admitted_settled_and_delivered_to_its_owner() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let owner = request.owner.clone();
    let plan = request.plan.clone();
    let approved = approve_run(&mut s, request).expect("owner attached and identical");
    let job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        checkout_side(f.repo.clone(), plan),
    )
    .expect("admitted");
    assert!(s.has_leases(), "admission reserves the scope");
    let stamp = job.stamp();
    assert_eq!(stamp.session, owner.session);
    assert_eq!(stamp.visit, owner.visit);

    let deliveries = apply(&mut s, job.run());
    assert!(!s.has_leases(), "a stopped write releases its lease");
    assert_eq!(git(&f.repo, &["branch", "--show-current"]), "side");
    let (delivered, report) = completed(&deliveries);
    assert_eq!(delivered, stamp, "routed by the stamp minted at admission");
    assert!(report.result.is_ok());
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert!(
        deliveries.iter().any(|d| matches!(
            d,
            Delivery::Invalidate(target) if Some(&target.worktree) == owner.worktree.as_ref()
        )),
        "the owner's worktree is invalidated"
    );
    let tail = read_oplog_tail(100);
    assert_eq!(tail.len(), 1, "one durable receipt, written by the backend");
    assert_eq!(tail[0].op, "checkout");
}

/// The repository not opening is still a completion: the job records the
/// failure itself so the lease is released and the owner sees a receipt.
#[test]
fn open_failure_still_yields_a_receipt_and_releases_the_lease() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        Box::new(|| Err("repository moved".to_string())),
    )
    .unwrap();
    let deliveries = apply(&mut s, job.run());
    assert!(!s.has_leases());
    let (_, report) = completed(&deliveries);
    assert!(report.result.is_err());
    let entry = report.recording.entry();
    assert_eq!(entry.op, "checkout");
    assert!(matches!(&entry.outcome, OpOutcome::Failed { error } if error == "repository moved"));
    assert_eq!(read_oplog_tail(100).len(), 1, "the failure is durable");
    assert_eq!(git(&f.repo, &["branch", "--show-current"]), "main");
}

#[test]
fn approve_run_refuses_a_detached_owner() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    s.detach(request.owner.session);
    assert_eq!(
        approve_run(&mut s, request).err(),
        Some(AdmissionError::StaleApproval)
    );
}

#[test]
fn a_second_run_is_refused_while_the_first_holds_the_lease() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let first = f.request(&mut s);
    let second = first.clone();
    let approved = approve_run(&mut s, first).unwrap();
    let _job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        Box::new(|| Err("never run".to_string())),
    )
    .unwrap();
    let approved = approve_run(&mut s, second).unwrap();
    assert_eq!(
        prepare_run(
            &mut s,
            approved,
            LegacyBusy(false),
            Box::new(|| Err("never run".to_string()))
        )
        .err(),
        Some(AdmissionError::Busy)
    );
}

/// A run-family write whose termination is unconfirmed: the lease is retained,
/// a reconcile requirement is registered, and — once the child is proven gone —
/// the read and the acknowledge release the scope.
///
/// The whole lifecycle, because the previous revision registered the entry and
/// then had no exit: `prepare_reconcile` refused every `!stopped` local plan
/// forever, so the scope stayed closed for the life of the process
/// (#702 review P1). Fault injection is unavailable at this level, so the job
/// closure stands in for the executor; what it returns is what `run_git` builds.
#[test]
fn an_unconfirmed_run_is_reconciled_once_its_child_is_proven_gone() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let request = f.request(&mut s);
    let plan = request.plan.clone();
    let repo = f.repo.clone();
    // A child the kill could not account for: only its pid can prove it later.
    let pid = dead_pid();
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        LegacyBusy(false),
        Box::new(move || {
            let evidence = "git fetch timed out after 60s".to_string();
            Ok(kagi_git::backend::recording::RunReport {
                result: Err(GitError::TerminationUnknown(Termination {
                    reason: evidence.clone(),
                    child_stopped: false,
                    pid: Some(pid),
                })),
                recording: kagi_git::backend::recording::finalize(
                    kagi_git::oplog::OpLogEntry::new(
                        "checkout",
                        repo.display().to_string(),
                        plan.current.clone(),
                        OpOutcome::Unknown {
                            after: StateSummary {
                                head: plan.current.head.clone(),
                                dirty: "unknown".to_string(),
                            },
                            evidence,
                        },
                    ),
                ),
                stash: None,
            })
        }),
    )
    .unwrap();
    let id = job.id();

    apply(&mut s, job.run());
    assert!(
        s.has_leases(),
        "an unconfirmed writer keeps the scope reserved (ADR-0175)"
    );

    let read = read_reconcile(&s, id).expect("a pid is something a read can check");
    assert!(
        read.stop_proven(),
        "nothing answers to that pid any more: the writer is proven stopped"
    );
    acknowledge(&mut s, read).expect("a proven stop releases the scope");
    assert!(!s.has_leases(), "and the lease is gone with it");
    assert_eq!(
        prepare_reconcile(&s, id).err().as_deref(),
        Some("no reconcile request"),
        "the requirement is cleared, not merely satisfied"
    );
}

/// A pid that has really exited: spawn a child, wait for it, reuse its id.
fn dead_pid() -> u32 {
    let mut child = Command::new("true").spawn().expect("spawn /usr/bin/true");
    let pid = child.id();
    child.wait().expect("reap it");
    pid
}
