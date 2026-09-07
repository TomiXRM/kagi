//! Conflict C1: window-free public application lifecycle for Save and D/F.
use kagi::app::*;
use kagi_domain::conflict_family::{ConflictDraft, ConflictProgress, ConflictRequest};
use kagi_domain::resolution::{DirFileChoice, ResolutionChoice};
use kagi_git::backend::conflict_ops::ConflictFaultPoint;
use kagi_git::backend::ExecutionPolicy;
use kagi_git::{Backend, OpOutcome};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static ENV: Mutex<()> = Mutex::new(());

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    _root: tempfile::TempDir,
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

fn git(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git")
        .success()
}

impl Fixture {
    fn content() -> Self {
        let lock = ENV.lock().unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        assert!(git(&repo, &["init", "-q", "-b", "main"]));
        assert!(git(&repo, &["config", "commit.gpgsign", "false"]));
        std::fs::write(repo.join("file.txt"), "base\n").unwrap();
        assert!(git(&repo, &["add", "."]));
        assert!(git(&repo, &["commit", "-qm", "base"]));
        assert!(git(&repo, &["checkout", "-qb", "feature"]));
        std::fs::write(repo.join("file.txt"), "feature\n").unwrap();
        assert!(git(&repo, &["commit", "-qam", "feature"]));
        assert!(git(&repo, &["checkout", "-q", "main"]));
        std::fs::write(repo.join("file.txt"), "main\n").unwrap();
        assert!(git(&repo, &["commit", "-qam", "main"]));
        assert!(!git(&repo, &["merge", "feature"]));
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", root.path().join("log"));
        Self {
            _lock: lock,
            _root: root,
            repo,
            old_log,
        }
    }

    fn dir_file() -> Self {
        let lock = ENV.lock().unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        assert!(git(&repo, &["init", "-q", "-b", "main"]));
        assert!(git(&repo, &["config", "commit.gpgsign", "false"]));
        std::fs::write(repo.join("base"), "base\n").unwrap();
        assert!(git(&repo, &["add", "."]));
        assert!(git(&repo, &["commit", "-qm", "base"]));
        assert!(git(&repo, &["checkout", "-qb", "file-side"]));
        std::fs::write(repo.join("thing"), "file side\n").unwrap();
        assert!(git(&repo, &["add", "thing"]));
        assert!(git(&repo, &["commit", "-qm", "file"]));
        assert!(git(&repo, &["checkout", "-q", "main"]));
        assert!(git(&repo, &["checkout", "-qb", "dir-side"]));
        std::fs::create_dir(repo.join("thing")).unwrap();
        std::fs::write(repo.join("thing/child"), "directory side\n").unwrap();
        assert!(git(&repo, &["add", "thing/child"]));
        assert!(git(&repo, &["commit", "-qm", "directory"]));
        assert!(git(&repo, &["checkout", "-q", "file-side"]));
        let repository = git2::Repository::open(&repo).unwrap();
        let other = repository
            .revparse_single("dir-side")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        let annotated = repository.find_annotated_commit(other.id()).unwrap();
        repository.merge(&[&annotated], None, None).unwrap();
        drop(annotated);
        drop(other);
        drop(repository);
        let old_log = std::env::var_os("KAGI_LOG_DIR");
        std::env::set_var("KAGI_LOG_DIR", root.path().join("log"));
        Self {
            _lock: lock,
            _root: root,
            repo,
            old_log,
        }
    }

    fn owner_and_snapshot(
        &self,
        sessions: &mut Sessions,
    ) -> (SessionId, kagi_git::backend::conflict_ops::ConflictSnapshot) {
        let owner = sessions.attach(self.repo.clone());
        let snapshot = Backend::open(&self.repo)
            .unwrap()
            .conflict_snapshot()
            .unwrap()
            .expect("conflict");
        sessions.observe_conflict(owner, Some(snapshot.observation.clone()));
        (owner, snapshot)
    }

