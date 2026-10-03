use kagi::app::{self, AdmissionError, Job, PlanState, Planned, Sessions, WriteScope};
use kagi::remote::stash::RemoteAttachment;
use kagi::remote::{RemoteError, RemotePullReport};
use kagi_domain::remote::{RemoteConnectionId, RemoteHost, RemoteRepoId};
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::StateSummary;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn plan(sessions: &mut Sessions) -> (Job, RemoteRepoId) {
    let host = RemoteHost {
        user: Some("alice".into()),
        host: "example.invalid".into(),
        port: None,
        identity_file: None,
    };
    let session = sessions.attach(PathBuf::from("alice@example.invalid:/srv/repo"));
    let request = app::RemotePullRequest {
        owner: RemoteAttachment {
            session,
            host,
            root: "/srv/repo".into(),
        },
        plan: Arc::new(kagi_git::plan_pull_remote(
            "main",
            "origin/main",
            1,
            0,
            false,
            "main".into(),
        )),
    };
    let repo_id = RemoteRepoId {
        connection: RemoteConnectionId {
            hostname: "example.invalid".into(),
            user: "alice".into(),
            port: 22,
            host_key_alias: None,
            identity_files: vec![],
            certificate_files: vec![],
            user_known_hosts: vec![],
            global_known_hosts: vec![],
            host_key_algorithms: vec![],
        },
        common_dir: "/srv/repo/.git".into(),
    };
    let completion = app::plan_remote_pull_for_test(sessions, request, repo_id.clone()).run();
    assert!(matches!(sessions.plan_state(), PlanState::Planning { .. }));
    assert!(app::apply_plan(sessions, completion));
    let PlanState::Ready { token, prepared } = sessions.plan_state() else {
        panic!("remote pull plan not ready")
    };
    let Planned::RemotePull { plan, .. } = prepared else {
        panic!("wrong plan family")
    };
    assert_eq!(plan.repo_id, repo_id);
    assert_eq!(prepared.scope(), WriteScope::Remote(repo_id.clone()));
    let approved = app::approve(
        sessions,
        token.clone(),
        app::Policy::Stash(Default::default()),
    )
    .unwrap();
    let job = app::prepare(sessions, approved).unwrap();
    assert!(sessions.has_leases());
    assert!(!sessions.may_close_host());
    (job, repo_id)
}

fn report(outcome: OpOutcome) -> RemotePullReport {
    let result = if matches!(outcome, OpOutcome::Success { .. }) {
        Ok("Fast-forward".into())
    } else {
        Err(RemoteError::NonZero {
            code: 1,
            stderr: "uncertain".into(),
        })
    };
    RemotePullReport {
        result,
        recording: kagi_git::backend::recording::finalize(OpLogEntry::new(
            "pull",
            "alice@example.invalid:/srv/repo",
            StateSummary {
                head: "main".into(),
                dirty: "clean".into(),
            },
            outcome,
        )),
    }
}

#[test]
fn remote_pull_terminal_matrix_and_unobservable_release() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let after = StateSummary {
        head: "main".into(),
        dirty: "unknown".into(),
    };
    for (outcome, parked) in [
        (
            OpOutcome::Success {
                after: after.clone(),
            },
            false,
        ),
        (
            OpOutcome::Failed {
                error: "refused".into(),
            },
            false,
        ),
        (
            OpOutcome::Unknown {
                after: after.clone(),
                evidence: "unknown".into(),
            },
            true,
        ),
        (
            OpOutcome::Partial {
                after: after.clone(),
                error: "mid-merge".into(),
            },
            true,
        ),
    ] {
        let mut sessions = Sessions::new();
        let (job, _) = plan(&mut sessions);
        assert!(matches!(&sessions.plan_state(), PlanState::Draft));
        let Job::RemotePull(job) = job else {
            panic!("not a remote pull job")
        };
        let id = job.id();
        let completion = (*job).with_report_for_test(report(outcome)).run();
        let deliveries = app::apply(&mut sessions, completion);
        assert!(
            matches!(deliveries.last(), Some(app::Delivery::RemoteCompleted { id: delivered, .. }) if *delivered == id)
        );
        assert_eq!(
            sessions.has_leases(),
            parked,
            "Unknown/Partial must retain the remote lease"
        );
        assert_eq!(
            sessions.reconcile_ids().contains(&id),
            parked,
            "uncertain remote pull needs reconcile"
        );
        if parked {
            let read = app::read_reconcile(&sessions, id).unwrap();
            assert!(read.stop_proven() && read.can_acknowledge_unobserved());
            assert_eq!(
                app::acknowledge(&mut sessions, read.clone()),
                Err(AdmissionError::NeedsReconcile)
            );
            let release = app::prepare_unobservable_release(&sessions, read)
                .expect("remote pull is eligible for audited release")
                .run();
            app::acknowledge_unobserved(&mut sessions, release).unwrap();
            assert!(!sessions.has_leases());
            assert!(sessions.reconcile_ids().is_empty());
        }
    }
}

#[test]
fn dropped_remote_pull_job_parks_unknown_instead_of_unlocking() {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let mut sessions = Sessions::new();
    let (job, _) = plan(&mut sessions);
    let Job::RemotePull(job) = job else {
        panic!("not remote")
    };
    let id = job.id();
    drop(job);
    let deliveries = sessions.drain_abandoned();
    assert!(
        matches!(deliveries.last(), Some(app::Delivery::RemoteCompleted { id: delivered, report, .. }) if *delivered == id && matches!(report.recording.entry().outcome, OpOutcome::Unknown { .. }))
    );
    assert!(sessions.has_leases(), "abandonment must retain the lease");
    let read = app::read_reconcile(&sessions, id).unwrap();
    assert!(read.can_acknowledge_unobserved());
    let release = app::prepare_unobservable_release(&sessions, read)
        .unwrap()
        .run();
    app::acknowledge_unobserved(&mut sessions, release).unwrap();
    assert!(!sessions.has_leases());
}
