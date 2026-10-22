//! #1721: an older Result cannot authorize writes after another handle advances.
use super::*;
use kagi_git::Backend;

#[derive(Debug, PartialEq, Eq)]
struct PhysicalState {
    head: String,
    rebase_head: Option<String>,
    index: Vec<u8>,
    stages: String,
    worktree: Vec<u8>,
    committed: String,
    operation: git2::RepositoryState,
}

fn physical_state(path: &Path) -> PhysicalState {
    physical_file_state(path, "file.txt")
}

fn physical_file_state(path: &Path, file: &str) -> PhysicalState {
    PhysicalState {
        head: git_output(path, &["rev-parse", "HEAD"]),
        rebase_head: std::fs::read_to_string(path.join(".git/REBASE_HEAD")).ok(),
        index: std::fs::read(path.join(".git/index")).unwrap(),
        stages: git_output(path, &["ls-files", "--stage"]),
        worktree: std::fs::read(path.join(file)).unwrap(),
        committed: git_output(path, &["show", &format!("HEAD:{file}")]),
        operation: Repository::open(path).unwrap().state(),
    }
}

#[test]
fn public_continue_refuses_old_result_after_external_same_path_advance() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_two_conflicting_steps_repo();
    let path = fixture.path();
    let policy = || kagi_git::backend::ExecutionPolicy::human(false);
    let old_backend = Backend::open_with_policy(path, policy()).unwrap();
    let repo = Repository::open(path).unwrap();
    let old_session = detect_conflict_session(&repo).unwrap();
    let mut old_buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    old_buffer
        .set_manual_text(Path::new("file.txt"), "RESOLVED ONE\n")
        .unwrap();
    let first = physical_state(path);
    assert!(old_backend
        .plan_conflict_continue_route(&old_session, &old_buffer, "side")
        .is_ok());
    Backend::open_with_policy(path, policy())
        .unwrap()
        .execute_conflict_continue(&old_session, &old_buffer)
        .unwrap();
    let before = physical_state(path);
    assert_ne!(before.head, first.head);
    assert_ne!(before.rebase_head, first.rebase_head);
    assert_eq!(before.committed, "RESOLVED ONE");
    assert!(!git_output(path, &["ls-files", "-u"]).is_empty());
    assert!(String::from_utf8_lossy(&before.worktree).contains("SIDE TWO"));

    assert!(old_backend
        .continue_blockers(&old_session, &old_buffer)
        .is_err());
    assert!(old_backend
        .plan_conflict_continue(&old_session, &old_buffer)
        .is_err());
    assert!(old_backend
        .stage_conflict_resolution(&old_session, &old_buffer)
        .is_err());
    assert!(old_backend
        .execute_conflict_save(&old_buffer, Path::new("file.txt"))
        .is_err());
    assert_eq!(physical_state(path), before);
    let plan = old_backend.plan_conflict_continue_route(&old_session, &old_buffer, "side");
    let execution = old_backend.execute_conflict_continue(&old_session, &old_buffer);
    let after = physical_state(path);
    assert!(
        plan.is_err() && execution.is_err() && before == after,
        "old Result must be refused without mutation after external advance; \
         plan_admitted={} execution_admitted={} execution_error={:?}\nbefore={before:#?}\nafter={after:#?}",
        plan.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
}

fn replace_incoming_stage(repo: &Repository, raw: bool) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    let mut conflict = index.conflicts().unwrap().next().unwrap().unwrap();
    let incoming = conflict.their.as_mut().unwrap();
    if raw {
        incoming.id = repo.blob(&[0, 8, 6, 4, 0, 2]).unwrap();
    } else {
        incoming.mode = 0o100755;
    }
    let oid = incoming.id;
    draft_revision::write_index_conflict(&mut index, &conflict);
    index.write().unwrap();
    oid
}

#[test]
fn same_operation_stage_change_refuses_text_and_raw_old_results() {
    if !test_support::run_isolated() {
        return;
    }
    for raw in [false, true] {
        let fixture = if raw {
            binary_merge_conflict_repo()
        } else {
            merge_conflict_repo()
        };
        let path = fixture.path();
        let file = if raw { "blob.bin" } else { "file.txt" };
        let rel = Path::new(file);
        let repo = Repository::open(path).unwrap();
        let backend = open_backend(path);
        let old = backend.conflict_snapshot().unwrap().unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .apply_choice(rel, ResolutionChoice::Incoming)
            .unwrap();
        let request = current_save_request(path, &buffer, rel);
        let prepared = Backend::plan_recorded_conflict(path, request).unwrap();
        let expected_oid = replace_incoming_stage(&repo, raw);
        let current = Backend::open(path)
            .unwrap()
            .conflict_snapshot()
            .unwrap()
            .unwrap();
        assert_eq!(current.session.op, old.session.op);
        let before = physical_file_state(path, file);
        assert!(backend.continue_blockers(&old.session, &buffer).is_err());
        assert!(backend.execute_conflict_save(&buffer, rel).is_err());
        assert!(backend
            .stage_conflict_resolution(&old.session, &buffer)
            .is_err());
        assert!(backend
            .execute_conflict_continue(&old.session, &buffer)
            .is_err());
        assert_eq!(physical_file_state(path, file), before);

        // New observation must not give old bytes a new file identity.
        let rebound = current_save_request(path, &buffer, rel);
        assert!(Backend::plan_recorded_conflict(path, rebound).is_err());
        let refused = Backend::run_recorded_conflict(
            &prepared,
            kagi_git::backend::ExecutionPolicy::human(false),
            None,
        );
        assert_eq!(
            refused.evidence.progress,
            kagi_domain::conflict_family::ConflictProgress::NotStarted
        );
        assert_eq!(physical_file_state(path, file), before);

        assert_fresh_recorded_save(path, rel, expected_oid, &before.head);
    }
}