    fn job(&self, sessions: &mut Sessions, request: ConflictRequest) -> ConflictJob {
        let owner = sessions
            .attachment(
                sessions.sessions_for(
                    &Backend::open(&self.repo)
                        .unwrap()
                        .write_worktree_id()
                        .unwrap(),
                )[0],
            )
            .unwrap();
        let policy = ExecutionPolicy::human(false);
        let plan = plan_conflict(sessions, ConflictAppRequest { owner, request }, policy);
        assert!(apply_plan(sessions, plan.run()));
        let PlanState::Ready { token, .. } = sessions.plan_state() else {
            panic!("expected Ready, got {:?}", sessions.plan_state());
        };
        let approved = approve(sessions, token.clone(), Policy::Conflict(policy)).unwrap();
        prepare_conflict(sessions, approved, LegacyBusy(false)).unwrap()
    }
}

fn save_request(fixture: &Fixture, sessions: &mut Sessions) -> ConflictRequest {
    let (_owner, snapshot) = fixture.owner_and_snapshot(sessions);
    let backend = Backend::open(&fixture.repo).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
        .unwrap();
    Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("file.txt"),
        snapshot.observation.operation.as_str(),
        "",
    )
    .unwrap()
}

#[test]
fn save_runs_once_records_once_and_applies_verified_observation() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let job = fixture.job(&mut sessions, request);
    let operation = job.id();
    assert!(sessions.has_leases());
    assert!(matches!(
        sessions.write_lease(&fixture.repo, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    let completion = job.run();
    assert_eq!(
        completion.report().evidence.progress,
        ConflictProgress::Verified
    );
    assert!(matches!(
        completion.report().recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    let first = sessions.apply(completion.clone());
    assert!(!sessions.has_leases());
    assert_eq!(kagi_git::oplog::read_oplog_tail(10).len(), 1);
    assert!(first.iter().any(|delivery| matches!(
        delivery,
        Delivery::Completed { id, .. } if *id == operation
    )));
    assert!(
        sessions.apply(completion).is_empty(),
        "duplicate completion"
    );
    assert_eq!(kagi_git::oplog::read_oplog_tail(10).len(), 1);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("file.txt")).unwrap(),
        "main\n"
    );
}

#[test]
fn changed_conflict_and_changed_buffer_are_refused_without_mutation() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let before = std::fs::read(fixture.repo.join("file.txt")).unwrap();
    let tampered = match request.clone() {
        ConflictRequest::Save {
            path,
            revision,
            buffer_revision,
            operation,
            before_hash,
            actions,
            ..
        } => ConflictRequest::Save {
            path,
            revision,
            buffer_revision,
            draft: ConflictDraft::Text(b"different\n".to_vec()),
            operation,
            before_hash,
            actions,
        },
        _ => unreachable!(),
    };
    let owner = sessions
        .attachment(
            sessions.sessions_for(
                &Backend::open(&fixture.repo)
                    .unwrap()
                    .write_worktree_id()
                    .unwrap(),
            )[0],
        )
        .unwrap();
    let plan = plan_conflict(
        &mut sessions,
        ConflictAppRequest {
            owner,
            request: tampered,
        },
        ExecutionPolicy::human(false),
    );
    assert!(apply_plan(&mut sessions, plan.run()));
    assert!(matches!(sessions.plan_state(), PlanState::Error { .. }));
    assert_eq!(
        std::fs::read(fixture.repo.join("file.txt")).unwrap(),
        before
    );

    assert!(git(&fixture.repo, &["add", "file.txt"]));
    assert!(Backend::plan_recorded_conflict(&fixture.repo, request).is_err());
}

#[test]
fn marker_draft_refusal_preserves_the_session_suffix_in_the_recording() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let (owner, snapshot) = fixture.owner_and_snapshot(&mut sessions);
    let backend = Backend::open(&fixture.repo).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    let markers = backend
        .materialized_markers(&buffer, Path::new("file.txt"))
        .expect("text conflict markers");
    assert!(buffer.ensure_hunks(Path::new("file.txt"), &markers));
    assert!(buffer.reset_hunk(Path::new("file.txt"), 0));
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("file.txt"),
        snapshot.observation.operation.as_str(),
        "",
    )
    .unwrap();
    assert!(matches!(
        &request,
        ConflictRequest::Save {
            draft: ConflictDraft::Text(bytes),
            ..
        } if bytes.windows(b"<<<<<<<".len()).any(|window| window == b"<<<<<<<")
    ));
    let owner = sessions.attachment(owner).unwrap();
    let completion = plan_conflict(
        &mut sessions,
        ConflictAppRequest { owner, request },
        ExecutionPolicy::human(false),
    )
    .run();
    assert!(apply_plan(&mut sessions, completion));
    let PlanState::Error {
        recording: Some(recording),
        ..
    } = sessions.plan_state()
    else {
        panic!("marker draft must be refused during planning")
    };
    assert_eq!(recording.entry().op, "conflict-save:merge");
}

