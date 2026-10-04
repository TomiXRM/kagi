use kagi::app::{self, AdmissionError, Job, PlanState, Planned, Sessions, WriteScope};
use kagi::remote::stash::RemoteAttachment;
use kagi::remote::{PullRepoIdentity, RemoteError, RemotePullReport};
use kagi_domain::remote::{
    RemoteConnectionId, RemoteHost, RemotePullConfig, RemotePullFingerprint, RemotePullHead,
    RemoteRepoId,
};
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::StateSummary;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct PathRestore(Option<std::ffi::OsString>);
impl Drop for PathRestore {
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

fn fixture(sessions: &mut Sessions) -> (app::RemotePullRequest, PullRepoIdentity) {
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
        cached_head_oid: Some("a".repeat(40)),
        cached_remote_dirty: false,
    };
    let repo_id = RemoteRepoId {
        connection: Arc::new(RemoteConnectionId {
            hostname: "example.invalid".into(),
            user: "alice".into(),
            port: 22,
            host_key_alias: None,
            proxy_jump: None,
            proxy_command: None,
            control_master: None,
            control_path: None,
            identity_files: vec![],
            certificate_files: vec![],
            user_known_hosts: vec![],
            global_known_hosts: vec![],
            host_key_algorithms: vec![],
        }),
        common_dir: "/srv/repo/.git".into(),
    };
    let head = RemotePullHead {
        branch: Some("main".into()),
        oid: "a".repeat(40),
        upstream: Some("origin/main".into()),
    };
    let config = RemotePullConfig {
        remote_name: "origin".into(),
        merge_ref: "refs/heads/main".into(),
        remote_url: "ssh://example.invalid/repo".into(),
        fetch_refspecs: vec!["+refs/heads/*:refs/remotes/origin/*".into()],
    };
    let fingerprint = RemotePullFingerprint {
        staged: [1; 32],
        worktree: [2; 32],
    };
    let identity = PullRepoIdentity {
        repo_id: repo_id.clone(),
        physical_toplevel: "/srv/repo".into(),
        head: head.clone(),
        config: Some(config.clone()),
        fingerprint: fingerprint.clone(),
        remote_dirty: false,
    };
    (request, identity)
}

fn plan(sessions: &mut Sessions) -> (Job, PullRepoIdentity) {
    let (request, identity) = fixture(sessions);
    let repo_id = &identity.repo_id;
    let head = &identity.head;
    let config = identity.config.as_ref().unwrap();
    let fingerprint = &identity.fingerprint;
    let completion = app::plan_remote_pull_for_test(
        sessions,
        request,
        repo_id.clone(),
        identity.physical_toplevel.clone(),
        head.clone(),
        config.clone(),
        fingerprint.clone(),
        false,
    )
    .run();
    assert!(matches!(sessions.plan_state(), PlanState::Planning { .. }));
    assert!(app::apply_plan(sessions, completion));
    let PlanState::Ready { token, prepared } = sessions.plan_state() else {
        panic!("remote pull plan not ready")
    };
    let Planned::RemotePull { plan, .. } = prepared else {
        panic!("wrong plan family")
    };
    assert_eq!(&plan.repo_id, repo_id);
    assert_eq!(plan.physical_toplevel, "/srv/repo");
    assert_eq!(&plan.head, head);
    assert_eq!(&plan.config, config);
    assert_eq!(&plan.fingerprint, fingerprint);
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
    (job, identity)
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

#[test]
fn stale_cached_head_or_dirty_preview_never_opens_a_pull_confirmation() {
    for mismatch in ["head", "dirty"] {
        let mut sessions = Sessions::new();
        let (request, mut identity) = fixture(&mut sessions);
        if mismatch == "head" {
            identity.head.oid = "b".repeat(40);
        } else {
            identity.remote_dirty = true;
            identity.fingerprint.worktree = [3; 32];
        }
        let completion = app::plan_remote_pull_for_test(
            &mut sessions,
            request,
            identity.repo_id,
            identity.physical_toplevel,
            identity.head,
            identity.config.unwrap(),
            identity.fingerprint,
            identity.remote_dirty,
        )
        .run();
        assert!(app::apply_plan(&mut sessions, completion));
        assert!(
            matches!(
                sessions.plan_state(),
                PlanState::Error {
                    blocker: Some(kagi_domain::plan_note::PlanNote::Pull(
                        kagi_domain::plan_note::PullNote::RemotePreviewStale
                    )),
                    ..
                }
            ),
            "cached {mismatch} must refuse before confirmation"
        );
        assert!(!sessions.has_leases());
        assert!(sessions.may_close_host());
    }
}

#[cfg(unix)]
#[test]
fn remote_pull_preflight_refuses_changed_status_config_and_repo_without_ssh_pull() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", root.path());
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let calls = root.path().join("ssh-calls");
    let ssh = bin.join("ssh");
    std::fs::write(
        &ssh,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 99\n",
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _restore = PathRestore(std::env::var_os("PATH"));
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &_restore.0.clone().unwrap_or_default(),
    ));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

    for (index, mismatch) in ["status", "config", "repo"].into_iter().enumerate() {
        let mut sessions = Sessions::new();
        let (job, mut observed) = plan(&mut sessions);
        match mismatch {
            "status" => observed.fingerprint.worktree = [9; 32],
            "config" => observed.config.as_mut().unwrap().remote_url = "ssh://other/repo".into(),
            "repo" => observed.repo_id.common_dir = "/srv/other/.git".into(),
            _ => unreachable!(),
        }
        let Job::RemotePull(job) = job else {
            panic!("wrong job family");
        };
        let id = job.id();
        let completion = (*job).with_preflight_identity_for_test(Ok(observed)).run();
        assert!(
            matches!(
                completion.report.recording.entry().outcome,
                OpOutcome::Refused { .. }
            ),
            "{mismatch} drift must be Refused, not a failed or successful SSH pull"
        );
        app::apply(&mut sessions, completion);
        assert!(!sessions.has_leases(), "{mismatch} refusal releases lease");
        assert!(sessions.may_close_host());
        assert!(!sessions.reconcile_ids().contains(&id));
        assert_eq!(
            kagi_git::oplog::read_oplog_tail(10)
                .iter()
                .filter(|entry| matches!(entry.outcome, OpOutcome::Refused { .. }))
                .count(),
            index + 1,
            "each refusal is durable"
        );
        assert!(!calls.exists(), "{mismatch} drift must not call ssh pull");
    }
}
