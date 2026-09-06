//! Window-free production approval/lease/recording contract for local stash.
use kagi::app::*;
use kagi_git::backend::stash::{StashEvent, StashStopReason};
use kagi_git::oplog::read_oplog_tail;
use kagi_git::{Backend, OpOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
static ENV: Mutex<()> = Mutex::new(());
struct Fixture {
    _lock: MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    repo: PathBuf,
    log: PathBuf,
    old: Option<std::ffi::OsString>,
}
fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
impl Fixture {
    fn new() -> Self {
        let lock = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("file"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        for text in ["one\n", "two\n", "three\n"] {
            std::fs::write(repo.join("file"), text).unwrap();
            git(&repo, &["stash", "push", "-qm", text.trim()]);
        }
        let log = dir.path().join("log");
        let old = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", &log);
        Self {
            _lock: lock,
            _dir: dir,
            repo,
            log,
            old,
        }
    }
    fn request(&self, s: &Sessions, owner: SessionId, action: StashAction) -> StashRequest {
        StashRequest {
            owner: s.attachment(owner).expect("owner is attached"),
            action,
        }
    }
    fn ready(&self, s: &mut Sessions, owner: SessionId, action: StashAction) -> PlanToken {
        let request = self.request(s, owner, action);
        let plan = plan_stash(s, request, StashPolicy::default());
        assert!(apply_plan(s, plan.run()));
        let PlanState::Ready { token, .. } = s.plan_state() else {
            panic!("{:?}", s.plan_state())
        };
        token.clone()
    }
    fn job(&self, s: &mut Sessions, owner: SessionId, action: StashAction) -> StashJob {
        let token = self.ready(s, owner, action);
        let approved = approve(s, token, StashPolicy::default()).unwrap();
        prepare_stash(s, approved, LegacyBusy(false)).unwrap()
    }
    fn ids(&self) -> Vec<String> {
        git(&self.repo, &["stash", "list", "--format=%H"])
            .lines()
            .map(str::to_owned)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        match &self.old {
            Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
            None => std::env::remove_var("KAGI_LOG_DIR"),
        }
    }
}
fn outcome(c: &StashCompletion) -> &OpOutcome {
    &c.report().recording.entry().outcome
}

#[test]
fn four_operations_on_three_stashes() {
    for action in [
        StashAction::Apply { index: 1 },
        StashAction::Pop { index: 1 },
        StashAction::Drop { index: 1 },
        StashAction::Push {
            message: Some("four".into()),
            include_untracked: true,
        },
    ] {
        let f = Fixture::new();
        let before = f.ids();
        let index_before = git(&f.repo, &["ls-files", "--stage"]);
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        if matches!(action, StashAction::Push { .. }) {
            std::fs::write(f.repo.join("new"), "four\n").unwrap();
        }
        let c = f.job(&mut s, owner, action.clone()).run();
        assert!(
            matches!(outcome(&c), OpOutcome::Success { .. }),
            "{:?}",
            outcome(&c)
        );
        assert_eq!(read_oplog_tail(100).len(), 1);
        assert_eq!(git(&f.repo, &["ls-files", "--stage"]), index_before);
        match action {
            StashAction::Apply { .. } => {
                assert_eq!(f.ids(), before);
                assert_eq!(
                    std::fs::read_to_string(f.repo.join("file")).unwrap(),
                    "two\n"
                );
            }
            StashAction::Pop { .. } => {
                assert_eq!(f.ids(), vec![before[0].clone(), before[2].clone()]);
                assert_eq!(
                    std::fs::read_to_string(f.repo.join("file")).unwrap(),
                    "two\n"
                );
            }
            StashAction::Drop { .. } => {
                assert_eq!(f.ids(), vec![before[0].clone(), before[2].clone()]);
                assert_eq!(
                    std::fs::read_to_string(f.repo.join("file")).unwrap(),
                    "base\n"
                );
                assert!(format!("{:?}", outcome(&c)).contains(&before[1]));
            }
            StashAction::Push { .. } => {
                assert_eq!(f.ids().len(), 4);
                assert!(!f.repo.join("new").exists());
            }
        }
        let duplicate = c.clone();
        assert_eq!(s.apply(c).len(), 2);
        assert!(s.apply(duplicate).is_empty());
        assert!(!s.has_leases());
        assert!(s.is_stale(s.worktree_of(owner).unwrap()));
    }
}
#[test]
fn same_count_replacement_refuses_wrong_entry() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let job = f.job(&mut s, owner, StashAction::Drop { index: 1 });
    git(&f.repo, &["stash", "drop", "stash@{0}"]);
    std::fs::write(f.repo.join("file"), "replacement\n").unwrap();
    git(&f.repo, &["stash", "push", "-qm", "replacement"]);
    let before = f.ids();
    let c = job.run();
    assert!(matches!(outcome(&c), OpOutcome::Refused { .. }));
    assert_eq!(before, f.ids());
    assert_eq!(read_oplog_tail(100).len(), 1);
}
#[test]
fn pop_second_step_drift_is_partial_not_refused() {
    let f = Fixture::new();
    let before = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let c = f
        .job(&mut s, owner, StashAction::Pop { index: 1 })
        .with_fault_for_test(StashFaultPoint::ListChangeBeforeDrop)
        .run();
    assert!(
        matches!(
            outcome(&c),
            OpOutcome::Partial { .. } | OpOutcome::Unknown { .. }
        ),
        "{:?}",
        outcome(&c)
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("file")).unwrap(),
        "two\n"
    );
    assert_eq!(
        &f.ids()[1..],
        before,
        "only the injected external update; no Kagi drop"
    );
    assert_eq!(read_oplog_tail(100).len(), 1);
}
#[test]
fn plan_error_is_recorded_only_after_acceptance_once() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let mut bad = f.request(&s, owner, StashAction::Drop { index: 0 });
    bad.owner.path = f.repo.join("missing");
    let old = plan_stash(&mut s, bad.clone(), StashPolicy::default()).run();
    f.ready(&mut s, owner, StashAction::Drop { index: 0 });
    assert!(!apply_plan(&mut s, old));
    assert!(read_oplog_tail(100).is_empty());
    let c = plan_stash(&mut s, bad, StashPolicy::default()).run();
    let duplicate = c.clone();
    assert!(apply_plan(&mut s, c));
    assert!(!apply_plan(&mut s, duplicate));
    let jobs = s.take_plan_error_jobs();
    assert_eq!(jobs.len(), 1);
    s.invalidate_plan();
    for job in jobs {
        let completion = job.run();
        s.apply_plan_error(completion);
    }
    assert_eq!(read_oplog_tail(100).len(), 1);
    assert!(matches!(s.plan_state(), PlanState::Draft));
}
#[test]
fn admission_both_directions_and_abandoned_job() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let guard = s.write_lease(&f.repo, LegacyBusy(false)).unwrap();
    let token = f.ready(&mut s, owner, StashAction::Drop { index: 0 });
    let approved = approve(&mut s, token, StashPolicy::default()).unwrap();
    assert!(matches!(
        prepare_stash(&mut s, approved, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    guard.complete();
    let job = f.job(&mut s, owner, StashAction::Drop { index: 0 });
    assert!(matches!(
        s.write_lease(&f.repo, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    drop(job);
    assert!(s.has_leases());
    assert_eq!(s.drain_abandoned().len(), 2);
    assert!(!s.has_leases());
    assert_eq!(f.ids().len(), 3);
    assert_eq!(read_oplog_tail(100).len(), 1);
}
#[test]
fn wrappers_record_one_receipt_per_independent_attempt() {
    let f = Fixture::new();
    let mut b = Backend::open(&f.repo).unwrap();
    let plan = b.plan_stash_drop(0).unwrap();
    b.run(&kagi_git::Operation::StashDrop { index: 0 }, &plan)
        .unwrap();
    let plan = b.plan_stash_drop(0).unwrap();
    let report = b.run_recorded(&kagi_git::Operation::StashDrop { index: 0 }, &plan);
    assert!(report.result.is_ok());
    assert_eq!(read_oplog_tail(100).len(), 2);
    assert_eq!(
        read_oplog_tail(100).first().unwrap().id,
        report.recording.entry().id
    );
}
#[test]
fn append_failure_keeps_mutation_result_without_retry() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.log.join("operations.jsonl")).unwrap();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let c = f.job(&mut s, owner, StashAction::Drop { index: 0 }).run();
    assert!(matches!(
        c.report().recording,
        kagi_git::backend::recording::Recording::Failed { .. }
    ));
    assert!(matches!(outcome(&c), OpOutcome::Success { .. }));
    assert_eq!(f.ids().len(), 2);
}

#[test]
fn changed_count_and_missing_target_are_refused_before_mutation() {
    for remove in [false, true] {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let job = f.job(&mut s, owner, StashAction::Drop { index: 1 });
        if remove {
            git(&f.repo, &["stash", "drop", "stash@{1}"]);
        } else {
            std::fs::write(f.repo.join("file"), "four\n").unwrap();
            git(&f.repo, &["stash", "push", "-qm", "four"]);
        }
        let ids = f.ids();
        let c = job.run();
        assert!(matches!(outcome(&c), OpOutcome::Refused { .. }));
        assert_eq!(f.ids(), ids);
        assert_eq!(read_oplog_tail(100).len(), 1);
    }
}

#[test]
fn push_without_untracked_preserves_untracked_bytes_and_message() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    std::fs::write(f.repo.join("file"), "four\n").unwrap();
    std::fs::write(f.repo.join("untracked"), "keep\n").unwrap();
    let c = f
        .job(
            &mut s,
            owner,
            StashAction::Push {
                message: Some("explicit message".into()),
                include_untracked: false,
            },
        )
        .run();
    assert!(matches!(outcome(&c), OpOutcome::Success { .. }));
    assert!(c.report().evidence.verified);
    assert_eq!(f.ids().len(), 4);
    assert!(git(&f.repo, &["stash", "list", "--format=%s"]).contains("explicit message"));
    assert_eq!(
        std::fs::read_to_string(f.repo.join("untracked")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("file")).unwrap(),
        "base\n"
    );
}

#[test]
fn blocker_confirmation_has_one_refused_receipt_no_mutation() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let before = f.ids();
    let mut events = vec![];
    let c = f
        .job(
            &mut s,
            owner,
            StashAction::Push {
                message: None,
                include_untracked: true,
            },
        )
        .run_with_events(|event| events.push(event));
    assert!(matches!(outcome(&c), OpOutcome::Refused { .. }));
    assert!(c.report().evidence.plan_blocked);
    assert_eq!(c.report().evidence.stop, Some(StashStopReason::PlanBlocked));
    assert!(matches!(events.as_slice(), [StashEvent::PlanBlocked]));
    assert!(!c.report().evidence.started);
    assert_eq!(f.ids(), before);
    assert_eq!(read_oplog_tail(100).len(), 1);
}

#[test]
fn blocker_plan_early_failures_report_actual_reason_and_log_lane() {
    {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let before = f.ids();
        let job = f.job(
            &mut s,
            owner,
            StashAction::Push {
                message: None,
                include_untracked: true,
            },
        );
        let moved = f.repo.with_file_name("moved-before-blocker-run");
        std::fs::rename(&f.repo, &moved).unwrap();
        let mut events = vec![];
        let c = job.run_with_events(|event| events.push(event));
        assert!(matches!(outcome(&c), OpOutcome::Failed { .. }));
        assert_eq!(c.report().evidence.stop, Some(StashStopReason::OpenFailed));
        assert!(!c.report().evidence.plan_blocked);
        assert!(matches!(events.as_slice(), [StashEvent::Started]));
        assert_eq!(
            git(&moved, &["stash", "list", "--format=%H"])
                .lines()
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(read_oplog_tail(100).len(), 1);
    }
    {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let before = f.ids();
        let mut events = vec![];
        let c = f
            .job(
                &mut s,
                owner,
                StashAction::Push {
                    message: None,
                    include_untracked: true,
                },
            )
            .with_fault_for_test(StashFaultPoint::Untrusted)
            .run_with_events(|event| events.push(event));
        assert!(
            matches!(outcome(&c), OpOutcome::Failed { error } if error.contains("not trusted"))
        );
        assert_eq!(c.report().evidence.stop, Some(StashStopReason::Untrusted));
        assert!(!c.report().evidence.plan_blocked);
        assert!(matches!(events.as_slice(), [StashEvent::Started]));
        assert_eq!(f.ids(), before);
        assert_eq!(read_oplog_tail(100).len(), 1);
    }
}

#[test]
fn cancellation_policy_revision_and_double_confirmation_are_stale() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let old = f.ready(&mut s, owner, StashAction::Drop { index: 1 });
    s.invalidate_plan();
    assert!(approve(&mut s, old, StashPolicy::default()).is_err());
    let token = f.ready(&mut s, owner, StashAction::Drop { index: 1 });
    assert!(approve(
        &mut s,
        token.clone(),
        StashPolicy {
            auto_snapshot: false,
            ..Default::default()
        }
    )
    .is_err());
    let approved = approve(&mut s, token.clone(), StashPolicy::default()).unwrap();
    assert!(approve(&mut s, token, StashPolicy::default()).is_err());
    let job = prepare_stash(&mut s, approved, LegacyBusy(false)).unwrap();
    let c = job.run();
    s.apply(c);
    assert_eq!(read_oplog_tail(100).len(), 1);
}

#[test]
fn faults_and_open_failure_record_once_without_delivery_retry() {
    for fault in [
        StashFaultPoint::BeforeMutation,
        StashFaultPoint::AfterMutation,
        StashFaultPoint::VerifyFailure,
    ] {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let job = f.job(&mut s, owner, StashAction::Drop { index: 1 });
        let id = job.id();
        let c = job.with_fault_for_test(fault).run();
        match fault {
            StashFaultPoint::BeforeMutation => {
                assert!(matches!(outcome(&c), OpOutcome::Failed { .. }));
                assert_eq!(f.ids().len(), 3);
            }
            StashFaultPoint::AfterMutation => {
                assert!(matches!(outcome(&c), OpOutcome::Unknown { .. }));
                assert_eq!(f.ids().len(), 2);
            }
            StashFaultPoint::VerifyFailure => {
                assert!(matches!(outcome(&c), OpOutcome::Partial { .. }));
                assert!(!c.report().evidence.verified);
                assert_eq!(f.ids().len(), 2);
            }
            _ => unreachable!(),
        }
        let original = format!("{:?}", outcome(&c));
        assert_eq!(format!("{:?}", read_oplog_tail(100)[0].outcome), original);
        let clone = c.clone();
        s.apply(c);
        assert!(s.apply(clone).is_empty());
        assert!(s.drain_abandoned().is_empty());
        assert_eq!(read_oplog_tail(100).len(), 1);
        if matches!(fault, StashFaultPoint::AfterMutation) {
            assert!(matches!(
                s.write_lease(&f.repo, LegacyBusy(false)),
                Err(AdmissionError::NeedsReconcile)
            ));
            let read = prepare_reconcile(&s, id).unwrap().run().unwrap();
            acknowledge(&mut s, read).unwrap();
            s.write_lease(&f.repo, LegacyBusy(false))
                .unwrap()
                .complete();
        }
    }
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let job = f.job(&mut s, owner, StashAction::Drop { index: 1 });
    let moved = f.repo.with_file_name("moved");
    std::fs::rename(&f.repo, &moved).unwrap();
    let c = job.run();
    assert!(matches!(outcome(&c), OpOutcome::Failed { .. }));
    s.apply(c);
    assert_eq!(read_oplog_tail(100).len(), 1);
    assert_eq!(
        git(&moved, &["stash", "list", "--format=%H"])
            .lines()
            .count(),
        3
    );
}

fn conflict(
    f: &Fixture,
    s: &mut Sessions,
    owner: SessionId,
    action: StashAction,
) -> StashCompletion {
    std::fs::write(f.repo.join("file"), "ours\n").unwrap();
    git(&f.repo, &["commit", "-qam", "ours"]);
    let c = f.job(s, owner, action).run();
    assert!(
        matches!(outcome(&c), OpOutcome::Partial { .. }),
        "{:?}",
        outcome(&c)
    );
    assert!(Backend::open(&f.repo)
        .unwrap()
        .detect_conflict_session()
        .is_some());
    assert_eq!(c.report().evidence.conflicts, ["file"]);
    assert_eq!(
        format!("{:?}", outcome(&c)),
        format!("{:?}", read_oplog_tail(100)[0].outcome)
    );
    c
}

#[test]
fn deep_conflict_receipt_payload_continue_and_new_oid_bound_plan() {
    for action in [
        StashAction::Pop { index: 1 },
        StashAction::Apply { index: 1 },
    ] {
        let f = Fixture::new();
        let before = f.ids();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let c = conflict(&f, &mut s, owner, action);
        let id = c.report().recording.entry().id;
        // The owner is closed/inactive: terminal apply still stores its evidence.
        let deliveries = s.apply(c);
        assert_eq!(deliveries.len(), 2);
        let payload = s.stash_conflict(owner).unwrap();
        assert_eq!(payload.oid, before[1]);
        let linked = f.repo.with_file_name("linked");
        git(
            &f.repo,
            &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
        );
        let sibling = s.attach(linked.clone());
        assert_ne!(sibling, owner, "a linked worktree is its own session");
        assert!(s.stash_conflict(sibling).is_none());
        assert!(s.take_stash_followup(sibling).is_none());
        assert_eq!(f.ids(), before);
        let b = Backend::open(&f.repo).unwrap();
        s.observe_stash_conflict(owner, &b.stash_conflict_identity().unwrap());
        let session = b.detect_conflict_session().unwrap();
        let mut buffer = b.resolution_buffer_from_repo_with_autosave().unwrap();
        buffer
            .apply_choice(Path::new("file"), kagi_git::ResolutionChoice::Incoming)
            .unwrap();
        let head = git(&f.repo, &["rev-parse", "HEAD"]);
        b.execute_conflict_continue(&session, &buffer).unwrap();
        assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            std::fs::read_to_string(f.repo.join("file")).unwrap(),
            "two\n"
        );
        s.continue_stash_conflict(owner);
        s.observe_stash_conflict(owner, &[]);
        assert!(
            s.stash_conflict(owner).is_none(),
            "resolved conflicts must leave the active-conflict map"
        );
        let payload = s.take_stash_followup(owner).unwrap();
        assert!(s.take_stash_followup(owner).is_none());
        let attachment = f.request(&s, owner, StashAction::Drop { index: 0 }).owner;
        let job = plan_stash_followup(&mut s, attachment, payload.oid, StashPolicy::default());
        assert!(apply_plan(&mut s, job.run()));
        let PlanState::Ready {
            token,
            prepared: Planned::Stash { plan, .. },
        } = s.plan_state()
        else {
            panic!("ready")
        };
        assert_eq!(plan.action, StashAction::Drop { index: 1 });
        let token = token.clone();
        assert_eq!(read_oplog_tail(100)[0].id, id, "planning must not append");
        // Cancel preserves every stash; original approval cannot be reused.
        s.invalidate_plan();
        assert!(approve(&mut s, token, StashPolicy::default()).is_err());
        assert_eq!(f.ids(), before);
    }
}