#[test]
fn legacy_busy_refuses_approval_and_partial_save_retains_recovery_evidence() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let owner = sessions
        .attachment(
            sessions.sessions_for(
                &Backend::open(&fixture.repo)
                    .unwrap()
                    .write_worktree_id()
                    .unwrap(),
            )[0],
        )
        .unwrap();
    let policy = ExecutionPolicy::human(false);
    let plan = plan_conflict(&mut sessions, ConflictAppRequest { owner, request }, policy);
    assert!(apply_plan(&mut sessions, plan.run()));
    let PlanState::Ready { token, .. } = sessions.plan_state() else {
        panic!()
    };
    let token = token.clone();
    let approved = approve(&mut sessions, token, Policy::Conflict(policy)).unwrap();
    assert!(matches!(
        prepare_conflict(&mut sessions, approved, LegacyBusy(true)),
        Err(AdmissionError::Busy)
    ));

    sessions.invalidate_plan();
    let request = save_request(&fixture, &mut sessions);
    let job = fixture
        .job(&mut sessions, request)
        .with_fault_for_test(ConflictFaultPoint::AfterWorktreeWrite);
    let completion = job.run();
    assert_eq!(
        completion.report().evidence.progress,
        ConflictProgress::WorktreeWritten
    );
    assert!(matches!(
        completion.report().recording.entry().outcome,
        OpOutcome::Partial { .. }
    ));
    sessions.apply(completion);
    assert!(!sessions.has_leases());
}

#[test]
fn dir_file_choices_use_the_same_recorded_boundary() {
    for choice in [DirFileChoice::KeepDirectory, DirFileChoice::KeepFile] {
        let fixture = Fixture::dir_file();
        let mut sessions = Sessions::new();
        let (_owner, snapshot) = fixture.owner_and_snapshot(&mut sessions);
        let request = ConflictRequest::ResolveDirFile {
            path: PathBuf::from("thing"),
            revision: snapshot.observation.revision,
            choice,
        };
        let completion = fixture.job(&mut sessions, request).run();
        assert_eq!(
            completion.report().evidence.progress,
            ConflictProgress::Verified
        );
        assert!(matches!(
            completion.report().recording.entry().outcome,
            OpOutcome::Success { .. }
        ));
        sessions.apply(completion);
        assert_eq!(kagi_git::oplog::read_oplog_tail(10).len(), 1);
        let backend = Backend::open(&fixture.repo).unwrap();
        assert!(backend.conflict_snapshot().unwrap().is_some());
    }
}

