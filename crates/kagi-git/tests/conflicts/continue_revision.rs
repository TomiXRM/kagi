//! #1721: an older Result cannot authorize writes after another handle advances.
use super::*;
use kagi_git::Backend;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PhysicalState {
    head: String,
    rebase_head: Option<String>,
    index: Vec<u8>,
    stages: String,
    worktree: Vec<u8>,
    committed: String,
    operation: git2::RepositoryState,
}

pub(super) fn physical_state(path: &Path) -> PhysicalState {
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
    let expected = old_backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let mut old_buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    old_buffer
        .set_manual_text(Path::new("file.txt"), "RESOLVED ONE\n")
        .unwrap();
    let first = physical_state(path);
    assert!(old_backend
        .plan_conflict_continue_route(&old_session, &old_buffer, "side", &expected)
        .is_ok());
    Backend::open_with_policy(path, policy())
        .unwrap()
        .execute_conflict_continue(&old_session, &old_buffer, &expected)
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
        .plan_conflict_continue(&old_session, &old_buffer, &expected)
        .is_err());
    assert!(old_backend
        .stage_conflict_resolution(&old_session, &old_buffer, &expected)
        .is_err());
    assert!(old_backend
        .execute_conflict_save(&old_buffer, Path::new("file.txt"))
        .is_err());
    assert_eq!(physical_state(path), before);
    let plan =
        old_backend.plan_conflict_continue_route(&old_session, &old_buffer, "side", &expected);
    let execution = old_backend.execute_conflict_continue(&old_session, &old_buffer, &expected);
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
            .stage_conflict_resolution(&old.session, &buffer, &old.observation.revision)
            .is_err());
        assert!(backend
            .execute_conflict_continue(&old.session, &buffer, &old.observation.revision)
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

pub(super) fn open_backend(path: &Path) -> Backend {
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
    let original_approval = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "MY RESOLUTION\n")
        .unwrap();
    write_file(path, "unrelated.txt", "independently staged\n");
    git(path, &["add", "unrelated.txt"]);
    assert!(backend
        .plan_conflict_continue_route(&session, &buffer, "main", &original_approval)
        .is_err());
    assert!(backend
        .continue_blockers(&session, &buffer)
        .unwrap()
        .is_empty());
    // The same file draft survives an unrelated accepted read, but Continue
    // requires a new explicit plan against the newly observed staged set.
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    assert!(backend
        .plan_conflict_continue_route(&session, &buffer, "main", &expected)
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
        .stage_conflict_resolution(&session, &buffer, &expected)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&session, &buffer, &expected)
        .is_err());
    assert!(backend
        .execute_conflict_save(&buffer, Path::new("file.txt"))
        .is_err());
    assert_eq!(physical_state(path), before);

    let fresh = Repository::open(path).unwrap();
    let session = detect_conflict_session(&fresh).unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    assert!(session.files.is_empty());
    let buffer = ResolutionBuffer::from_repo(&fresh).unwrap();
    backend
        .execute_conflict_continue(&session, &buffer, &expected)
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
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let file = Path::new("file.txt");
    assert!(buffer.file_identity(file).is_none());
    buffer
        .set_manual_text(file, "LIVE BUT UNVERIFIED\n")
        .unwrap();
    let before = physical_state(path);
    assert!(backend.execute_conflict_save(&buffer, file).is_err());
    assert!(backend
        .stage_conflict_resolution(&session, &buffer, &expected)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&session, &buffer, &expected)
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
    buffer
        .set_manual_text(Path::new("file.txt"), "UNIQUE SAVED TREE 1721\n")
        .unwrap();
    backend
        .execute_conflict_save(&buffer, Path::new("file.txt"))
        .unwrap();
    assert_eq!(git_output(path, &["ls-files", "-u"]), "");
    std::fs::write(path.join(".git/MERGE_HEAD"), "not a commit identity\n").unwrap();
    let fresh = Repository::open(path).unwrap();
    let session = detect_conflict_session(&fresh).unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    assert!(session.files.is_empty());
    let buffer = ResolutionBuffer::from_repo(&fresh).unwrap();
    let before = physical_state(path);
    let objects_before = git_output(path, &["count-objects", "-v"]);
    let blockers = backend.continue_blockers(&session, &buffer);
    let plan = backend.plan_conflict_continue_route(&session, &buffer, "main", &expected);
    let staging = backend.stage_conflict_resolution(&session, &buffer, &expected);
    let execution = backend.execute_conflict_continue(&session, &buffer, &expected);
    let after = physical_state(path);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    assert!(
        blockers.is_err()
            && plan.is_err()
            && staging.is_err()
            && execution.is_err()
            && before == after
            && objects_before == objects_after,
        "an empty session still needs verified operation identity before writes; \
         checklist_admitted={} plan_admitted={} stage_admitted={} execute_admitted={} \
         execute_error={:?}\nobjects_before={objects_before}\nobjects_after={objects_after}\n\
         before={before:#?}\nafter={after:#?}",
        blockers.is_ok(),
        plan.is_ok(),
        staging.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
}