#[test]
fn continued_conflict_owner_close_clears_before_or_after_reload() {
    for reload_settled in [false, true] {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let completion = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
        s.apply(completion);
        s.continue_stash_conflict(owner);
        if reload_settled {
            s.observe_stash_conflict(owner, &[]);
            assert!(s.stash_conflict(owner).is_none());
        }

        // #482 stage 1: the host's tab-close adapter detaches the session, and
        // the conflict/follow-up payloads go with it. Closing on either side of
        // the reload transition must discard the one-shot, and the tab reopened
        // on the same path is a new owner that inherits none of it.
        s.detach(owner);
        assert!(!s.is_attached(owner));
        let reopened = s.attach(f.repo.clone());
        assert_ne!(reopened, owner);
        s.observe_stash_conflict(reopened, &[]); // reopen sees a clean index
        assert!(s.stash_conflict(reopened).is_none());
        assert!(s.take_stash_followup(reopened).is_none());
        assert!(s.stash_conflict(owner).is_none());
        assert!(s.take_stash_followup(owner).is_none());
    }
}

#[test]
fn unrelated_drop_during_conflict_preserves_origin_payload() {
    for duplicate_drop_oid in [false, true] {
        let f = Fixture::new();
        let initial = f.ids();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let conflict = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
        s.apply(conflict);
        let origin = s.stash_conflict(owner).unwrap();
        let conflict_id = origin.operation;
        let origin_oid = origin.oid.clone();
        assert_eq!(origin_oid, initial[1]);

        let unrelated_oid = initial[0].clone();
        if duplicate_drop_oid {
            git(
                &f.repo,
                &["stash", "store", "-m", "duplicate B", &unrelated_oid],
            );
        }
        let before = f.ids();
        let bytes = std::fs::read(f.repo.join("file")).unwrap();
        let index = git(&f.repo, &["ls-files", "--stage"]);
        let drop = f.job(&mut s, owner, StashAction::Drop { index: 0 }).run();
        assert!(matches!(outcome(&drop), OpOutcome::Success { .. }));
        assert!(drop.report().evidence.conflicts.is_empty());
        assert_eq!(f.ids().len(), before.len() - 1);
        assert_eq!(std::fs::read(f.repo.join("file")).unwrap(), bytes);
        assert_eq!(git(&f.repo, &["ls-files", "--stage"]), index);
        s.apply(drop);

        let payload = s.stash_conflict(owner).unwrap();
        assert_eq!(payload.operation, conflict_id);
        assert_eq!(payload.oid, origin_oid);
        let entries = read_oplog_tail(100);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0].outcome, OpOutcome::Success { .. }));
        assert!(matches!(entries[1].outcome, OpOutcome::Partial { .. }));
    }
}