#[test]
fn double_approval_is_one_shot_and_old_owner_never_receives_reopened_delivery() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let old_owner = sessions.sessions_for(
        &Backend::open(&fixture.repo)
            .unwrap()
            .write_worktree_id()
            .unwrap(),
    )[0];
    let owner = sessions.attachment(old_owner).unwrap();
    let policy = ExecutionPolicy::human(false);
    let plan = plan_conflict(&mut sessions, ConflictAppRequest { owner, request }, policy);
    assert!(apply_plan(&mut sessions, plan.run()));
    let PlanState::Ready { token, .. } = sessions.plan_state() else {
        panic!()
    };
    let token = token.clone();
    let approved = approve(&mut sessions, token.clone(), Policy::Conflict(policy)).unwrap();
    assert!(matches!(
        approve(&mut sessions, token, Policy::Conflict(policy)),
        Err(AdmissionError::StaleApproval)
    ));
    let job = prepare_conflict(&mut sessions, approved, LegacyBusy(false)).unwrap();
    sessions.detach(old_owner);
    let reopened = sessions.attach(fixture.repo.clone());
    assert_ne!(old_owner, reopened);
    let deliveries = sessions.apply(job.run());
    assert!(sessions.conflict_state(reopened).is_none());
    assert!(deliveries.iter().any(|delivery| matches!(
        delivery,
        Delivery::Completed { attachment, .. } if attachment.session == old_owner
    )));
    assert!(!deliveries.iter().any(|delivery| matches!(
        delivery,
        Delivery::Completed { attachment, .. } if attachment.session == reopened
    )));
}

#[test]
fn existing_shared_writer_blocks_conflict_before_dispatch() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let worktree = Backend::open(&fixture.repo)
        .unwrap()
        .write_worktree_id()
        .unwrap();
    let owner = sessions
        .attachment(sessions.sessions_for(&worktree)[0])
        .unwrap();
    let guard = sessions
        .write_lease(&fixture.repo, LegacyBusy(false))
        .unwrap();
    let policy = ExecutionPolicy::human(false);
    let plan = plan_conflict(&mut sessions, ConflictAppRequest { owner, request }, policy);
    assert!(apply_plan(&mut sessions, plan.run()));
    let PlanState::Ready { token, .. } = sessions.plan_state() else {
        panic!()
    };
    let token = token.clone();
    let approved = approve(&mut sessions, token, Policy::Conflict(policy)).unwrap();
    assert!(matches!(
        prepare_conflict(&mut sessions, approved, LegacyBusy(false)),
        Err(AdmissionError::Busy)
    ));
    guard.complete();
}

#[test]
fn runtime_index_drift_is_recorded_refusal_and_releases_lease() {
    let fixture = Fixture::dir_file();
    let mut sessions = Sessions::new();
    let (_owner, snapshot) = fixture.owner_and_snapshot(&mut sessions);
    let request = ConflictRequest::ResolveDirFile {
        path: PathBuf::from("thing"),
        revision: snapshot.observation.revision,
        choice: DirFileChoice::KeepFile,
    };
    let job = fixture.job(&mut sessions, request);
    std::fs::write(fixture.repo.join("base"), "changed after approval\n").unwrap();
    assert!(git(&fixture.repo, &["add", "base"]));
    let completion = job.run();
    assert_eq!(
        completion.report().evidence.progress,
        ConflictProgress::NotStarted
    );
    assert!(matches!(
        completion.report().recording.entry().outcome,
        OpOutcome::Refused { .. }
    ));
    sessions.apply(completion);
    assert!(!sessions.has_leases());
    assert!(Backend::open(&fixture.repo)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .is_some());
}

#[test]
fn matching_external_bytes_never_forge_this_attempts_progress() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let job = fixture.job(&mut sessions, request);
    std::fs::write(fixture.repo.join("file.txt"), "main\n").unwrap();
    assert!(git(&fixture.repo, &["add", "file.txt"]));
    let completion = job.run();
    assert_eq!(
        completion.report().evidence.progress,
        ConflictProgress::NotStarted
    );
    assert!(matches!(
        completion.report().recording.entry().outcome,
        OpOutcome::Refused { .. }
    ));
    sessions.apply(completion);
    assert!(!sessions.has_leases());
}