fn open_backend(path: &Path) -> Backend {
    Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false)).unwrap()
}

fn current_save_request(
    path: &Path,
    buffer: &ResolutionBuffer,
    file: &Path,
) -> kagi_domain::conflict_family::ConflictRequest {
    let snapshot = Backend::open(path)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .unwrap();
    Backend::conflict_save_request(
        snapshot.observation.revision,
        buffer,
        file,
        snapshot.observation.kind,
        "",
    )
    .unwrap()
}

fn assert_fresh_recorded_save(path: &Path, file: &Path, expected_oid: git2::Oid, head: &str) {
    let backend = Backend::open(path).unwrap();
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer
        .apply_choice(file, ResolutionChoice::Incoming)
        .unwrap();
    let plan =
        Backend::plan_recorded_conflict(path, current_save_request(path, &buffer, file)).unwrap();
    let report = Backend::run_recorded_conflict(
        &plan,
        kagi_git::backend::ExecutionPolicy::human(false),
        None,
    );
    assert_eq!(
        report.evidence.progress,
        kagi_domain::conflict_family::ConflictProgress::Verified
    );
    assert_eq!(git_output(path, &["ls-files", "-u"]), "");
    assert_eq!(
        Repository::open(path)
            .unwrap()
            .index()
            .unwrap()
            .get_path(file, 0)
            .unwrap()
            .id,
        expected_oid
    );
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
}

#[test]
fn unrelated_index_change_is_allowed_but_old_session_cannot_replace_saved_result() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = merge_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let repo = Repository::open(path).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "MY RESOLUTION\n")
        .unwrap();
    write_file(path, "unrelated.txt", "independently staged\n");
    git(path, &["add", "unrelated.txt"]);
    assert!(backend
        .plan_conflict_continue_route(&session, &buffer, "main")
        .is_ok());
    backend
        .execute_conflict_save(&buffer, Path::new("file.txt"))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("file.txt")).unwrap(),
        "MY RESOLUTION\n"
    );
    assert!(Repository::open(path)
        .unwrap()
        .index()
        .unwrap()
        .get_path(Path::new("unrelated.txt"), 0)
        .is_some());

    write_file(path, "file.txt", "EXTERNAL SAVED RESULT\n");
    git(path, &["add", "file.txt"]);
    let before = physical_state(path);
    assert!(backend
        .stage_conflict_resolution(&session, &buffer)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&session, &buffer)
        .is_err());
    assert!(backend
        .execute_conflict_save(&buffer, Path::new("file.txt"))
        .is_err());
    assert_eq!(physical_state(path), before);

    let fresh = Repository::open(path).unwrap();
    let session = detect_conflict_session(&fresh).unwrap();
    assert!(session.files.is_empty());
    let buffer = ResolutionBuffer::from_repo(&fresh).unwrap();
    backend
        .execute_conflict_continue(&session, &buffer)
        .unwrap();
    assert_eq!(
        git_output(path, &["show", "HEAD:file.txt"]),
        "EXTERNAL SAVED RESULT"
    );
    assert_eq!(git_output(path, &["status", "--porcelain"]), "");
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn unknown_current_identity_cannot_authorize_save_or_continue() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = merge_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    std::fs::write(path.join(".git/MERGE_HEAD"), "not a commit identity\n").unwrap();
    let repo = Repository::open(path).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let file = Path::new("file.txt");
    assert!(buffer.file_identity(file).is_none());
    buffer
        .set_manual_text(file, "LIVE BUT UNVERIFIED\n")
        .unwrap();
    let before = physical_state(path);
    assert!(backend.execute_conflict_save(&buffer, file).is_err());
    assert!(backend
        .stage_conflict_resolution(&session, &buffer)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&session, &buffer)
        .is_err());
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    assert!(Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        file,
        snapshot.observation.kind,
        ""
    )
    .is_err());
    assert_eq!(physical_state(path), before);
    assert_eq!(
        buffer.resolved_text(file).as_deref(),
        Some("LIVE BUT UNVERIFIED\n")
    );
}

#[test]
fn empty_session_requires_known_operation_before_any_continue_write() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = merge_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let repo = Repository::open(path).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.set_manual_text(Path::new("file.txt"), "UNIQUE SAVED TREE 1721\n").unwrap();
    backend.execute_conflict_save(&buffer, Path::new("file.txt")).unwrap();
    assert_eq!(git_output(path, &["ls-files", "-u"]), "");
    std::fs::write(path.join(".git/MERGE_HEAD"), "not a commit identity\n").unwrap();
    let fresh = Repository::open(path).unwrap();
    let session = detect_conflict_session(&fresh).unwrap();
    assert!(session.files.is_empty());
    let buffer = ResolutionBuffer::from_repo(&fresh).unwrap();
    let before = physical_state(path);
    let objects_before = git_output(path, &["count-objects", "-v"]);
    let blockers = backend.continue_blockers(&session, &buffer);
    let plan = backend.plan_conflict_continue_route(&session, &buffer, "main");
    let staging = backend.stage_conflict_resolution(&session, &buffer);
    let execution = backend.execute_conflict_continue(&session, &buffer);
    let after = physical_state(path);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    assert!(
        blockers.is_err() && plan.is_err() && staging.is_err() && execution.is_err()
            && before == after && objects_before == objects_after,
        "an empty session still needs verified operation identity before writes; \
         checklist_admitted={} plan_admitted={} stage_admitted={} execute_admitted={} \
         execute_error={:?}\nobjects_before={objects_before}\nobjects_after={objects_after}\n\
         before={before:#?}\nafter={after:#?}",
        blockers.is_ok(), plan.is_ok(), staging.is_ok(), execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
}