#[test]
fn followup_never_chooses_missing_or_duplicate_oid_or_unknown_origin() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    assert!(s.stash_conflict(owner).is_none());
    s.continue_stash_conflict(owner);
    assert!(s.take_stash_followup(owner).is_none());
    let oid = f.ids()[1].clone();
    git(&f.repo, &["stash", "store", "-m", "duplicate", &oid]);
    assert!(Backend::plan_stash_drop_by_oid(&f.repo, &oid)
        .unwrap()
        .is_none());
    assert!(Backend::plan_stash_drop_by_oid(&f.repo, &"0".repeat(40))
        .unwrap()
        .is_none());
    let attachment = f.request(&s, owner, StashAction::Drop { index: 0 }).owner;
    let job = plan_stash_followup(&mut s, attachment, oid, StashPolicy::default());
    assert!(apply_plan(&mut s, job.run()));
    assert!(matches!(s.plan_state(), PlanState::Draft));
    assert!(read_oplog_tail(100).is_empty());
}

#[test]
fn conflict_abort_end_and_replacement_clear_only_matching_owner() {
    for abort in [false, true] {
        let f = Fixture::new();
        let ids = f.ids();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let c = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
        s.apply(c);
        let other = s.attach(f.repo.with_file_name("other"));
        s.observe_stash_conflict(other, &[]);
        assert!(s.stash_conflict(owner).is_some());
        if abort {
            let b = Backend::open(&f.repo).unwrap();
            let session = b.detect_conflict_session().unwrap();
            let buffer = b.resolution_buffer_from_repo_with_autosave().unwrap();
            b.execute_stash_conflict_abort(&session, &buffer).unwrap();
            s.clear_stash_conflict(owner);
        } else {
            s.observe_stash_conflict(owner, &["different index conflict".into()]);
        }
        assert!(s.stash_conflict(owner).is_none());
        assert_eq!(f.ids(), ids);
    }
}