#[test]
fn depart_retains_inflight_owner_and_only_matching_completion_settles_it() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let owner = sessions.sessions_for(
        &Backend::open(&fixture.repo)
            .unwrap()
            .write_worktree_id()
            .unwrap(),
    )[0];
    let job = fixture.job(&mut sessions, request);
    let operation = job.id();
    sessions.depart(owner);
    assert!(matches!(
        sessions.conflict_state(owner),
        Some(ConflictOwnerState::InFlight { operation: current, .. }) if *current == operation
    ));
    sessions.apply(job.run());
    assert!(matches!(
        sessions.conflict_state(owner),
        Some(ConflictOwnerState::Settled(_))
    ));
    sessions.detach(owner);
    assert!(sessions.conflict_state(owner).is_none());
}

#[test]
fn recording_failure_is_reported_without_a_ui_fallback_append() {
    let fixture = Fixture::content();
    let bad_log = fixture.repo.join("not-a-directory");
    std::fs::write(&bad_log, "sentinel").unwrap();
    std::env::set_var("KAGI_LOG_DIR", &bad_log);
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let completion = fixture.job(&mut sessions, request).run();
    assert!(matches!(
        completion.report().recording,
        kagi_git::backend::recording::Recording::Failed { .. }
    ));
    sessions.apply(completion);
    assert_eq!(std::fs::read_to_string(bad_log).unwrap(), "sentinel");
}

#[test]
fn same_path_new_conflict_revision_refuses_the_old_save() {
    let fixture = Fixture::content();
    let mut sessions = Sessions::new();
    let old_request = save_request(&fixture, &mut sessions);
    let old_revision = old_request.revision().clone();
    assert!(git(&fixture.repo, &["merge", "--abort"]));
    assert!(git(
        &fixture.repo,
        &["checkout", "-qb", "feature-two", "HEAD~1"]
    ));
    std::fs::write(fixture.repo.join("file.txt"), "feature two\n").unwrap();
    assert!(git(&fixture.repo, &["commit", "-qam", "feature two"]));
    assert!(git(&fixture.repo, &["checkout", "-q", "main"]));
    assert!(!git(&fixture.repo, &["merge", "feature-two"]));
    let fresh = Backend::open(&fixture.repo)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .expect("second conflict");
    assert_ne!(fresh.observation.revision, old_revision);
    let before = std::fs::read(fixture.repo.join("file.txt")).unwrap();
    let owner = sessions.sessions_for(
        &Backend::open(&fixture.repo)
            .unwrap()
            .write_worktree_id()
            .unwrap(),
    )[0];
    sessions.observe_conflict(owner, Some(fresh.observation));
    let owner = sessions.attachment(owner).unwrap();
    let plan = plan_conflict(
        &mut sessions,
        ConflictAppRequest {
            owner,
            request: old_request,
        },
        ExecutionPolicy::human(false),
    );
    assert!(apply_plan(&mut sessions, plan.run()));
    assert!(matches!(sessions.plan_state(), PlanState::Error { .. }));
    assert_eq!(
        std::fs::read(fixture.repo.join("file.txt")).unwrap(),
        before
    );
}

#[cfg(unix)]
#[test]
fn text_save_preserves_executable_mode_in_stage_zero() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = Fixture::content();
    let path = fixture.repo.join("file.txt");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    let mut sessions = Sessions::new();
    let request = save_request(&fixture, &mut sessions);
    let completion = fixture.job(&mut sessions, request).run();
    sessions.apply(completion);
    let repo = git2::Repository::open(&fixture.repo).unwrap();
    let index = repo.index().unwrap();
    assert_eq!(
        index.get_path(Path::new("file.txt"), 0).unwrap().mode,
        0o100755
    );
}
