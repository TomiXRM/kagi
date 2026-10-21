//! ADR-0196 Wave 3: the generic run-pipeline family through the public
//! application API — admission, job, settlement, delivery. No window.
use kagi::app::*;
use kagi_git::oplog::{read_oplog_tail, OpOutcome};
use kagi_git::{Backend, GitError, Operation, StateSummary, Termination};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::Mutex;

#[path = "support/git_fixture.rs"]
mod git_fixture;
use git_fixture::git_output as git;

#[path = "support/app_fixture.rs"]
mod app_fixture;
use app_fixture::AppFixture as Fixture;

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
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let owner = request.owner.clone();
    let plan = request.plan.clone();
    let approved = approve_run(&mut s, request).expect("owner attached and identical");
    let job = prepare_run(&mut s, approved, checkout_side(f.repo.clone(), plan)).expect("admitted");
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
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
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
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    s.detach(request.owner.session);
    assert_eq!(
        approve_run(&mut s, request).err(),
        Some(AdmissionError::StaleApproval)
    );
}

#[test]
fn a_second_run_is_refused_while_the_first_holds_the_lease() {
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let first = f.checkout_request(&mut s);
    let second = first.clone();
    let approved = approve_run(&mut s, first).unwrap();
    let job = prepare_run(&mut s, approved, Box::new(|| Err("never run".to_string()))).unwrap();
    assert_eq!(
        approve_run(&mut s, second.clone()).err(),
        Some(AdmissionError::Busy),
        "the second run is refused before it can replace any existing plan"
    );
    apply(&mut s, job.run());
    let approved = approve_run(&mut s, second).expect("Busy is retry-later");
    let retry = prepare_run(&mut s, approved, Box::new(|| Err("never run".to_string())))
        .expect("the settled writer releases admission");
    apply(&mut s, retry.run());
}