#[test]
fn drop_receipt_can_restore_exact_content_despite_another_append() {
    let f = Fixture::new();
    let ids = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let c = f.job(&mut s, owner, StashAction::Drop { index: 1 }).run();
    let original = c.report().recording.entry().clone();
    assert!(c.report().evidence.snapshot.is_none());
    let mut b = Backend::open(&f.repo).unwrap();
    let plan = b.plan_stash_drop(0).unwrap();
    b.run(&kagi_git::Operation::StashDrop { index: 0 }, &plan)
        .unwrap();
    assert_ne!(read_oplog_tail(100)[0].id, original.id);
    let deliveries = s.apply(c);
    let Delivery::Completed { report, .. } = &deliveries[1] else {
        panic!("completed")
    };
    assert_eq!(report.recording.entry().id, original.id);
    assert!(format!("{:?}", original.outcome).contains(&ids[1]));
    git(&f.repo, &["stash", "store", "-m", "recovered", &ids[1]]);
    assert_eq!(
        git(&f.repo, &["show", &format!("{}:file", ids[1])]),
        "two\n"
    );
    assert_eq!(read_oplog_tail(100).len(), 2);
}

#[test]
fn fresh_open_trust_refusal_never_mutates() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let before = f.ids();
    let c = f
        .job(&mut s, owner, StashAction::Drop { index: 1 })
        .with_fault_for_test(StashFaultPoint::Untrusted)
        .run();
    assert!(matches!(outcome(&c), OpOutcome::Failed { error } if error.contains("not trusted")));
    assert_eq!(f.ids(), before);
    assert!(!c.report().evidence.started);
    s.apply(c);
    assert_eq!(read_oplog_tail(100).len(), 1);
}