fn replace_saved_rebase_onto_another_base(path: &Path) {
    git(path, &["rebase", "--abort"]);
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "file.txt", "DIFFERENT REBASE BASE\n");
    git(path, &["commit", "-qam", "replacement rebase base"]);
    git(path, &["checkout", "-q", "side"]);
    assert!(!git_allow_fail(path, &["rebase", "main"]));
    assert!(!git_output(path, &["ls-files", "-u"]).is_empty());
    write_file(path, "file.txt", "REPLACEMENT RESOLUTION\n");
    git(path, &["add", "file.txt"]);
    assert_eq!(git_output(path, &["ls-files", "-u"]), "");
}

#[test]
fn empty_saved_session_refuses_replacement_rebase_with_same_commit_and_step() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "ORIGINAL APPROVED RESOLUTION\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    let original = open_backend(path).conflict_snapshot().unwrap().unwrap();
    let expected = original.observation.revision.clone();
    assert!(original.session.files.is_empty());
    let approved_buffer = open_backend(path).resolution_buffer_from_repo().unwrap();
    let approved_route = backend
        .plan_conflict_continue_route(&original.session, &approved_buffer, "side", &expected)
        .unwrap();
    let original_state = physical_state(path);

    // An independent Git CLI replaces the operation after approval. Neither
    // the original session, buffer nor plan is refreshed from the new rebase.
    replace_saved_rebase_onto_another_base(path);
    let replacement = open_backend(path).conflict_snapshot().unwrap().unwrap();
    assert!(replacement.session.files.is_empty());
    assert_eq!(original.session.op, replacement.session.op);
    assert_ne!(
        original.observation.revision,
        replacement.observation.revision
    );
    let before = physical_state(path);
    assert_ne!(original_state.head, before.head);
    assert_eq!(original_state.rebase_head, before.rebase_head);
    let objects_before = git_output(path, &["count-objects", "-v"]);

    let blockers = backend.continue_blockers(&original.session, &approved_buffer);
    let plan = backend.plan_conflict_continue_route(
        &original.session,
        &approved_buffer,
        "side",
        &expected,
    );
    let staging = backend.stage_conflict_resolution(&original.session, &approved_buffer, &expected);
    let execution =
        backend.execute_conflict_continue(&original.session, &approved_buffer, &expected);
    let after = physical_state(path);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    assert!(
        blockers.is_err()
            && plan.is_err()
            && staging.is_err()
            && execution.is_err()
            && before == after
            && objects_before == objects_after,
        "old empty approval must not advance a replacement operation; \
         original_op={:?} replacement_op={:?} approved_route={approved_route:?} \
         original_revision={:?} replacement_revision={:?} \
         checklist_admitted={} plan_admitted={} stage_admitted={} execute_admitted={} \
         execute_error={:?}\nobjects_before={objects_before}\nobjects_after={objects_after}\n\
         original={original_state:#?}\nbefore={before:#?}\nafter={after:#?}",
        original.session.op,
        replacement.session.op,
        original.observation.revision,
        replacement.observation.revision,
        blockers.is_ok(),
        plan.is_ok(),
        staging.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
}