/// A Busy modal admission is retry-later, not a replacement of another
/// family's ready plan. Once the first writer settles without changing Git,
/// the original token is still approvable and is consumed only by begin_write.
#[test]
fn busy_run_admission_preserves_another_ready_plan_and_its_one_shot_token() {
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let first = approve_run(&mut s, request.clone()).unwrap();
    let job = prepare_run(
        &mut s,
        first,
        Box::new(|| Err("repository did not open".to_string())),
    )
    .unwrap();

    std::fs::write(f.repo.join("a.txt"), "changed\n").unwrap();
    let plan = plan_stash(
        &mut s,
        StashRequest {
            owner: request.owner.clone(),
            action: StashAction::Push {
                message: Some("keep this plan".into()),
                include_untracked: false,
            },
        },
        StashPolicy::default(),
    );
    assert!(apply_plan(&mut s, plan.run()));
    let PlanState::Ready { token, .. } = s.plan_state() else {
        panic!("expected a ready stash plan, got {:?}", s.plan_state());
    };
    let token = token.clone();

    assert_eq!(
        approve_run(&mut s, request).err(),
        Some(AdmissionError::Busy),
        "a concurrent run must refuse before replacing the ready plan"
    );
    assert!(
        matches!(s.plan_state(), PlanState::Ready { .. }),
        "Busy must leave the ready plan on screen"
    );

    apply(&mut s, job.run());
    let approved = approve(&mut s, token, StashPolicy::default())
        .expect("the original plan revision remains current after Busy");
    begin_write(&mut s, &approved).expect("one write may spend the approval");
    assert_eq!(
        begin_write(&mut s, &approved).err(),
        Some(AdmissionError::StaleApproval),
        "begin_write still spends the approval exactly once"
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
#[cfg(unix)]
#[test]
fn an_unconfirmed_run_is_reconciled_once_its_child_is_proven_gone() {
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let plan = request.plan.clone();
    let repo = f.repo.clone();
    // A group the kill could not account for: only probing it proves it later.
    let pid = dead_group();
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        Box::new(move || {
            let evidence = "git fetch timed out after 60s".to_string();
            Ok(kagi_git::backend::recording::RunReport {
                result: Err(GitError::TerminationUnknown(Termination::Unaccounted {
                    reason: evidence.clone(),
                    group: pid,
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

#[cfg(unix)]
/// A process group with nothing left in it: spawn a child in its own group,
/// wait for it, and reuse the id. `kill(-pgid, 0)` then answers ESRCH.
fn dead_group() -> u32 {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("true");
    cmd.process_group(0);
    let mut child = cmd.spawn().expect("spawn /usr/bin/true");
    let pid = child.id();
    child.wait().expect("reap it");
    pid
}

/// #703: a run job whose **task unwinds** while a process group it spawned is
/// still running.
///
/// Before the supervisor, the child handle lived on the panicking task's stack,
/// so the unwind took it: the termination had nothing to probe, the lease
/// stayed reserved and `may_close_host()` refused Quit until the application
/// restarted. The group is now owned outside the task, so the abandonment can
/// name it — and the acknowledge is still refused until that group is *proven*
/// empty, which is the half that keeps this from being "release on a panic".
#[cfg(unix)]
#[test]
fn a_panicked_run_is_reconciled_through_the_supervisor_once_its_group_is_gone() {
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let approved = approve_run(&mut s, request).unwrap();
    // What the job spawned, read back after the unwind so the test can watch it.
    let spawned: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let recorder = Arc::clone(&spawned);
    // How the test stops the descendant again. A file it polls for, rather than
    // a signal: `kill` with a negative pid is not portable between the BSD and
    // GNU tools (the previous revision passed on macOS and failed on Linux),
    // and the group must go away on its own for the probe to mean anything.
    let stop = f.repo.join("stop-the-descendant");
    let watch = stop.clone();
    let job = prepare_run(
        &mut s,
        approved,
        Box::new(move || {
            // A descendant that outlives the direct child and keeps its pipes
            // open: the run cannot prove that group empty, so the supervisor
            // goes on owning it. Then the task unwinds with it still running.
            let mut cmd = Command::new("sh");
            cmd.args([
                "-c",
                &format!(
                    "until [ -f '{}' ]; do sleep 0.05; done & exit 0",
                    watch.display()
                ),
            ]);
            let run = kagi_git::proc::run_child(&mut cmd, std::time::Duration::from_secs(5), None)
                .expect("spawn");
            *recorder.lock().unwrap() = Some(run.pid);
            assert!(
                !run.group_stopped,
                "precondition: the descendant must still hold the group"
            );
            panic!("the checkout task unwound mid-write");
        }),
    )
    .unwrap();
    let id = job.id();
    let abandonment = job.abandonment();

    let hushed = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())).is_err();
    std::panic::set_hook(hushed);
    assert!(panicked, "the job must actually unwind");
    let group = spawned.lock().unwrap().expect("the job spawned a group");
    assert!(
        kagi_git::proc::group_alive(group),
        "precondition: the abandoned job's group is still running"
    );

    apply(&mut s, abandonment.into_completion());
    assert!(
        s.has_leases(),
        "a panic proves nothing: the write may still be running (ADR-0175)"
    );

    // Still running → there is something to probe, and the probe says no.
    let read = read_reconcile(&s, id).expect("the supervisor left a group to probe");
    assert!(
        !read.stop_proven(),
        "the abandoned job's group is still alive: nothing is proven"
    );
    assert!(
        acknowledge(&mut s, read).is_err(),
        "acknowledging an unproven stop would release the scope on no evidence"
    );
    assert!(s.has_leases(), "and so the lease is still held");

    // The descendant goes; the same probe now proves the stop.
    std::fs::write(&stop, "").expect("write the stop file");
    for _ in 0..200 {
        if !kagi_git::proc::group_alive(group) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        !kagi_git::proc::group_alive(group),
        "the test could not stop the group it started"
    );

    let read = read_reconcile(&s, id).expect("still readable");
    assert!(
        read.stop_proven(),
        "the group is empty: the writer is proven stopped"
    );
    acknowledge(&mut s, read).expect("a proven stop releases the scope");
    assert!(!s.has_leases(), "the lease goes with it");
}

/// The other half of the same rule: a panicked job that spawned **nothing** has
/// no writer left either — the executor thread was it, and it unwound. That is
/// a stop proof, not an absence of one, so the entry is acknowledgeable at once
/// instead of holding the scope for the life of the process (#703).
#[test]
fn a_panicked_run_that_spawned_nothing_is_acknowledgeable() {
    let f = Fixture::checkout();
    let mut s = Sessions::new();
    let request = f.checkout_request(&mut s);
    let approved = approve_run(&mut s, request).unwrap();
    let job = prepare_run(
        &mut s,
        approved,
        Box::new(|| panic!("unwound before spawning")),
    )
    .unwrap();
    let id = job.id();
    let abandonment = job.abandonment();

    let hushed = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())).is_err();
    std::panic::set_hook(hushed);
    assert!(panicked, "the job must actually unwind");

    apply(&mut s, abandonment.into_completion());
    let read = read_reconcile(&s, id).expect("a stopped writer is readable");
    assert!(read.stop_proven(), "nothing of that job is running");
    acknowledge(&mut s, read).expect("so the scope can be let go");
    assert!(!s.has_leases());
}

#[path = "support/oplog.rs"]
mod oplog;

#[test]
fn receipt_filter_keeps_caller_limit_order_and_repository_scope() {
    use kagi_git::oplog::{append_oplog, OpLogEntry};

    let fixture = Fixture::checkout();
    for (repo, op) in [
        (&fixture.repo, "wanted"),
        (&fixture.repo, "unrelated"),
        (&fixture.repo.join("other"), "wanted"),
        (&fixture.repo, "wanted"),
    ] {
        append_oplog(&OpLogEntry::new(
            op,
            repo.display().to_string(),
            StateSummary {
                head: "main".into(),
                dirty: "clean".into(),
            },
            OpOutcome::Refused {
                blockers: vec!["fixture receipt".into()],
            },
        ))
        .unwrap();
    }

    let newest = oplog::records(&fixture.repo, "wanted", 100, true);
    assert_eq!(newest.len(), 2);
    let oldest = oplog::records(&fixture.repo, "wanted", 100, false);
    assert_eq!(oldest[0].id, newest[1].id);
    assert_eq!(oldest[1].id, newest[0].id);
    assert_eq!(
        oplog::records(&fixture.repo, "wanted", 1, true)[0].id,
        newest[0].id
    );
    assert_eq!(oplog::records(&fixture.repo, "wanted", 2, true).len(), 1);
    assert!(oplog::records(&fixture.repo, "wanted", 0, true).is_empty());
}