#[test]
fn all_legacy_writers_and_remove_exclude_stash_in_both_orders() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let linked = f.repo.with_file_name("linked");
    git(
        &f.repo,
        &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
    );
    git(
        &linked,
        &["remote", "add", "origin", f.repo.to_str().unwrap()],
    );
    for writer in 0..5 {
        let guard = s.write_lease(&linked, LegacyBusy(false)).unwrap();
        let token = f.ready(&mut s, owner, StashAction::Drop { index: 1 });
        let approval = approve(&mut s, token, StashPolicy::default()).unwrap();
        assert!(matches!(
            prepare_stash(&mut s, approval, LegacyBusy(false)),
            Err(AdmissionError::Busy)
        ));
        assert_eq!(f.ids().len(), 3);
        match writer {
            0 => guard.run(|| std::fs::write(linked.join("file"), "saved\n").unwrap()),
            1 => guard.run(|| {
                Backend::open(&linked)
                    .unwrap()
                    .stage_file(Path::new("file"))
                    .unwrap()
            }),
            2 => guard.run(|| {
                Backend::open(&linked)
                    .unwrap()
                    .create_snapshot("writer")
                    .unwrap();
            }),
            _ => {
                let b = Backend::open(&linked).unwrap();
                let result = if writer == 3 {
                    b.fetch_remote()
                } else {
                    b.fetch_remote_branch("origin/main")
                };
                guard.complete_git(&result);
                result.unwrap();
            }
        }
        let job = f.job(&mut s, owner, StashAction::Drop { index: 1 });
        let bytes = std::fs::read(linked.join("file")).unwrap();
        let index = git(&linked, &["ls-files", "--stage"]);
        assert!(matches!(
            s.write_lease(&linked, LegacyBusy(false)),
            Err(AdmissionError::Busy)
        ));
        assert_eq!(std::fs::read(linked.join("file")).unwrap(), bytes);
        assert_eq!(git(&linked, &["ls-files", "--stage"]), index);
        drop(job);
        s.drain_abandoned();
    }
    // A clean second linked tree gives remove a valid prepared job.
    let clean = f.repo.with_file_name("clean");
    git(
        &f.repo,
        &["worktree", "add", "-qb", "clean", clean.to_str().unwrap()],
    );
    let request = RemoveRequest {
        owner: f.request(&s, owner, StashAction::Drop { index: 1 }).owner,
        name: "clean".into(),
        delete_branch: true,
    };
    for stash_first in [false, true] {
        if stash_first {
            let stash = f.job(&mut s, owner, StashAction::Drop { index: 1 });
            let plan = plan_remove(&mut s, request.clone(), RemovePolicy::default());
            apply_plan(&mut s, plan.run());
            let PlanState::Ready { token, .. } = s.plan_state() else {
                panic!("ready")
            };
            let token = token.clone();
            let approval = approve(&mut s, token, RemovePolicy::default()).unwrap();
            assert!(matches!(
                prepare_remove(&mut s, approval, LegacyBusy(false)),
                Err(AdmissionError::Busy)
            ));
            drop(stash);
            s.drain_abandoned();
        } else {
            let plan = plan_remove(&mut s, request.clone(), RemovePolicy::default());
            apply_plan(&mut s, plan.run());
            let PlanState::Ready { token, .. } = s.plan_state() else {
                panic!("ready")
            };
            let token = token.clone();
            let approval = approve(&mut s, token, RemovePolicy::default()).unwrap();
            let remove = prepare_remove(&mut s, approval, LegacyBusy(false)).unwrap();
            let token = f.ready(&mut s, owner, StashAction::Drop { index: 1 });
            let approval = approve(&mut s, token, StashPolicy::default()).unwrap();
            assert!(matches!(
                prepare_stash(&mut s, approval, LegacyBusy(false)),
                Err(AdmissionError::Busy)
            ));
            drop(remove);
            s.drain_abandoned();
        }
    }
    assert!(clean.exists());
    assert_eq!(f.ids().len(), 3);
}

