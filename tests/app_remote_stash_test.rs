use kagi::app::{
    self, approve, plan_remote_stash_for_test, AdmissionError, LegacyBusy, PlanState,
    RemoteStashRequest, Sessions, StashPolicy,
};
use kagi::remote::stash::{RemoteAttachment, RemotePlanFixture, RemoteStashFault};
use kagi_domain::remote::{
    KnownHostsIdentity, RemoteConnectionId, RemoteDropOutcome, RemoteHost, RemoteStashState,
};
use std::sync::Mutex;
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn oid(ch: char) -> String {
    std::iter::repeat_n(ch, 40).collect()
}

fn connection(hostname: &str) -> RemoteConnectionId {
    RemoteConnectionId {
        hostname: hostname.into(),
        user: "alice".into(),
        port: 22,
        host_key_alias: None,
        identity_files: vec!["/keys/id".into()],
        certificate_files: vec![],
        user_known_hosts: vec![KnownHostsIdentity {
            path: "/known".into(),
            digest: "1".repeat(64),
        }],
        global_known_hosts: vec![],
        host_key_algorithms: vec!["ssh-ed25519".into()],
    }
}

fn prepare(
    sessions: &mut Sessions,
    fault: RemoteStashFault,
    root: &str,
    common_dir: &str,
) -> Result<app::Job, AdmissionError> {
    let host = RemoteHost {
        user: Some("alice".into()),
        host: "host".into(),
        port: Some(22),
        identity_file: Some("/keys/id".into()),
    };
    let request = RemoteStashRequest {
        owner: RemoteAttachment {
            host,
            root: root.into(),
            generation: 7,
        },
        index: 1,
    };
    let fixture = RemotePlanFixture {
        connection: connection("host"),
        common_dir: common_dir.into(),
        before: RemoteStashState {
            head: oid('f'),
            ordered_oids: vec![oid('a'), oid('b'), oid('c')],
            index_fingerprint: oid('d'),
            worktree_fingerprint: oid('e'),
        },
    };
    let policy = StashPolicy::default();
    let completion = plan_remote_stash_for_test(sessions, request, policy.clone(), fixture).run();
    assert!(app::apply_plan(sessions, completion));
    let PlanState::Ready { token, .. } = sessions.plan_state() else {
        panic!("remote plan must be ready")
    };
    let approved = approve(sessions, token.clone(), policy).unwrap();
    let job = app::prepare(sessions, approved, LegacyBusy(false))?;
    Ok(match job {
        app::Job::Stash(job) => app::Job::Stash(job.with_remote_fault_for_test(fault)),
        _ => unreachable!(),
    })
}

fn run(sessions: &mut Sessions, fault: RemoteStashFault) -> (app::OperationId, RemoteDropOutcome) {
    let completion = prepare(sessions, fault, "/srv/linked", "/srv/repo/.git")
        .unwrap()
        .run();
    let app::Completion::Stash(stash) = &completion else {
        unreachable!()
    };
    let report = stash.remote_report().unwrap();
    let outcome = report.evidence.outcome;
    let deliveries = app::apply(sessions, completion);
    let id = deliveries
        .into_iter()
        .find_map(|delivery| match delivery {
            app::Delivery::RemoteCompleted { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();
    (id, outcome)
}

#[test]
fn remote_outcome_matrix_and_single_scope_lease() {
    let _guard = ENV_LOCK.lock().unwrap();
    let cases = [
        (RemoteStashFault::Success, RemoteDropOutcome::Success, false),
        (
            RemoteStashFault::ConfigDrift,
            RemoteDropOutcome::Refused,
            false,
        ),
        (
            RemoteStashFault::PreflightDrift,
            RemoteDropOutcome::Refused,
            false,
        ),
        (
            RemoteStashFault::NonZeroUnchanged,
            RemoteDropOutcome::Failed,
            false,
        ),
        (
            RemoteStashFault::VerifyMismatch,
            RemoteDropOutcome::Partial,
            false,
        ),
        (
            RemoteStashFault::LocalSpawn,
            RemoteDropOutcome::Failed,
            false,
        ),
        (
            RemoteStashFault::MissingToken,
            RemoteDropOutcome::Unknown,
            true,
        ),
    ];
    for (fault, expected, held) in cases {
        let log = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", log.path());
        let mut sessions = Sessions::new();
        let (_, outcome) = run(&mut sessions, fault);
        assert_eq!(outcome, expected);
        assert_eq!(sessions.has_leases(), held);
    }
}

#[test]
fn slow_before_state_with_live_writer_never_acknowledges() {
    let _guard = ENV_LOCK.lock().unwrap();
    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let mut sessions = Sessions::new();
    let (id, outcome) = run(&mut sessions, RemoteStashFault::TimeoutBeforeDrop);
    assert_eq!(outcome, RemoteDropOutcome::Unknown);
    assert!(sessions.has_leases());
    assert!(app::read_reconcile(&sessions, id).is_err());
    assert!(matches!(
        prepare(
            &mut sessions,
            RemoteStashFault::Success,
            "/srv/main",
            "/srv/repo/.git"
        ),
        Err(_)
    ));
    assert!(
        sessions.has_leases(),
        "no ack token may release the live writer"
    );
}

#[test]
fn only_matching_completion_token_plus_read_releases_unknown() {
    let _guard = ENV_LOCK.lock().unwrap();
    let faults = [
        RemoteStashFault::MalformedToken,
        RemoteStashFault::WrongScopeToken,
        RemoteStashFault::UnreadableToken,
    ];
    for fault in faults {
        let log = tempfile::tempdir().unwrap();
        std::env::set_var("KAGI_LOG_DIR", log.path());
        let mut sessions = Sessions::new();
        let (id, _) = run(&mut sessions, fault);
        assert!(app::read_reconcile(&sessions, id).is_err());
        assert!(sessions.has_leases());
    }

    let log = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log.path());
    let mut sessions = Sessions::new();
    let (id, _) = run(&mut sessions, RemoteStashFault::ValidTokenAfterTimeout);
    let read = app::read_reconcile(&sessions, id).unwrap();
    app::acknowledge(&mut sessions, read).unwrap();
    assert!(!sessions.has_leases());
}

#[test]
fn main_and_linked_roots_share_remote_repo_identity() {
    let left = kagi_domain::remote::RemoteRepoId {
        connection: connection("host"),
        common_dir: "/srv/repo/.git".into(),
    };
    let right = kagi_domain::remote::RemoteRepoId {
        connection: connection("host"),
        common_dir: "/srv/repo/.git".into(),
    };
    assert_eq!(left, right);
    assert_ne!(
        left,
        kagi_domain::remote::RemoteRepoId {
            connection: connection("other"),
            common_dir: "/srv/repo/.git".into(),
        }
    );
}