#[test]
fn empty_saved_approval_refuses_staged_only_replacement_until_explicit_replan() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "FIRST SAVED RESULT\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    let original = backend.conflict_snapshot().unwrap().unwrap();
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(original.session.files.is_empty());
    backend
        .plan_conflict_continue_route(
            &original.session,
            &buffer,
            "side",
            &original.observation.revision,
        )
        .unwrap();

    // Keep the operation intact: only the stage-zero blob changes externally.
    write_file(path, "file.txt", "OTHER SAVED RESULT\n");
    git(path, &["add", "file.txt"]);
    let current = backend.conflict_snapshot().unwrap().unwrap();
    let independent = open_backend(path).conflict_snapshot().unwrap().unwrap();
    assert_eq!(current.observation, independent.observation);
    assert_eq!(current.session.op, original.session.op);
    assert_ne!(current.observation.revision, original.observation.revision);
    assert!(backend
        .continue_blockers(&original.session, &buffer)
        .unwrap()
        .is_empty());
    let before = physical_state(path);
    let objects = git_output(path, &["count-objects", "-v"]);
    assert!(backend
        .plan_conflict_continue(&original.session, &buffer, &original.observation.revision,)
        .is_err());
    assert!(backend
        .plan_conflict_continue_route(
            &original.session,
            &buffer,
            "side",
            &original.observation.revision,
        )
        .is_err());
    assert!(backend
        .stage_conflict_resolution(&original.session, &buffer, &original.observation.revision,)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&original.session, &buffer, &original.observation.revision,)
        .is_err());
    assert_eq!(physical_state(path), before);
    assert_eq!(git_output(path, &["count-objects", "-v"]), objects);

    // A new explicit approval may continue the same operation with its new tree.
    backend
        .plan_conflict_continue_route(
            &current.session,
            &buffer,
            "side",
            &current.observation.revision,
        )
        .unwrap();
    backend
        .execute_conflict_continue(&current.session, &buffer, &current.observation.revision)
        .unwrap();
    assert_eq!(
        git_output(path, &["show", "HEAD:file.txt"]),
        "OTHER SAVED RESULT"
    );
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

fn copy_fixture_tree(source: &Path, target: &Path) {
    std::fs::create_dir(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture_tree(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

#[test]
fn empty_live_buffer_cannot_authorize_identical_repository_at_another_root() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let source = open_backend(path);
    let mut resolving = source.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "SAVED FOR THIS ROOT\n")
        .unwrap();
    source
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    let original = source.conflict_snapshot().unwrap().unwrap();
    let buffer = source.resolution_buffer_from_repo().unwrap();
    assert!(original.session.files.is_empty());
    let destination = tempfile::tempdir().unwrap();
    let copy = destination.path().join("copy");
    copy_fixture_tree(path, &copy);
    let other = open_backend(&copy);
    let current = other.conflict_snapshot().unwrap().unwrap();
    assert_eq!(original.observation, current.observation);
    let before = physical_state(&copy);
    let objects = git_output(&copy, &["count-objects", "-v"]);
    assert!(other.continue_blockers(&current.session, &buffer).is_err());
    assert!(other
        .plan_conflict_continue_route(
            &current.session,
            &buffer,
            "side",
            &current.observation.revision,
        )
        .is_err());
    assert!(other
        .stage_conflict_resolution(&current.session, &buffer, &current.observation.revision,)
        .is_err());
    assert!(other
        .execute_conflict_continue(&current.session, &buffer, &current.observation.revision,)
        .is_err());
    assert_eq!(physical_state(&copy), before);
    assert_eq!(git_output(&copy, &["count-objects", "-v"]), objects);
    assert!(source
        .continue_blockers(&original.session, &buffer)
        .unwrap()
        .is_empty());
}

#[test]
fn empty_buffer_clone_keeps_frozen_context_but_decode_and_new_never_gain_authority() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "SAVED ORIGINAL\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    let original = backend.conflict_snapshot().unwrap().unwrap();
    let live = backend.resolution_buffer_from_repo().unwrap();
    let mut cloned = live.clone();
    assert!(backend
        .continue_blockers(&original.session, &cloned)
        .unwrap()
        .is_empty());
    live.autosave().unwrap();
    let mut decoded = ResolutionBuffer::load(path).unwrap();
    let mut new = ResolutionBuffer::new(path);
    decoded.retain_drafts(live.clone());
    new.retain_drafts(live.clone());
    let before = physical_state(path);
    for unauthorized in [&decoded, &new] {
        assert!(backend
            .continue_blockers(&original.session, unauthorized)
            .is_err());
        assert!(backend
            .stage_conflict_resolution(
                &original.session,
                unauthorized,
                &original.observation.revision,
            )
            .is_err());
        assert!(backend
            .execute_conflict_continue(
                &original.session,
                unauthorized,
                &original.observation.revision,
            )
            .is_err());
    }
    assert_eq!(physical_state(path), before);

    replace_saved_rebase_onto_another_base(path);
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(current.session.op, original.session.op);
    // Retaining a newly read buffer must not overwrite the receiving clone's
    // original operation provenance, even though both buffers have zero files.
    cloned.retain_drafts(backend.resolution_buffer_from_repo().unwrap());
    let before = physical_state(path);
    assert!(backend
        .continue_blockers(&current.session, &cloned)
        .is_err());
    assert!(backend
        .execute_conflict_continue(&current.session, &cloned, &current.observation.revision,)
        .is_err());
    assert_eq!(physical_state(path), before);
}