#[test]
fn late_ready_cannot_replace_new_error_or_cancelled_modal() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let request = f.request(&s, owner, StashAction::Drop { index: 1 });
    let old = plan_stash(&mut s, request, StashPolicy::default()).run();
    let mut request = f.request(
        &s,
        owner,
        StashAction::Push {
            message: Some("new input".into()),
            include_untracked: true,
        },
    );
    request.owner.path = f.repo.join("missing");
    let error = plan_stash(&mut s, request, StashPolicy::default()).run();
    assert!(apply_plan(&mut s, error));
    assert!(!apply_plan(&mut s, old));
    assert!(matches!(s.plan_state(), PlanState::Error { .. }));
    let request = f.request(&s, owner, StashAction::Drop { index: 1 });
    let old = plan_stash(&mut s, request, StashPolicy::default()).run();
    s.invalidate_plan();
    assert!(!apply_plan(&mut s, old));
    assert!(matches!(s.plan_state(), PlanState::Draft));
    for job in s.take_plan_error_jobs() {
        s.apply_plan_error(job.run());
    }
    assert_eq!(read_oplog_tail(100).len(), 1);
    assert_eq!(f.ids().len(), 3);
}

#[test]
fn lost_delivery_never_appends_again_or_claims_unconfirmed_release() {
    let f = Fixture::new();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let completion = f.job(&mut s, owner, StashAction::Drop { index: 1 }).run();
    drop(completion);
    assert!(s.drain_abandoned().is_empty());
    assert!(s.has_leases());
    assert!(!s.may_close_host());
    assert_eq!(read_oplog_tail(100).len(), 1);
    assert_eq!(f.ids().len(), 2);
}

/// #482 review P2: leaving a tab ends the visit. A pending follow-up proposal is
/// discarded, a completion that lands after the departure creates no proposal,
/// and on return the payload proposes nothing until a **live** re-observation of
/// the same conflict re-proves it. The OID a proposal carries therefore always
/// comes from the current repository state, never from a preserved payload.
#[test]
fn departing_a_tab_expires_proposals_and_late_completions_make_none() {
    let f = Fixture::new();
    let before = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());

    // A completion that lands after the user left A proposes nothing.
    let late = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
    s.depart(owner);
    s.apply(late);
    assert!(
        s.stash_conflict(owner).is_none(),
        "late completion, no proposal"
    );
    let live = Backend::open(&f.repo)
        .unwrap()
        .stash_conflict_identity()
        .unwrap();
    assert!(
        !live.is_empty(),
        "the repository really is still conflicted"
    );
    s.observe_stash_conflict(owner, &live);
    assert!(
        s.stash_conflict(owner).is_none(),
        "a proposal the user never saw is not resurrected by observing the conflict"
    );
    assert_eq!(f.ids(), before);
}

/// Continuing and then leaving before the authoritative reload lands must not
/// leave a drop proposal waiting on the next visit.
#[test]
fn departing_between_continue_and_reload_discards_the_follow_up() {
    let f = Fixture::new();
    let before = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let completion = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
    s.apply(completion);
    let b = Backend::open(&f.repo).unwrap();
    s.observe_stash_conflict(owner, &b.stash_conflict_identity().unwrap());
    s.continue_stash_conflict(owner);
    s.depart(owner);
    s.observe_stash_conflict(owner, &[]); // the reload finally lands
    assert!(s.take_stash_followup(owner).is_none());
    assert!(s.stash_conflict(owner).is_none());
    assert_eq!(f.ids(), before);
}

#[test]
fn a_proposal_is_re_proven_by_live_re_observation_after_a_return() {
    let f = Fixture::new();
    let before = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let completion = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
    s.apply(completion);
    assert_eq!(s.stash_conflict(owner).unwrap().oid, before[1]);

    // A→B: same tab, same incarnation, but the visit is over.
    s.depart(owner);
    assert!(s.is_attached(owner), "the tab is still open");
    assert!(
        s.stash_conflict(owner).is_none(),
        "a departed visit proposes nothing"
    );
    s.continue_stash_conflict(owner);
    assert!(
        s.stash_conflict(owner).is_none(),
        "and cannot be marked continued while inert"
    );

    // B→A: the reload re-observes the real conflict, and *that* is what makes it
    // proposable again — the OID is the one the live conflict still matches.
    let b = Backend::open(&f.repo).unwrap();
    let live = b.stash_conflict_identity().unwrap();
    assert!(!live.is_empty());
    s.observe_stash_conflict(owner, &live);
    let payload = s
        .stash_conflict(owner)
        .expect("a live conflict is proposable again");
    assert_eq!(
        payload.oid, before[1],
        "the OID comes from the re-observed conflict"
    );
    assert_eq!(f.ids(), before);
    // The stale continue was not carried over: promoting still needs a fresh one.
    s.observe_stash_conflict(owner, &[]);
    assert!(s.take_stash_followup(owner).is_none());
}

/// Resolving the conflict while away leaves nothing to re-prove on return.
#[test]
fn a_conflict_resolved_while_away_proposes_nothing_on_return() {
    let f = Fixture::new();
    let before = f.ids();
    let mut s = Sessions::new();
    let owner = s.attach(f.repo.clone());
    let completion = conflict(&f, &mut s, owner, StashAction::Pop { index: 1 });
    s.apply(completion);
    s.depart(owner);

    let b = Backend::open(&f.repo).unwrap();
    let session = b.detect_conflict_session().unwrap();
    let mut buffer = b.resolution_buffer_from_repo_with_autosave().unwrap();
    buffer
        .apply_choice(Path::new("file"), kagi_git::ResolutionChoice::Incoming)
        .unwrap();
    b.execute_conflict_continue(&session, &buffer).unwrap();

    // The return re-observes a clean index: nothing to propose, and the old
    // payload is not promoted to a drop follow-up.
    s.observe_stash_conflict(owner, &[]);
    assert!(s.stash_conflict(owner).is_none());
    assert!(s.take_stash_followup(owner).is_none());
    assert_eq!(f.ids(), before, "no stash was dropped without a proposal");
}

/// #482 review P2: `refs/stash` is shared, so push/pop/drop make every open
/// sibling worktree stale. `apply` writes only this worktree's index and working
/// tree, so it stays scoped to its own target.
#[test]
fn shared_stash_changes_reach_siblings_and_index_only_changes_do_not() {
    for (action, shared) in [
        (StashAction::Drop { index: 1 }, true),
        (StashAction::Apply { index: 1 }, false),
    ] {
        let f = Fixture::new();
        let mut s = Sessions::new();
        let owner = s.attach(f.repo.clone());
        let linked = f.repo.with_file_name("linked");
        git(
            &f.repo,
            &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
        );
        let sibling = Backend::open(&linked)
            .and_then(|b| b.write_worktree_id())
            .unwrap();
        let _open = s.attach(linked.clone());

        let done = f.job(&mut s, owner, action.clone()).run();
        let deliveries = s.apply(done);
        assert_eq!(s.is_stale(&sibling), shared, "{action:?} sibling staleness");
        assert_eq!(
            deliveries.iter().any(|delivery| matches!(
                delivery,
                Delivery::Invalidate(target) if target.worktree == sibling
            )),
            shared,
            "{action:?} sibling delivery"
        );
        assert!(matches!(
            deliveries.last(),
            Some(Delivery::Completed { .. })
        ));
    }
}
