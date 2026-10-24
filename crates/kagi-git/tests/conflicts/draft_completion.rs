//! #1721: completion must not depend on a later UI callback to preserve drafts.
use super::draft_revision::recovery_texts;
use super::*;
use kagi_git::{Backend, ContinueOutcome, Operation, OperationOutcome};

fn open_backend(path: &Path) -> Backend {
    Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false)).unwrap()
}

fn assert_inspection_only(buffer: &ResolutionBuffer, expected: &[&str]) {
    let path = Path::new("file.txt");
    assert!(!buffer.has_resolution(path));
    assert_eq!(buffer.resolved_text(path), None);
    let mut actual = recovery_texts(buffer);
    actual.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(buffer
        .recovery_drafts()
        .all(|draft| draft.identity.is_some()));
}

fn assert_unresolved_continue_preserves_repository(
    backend: &Backend,
    dir: &Path,
    session: &kagi_git::ConflictSession,
    buffer: &ResolutionBuffer,
    expected: &kagi_domain::conflict_family::ConflictRevision,
) {
    let index = std::fs::read(dir.join(".git/index")).unwrap();
    let worktree = std::fs::read(dir.join("file.txt")).unwrap();
    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let step = std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap();
    assert!(backend
        .continue_blockers(session, buffer)
        .unwrap()
        .iter()
        .any(|blocker| blocker.code() == "unresolved-files"));
    assert!(backend
        .execute_conflict_continue(session, buffer, expected)
        .is_err());
    assert_eq!(index, std::fs::read(dir.join(".git/index")).unwrap());
    assert_eq!(worktree, std::fs::read(dir.join("file.txt")).unwrap());
    assert_eq!(head, git_output(dir, &["rev-parse", "HEAD"]));
    assert_eq!(step, std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap());
}

#[test]
fn continue_completion_keeps_durable_draft_without_ui_adoption() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = rebase_two_conflicting_steps_repo();
    let dir = tmp.path();
    let path = Path::new("file.txt");
    let backend = open_backend(dir);
    let first_session = backend.detect_conflict_session().unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let first_step = std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap();
    let mut first = backend.resolution_buffer_from_repo().unwrap();
    let first_identity = first.file_identity(path).unwrap().clone();
    first.set_manual_text(path, "RESOLVED ONE\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let first_document = std::fs::read(&saved_path).unwrap();
    let result = backend
        .execute_conflict_continue(&first_session, &first, &expected)
        .unwrap();
    assert!(matches!(result.outcome, ContinueOutcome::Staged));
    assert_ne!(
        first_step,
        std::fs::read(dir.join(".git/REBASE_HEAD")).unwrap()
    );
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "RESOLVED ONE");
    // No adoption, retain_drafts, replacement file, or post-completion autosave:
    // the already durable document must be enough after losing all live owners.
    drop(first);
    drop(backend);
    assert_eq!(first_document, std::fs::read(&saved_path).unwrap());

    let backend = open_backend(dir);
    let second_session = backend.detect_conflict_session().unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let second = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_ne!(second.file_identity(path), Some(&first_identity));
    assert_inspection_only(&second, &["RESOLVED ONE\n"]);
    assert_eq!(
        second.recovery_drafts().next().unwrap().identity,
        Some(&first_identity)
    );
    assert_unresolved_continue_preserves_repository(
        &backend,
        dir,
        &second_session,
        &second,
        &expected,
    );
    assert_eq!(saved_path, second.autosave().unwrap());
    let mut second = open_backend(dir)
        .resolution_buffer_from_repo_with_autosave()
        .unwrap();
    assert_inspection_only(&second, &["RESOLVED ONE\n"]);
    second.set_manual_text(path, "RESOLVED TWO\n").unwrap();
    assert_eq!(saved_path, second.autosave().unwrap());
    let result = backend
        .execute_conflict_continue(&second_session, &second, &expected)
        .unwrap();
    assert!(matches!(result.outcome, ContinueOutcome::Committed(_)));
    drop(second);
    drop(backend);

    let backend = open_backend(dir);
    assert!(backend.detect_conflict_session().is_none());
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "RESOLVED TWO");
    assert_eq!(git_output(dir, &["show", "HEAD^:file.txt"]), "RESOLVED ONE");
    let completed = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_inspection_only(&completed, &["RESOLVED ONE\n", "RESOLVED TWO\n"]);
    assert_eq!(saved_path, completed.autosave().unwrap());
    assert_inspection_only(
        &open_backend(dir)
            .resolution_buffer_from_repo_with_autosave()
            .unwrap(),
        &["RESOLVED ONE\n", "RESOLVED TWO\n"],
    );
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "RESOLVED TWO");
}

#[test]
fn skip_completion_keeps_each_discarded_draft_inspection_only() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = rebase_two_conflicting_steps_repo();
    let dir = tmp.path();
    let path = Path::new("file.txt");
    let backend = open_backend(dir);
    let session = backend.detect_conflict_session().unwrap();
    let mut first = backend.resolution_buffer_from_repo().unwrap();
    let first_identity = first.file_identity(path).unwrap().clone();
    first.set_manual_text(path, "SKIPPED ONE\n").unwrap();
    let saved_path = first.autosave().unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let approved = backend.plan_conflict_skip(&session, &expected).unwrap();
    let result = backend.execute_conflict_skip(&approved, &first).unwrap();
    assert_eq!(result.progress, SkipProgress::Advanced);
    assert_eq!(result.buffer_preserved_at.as_ref(), Some(&saved_path));
    drop(first);
    drop(backend);

    let backend = open_backend(dir);
    let session = backend.detect_conflict_session().unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let mut second = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_ne!(second.file_identity(path), Some(&first_identity));
    assert_inspection_only(&second, &["SKIPPED ONE\n"]);
    assert_unresolved_continue_preserves_repository(&backend, dir, &session, &second, &expected);
    second.set_manual_text(path, "SKIPPED TWO\n").unwrap();
    assert_eq!(saved_path, second.autosave().unwrap());
    let approved = backend.plan_conflict_skip(&session, &expected).unwrap();
    let result = backend.execute_conflict_skip(&approved, &second).unwrap();
    assert_eq!(result.progress, SkipProgress::Finished);
    assert_eq!(result.buffer_preserved_at.as_ref(), Some(&saved_path));
    drop(second);
    drop(backend);

    let backend = open_backend(dir);
    assert!(backend.detect_conflict_session().is_none());
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "MAIN");
    assert_eq!(std::fs::read_to_string(dir.join(path)).unwrap(), "MAIN\n");
    assert_eq!(git_output(dir, &["show", ":file.txt"]), "MAIN");
    let completed = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_inspection_only(&completed, &["SKIPPED ONE\n", "SKIPPED TWO\n"]);
    assert_eq!(saved_path, completed.autosave().unwrap());
    assert_inspection_only(
        &open_backend(dir)
            .resolution_buffer_from_repo_with_autosave()
            .unwrap(),
        &["SKIPPED ONE\n", "SKIPPED TWO\n"],
    );
    assert_eq!(git_output(dir, &["show", ":file.txt"]), "MAIN");
}

#[test]
fn merge_commit_completion_keeps_durable_draft_inspection_only() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let path = Path::new("file.txt");
    let mut backend = open_backend(dir);
    let session = backend.detect_conflict_session().unwrap();
    let expected = backend
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;
    let mut buffer = backend.resolution_buffer_from_repo().unwrap();
    buffer.set_manual_text(path, "MERGE RESULT\n").unwrap();
    let saved_path = buffer.autosave().unwrap();
    let saved_document = std::fs::read(&saved_path).unwrap();
    backend
        .stage_conflict_resolution(&session, &buffer, &expected)
        .unwrap();
    let operation = Operation::MergeCommit {
        message: "Merge feature with retained recovery".into(),
    };
    let plan = backend.plan(&operation).unwrap();
    let report = backend.run_recorded(&operation, &plan);
    assert!(matches!(report.result, Ok(OperationOutcome::Commit(_))));
    drop(buffer);
    drop(backend);
    assert_eq!(saved_document, std::fs::read(&saved_path).unwrap());

    let backend = open_backend(dir);
    assert!(backend.detect_conflict_session().is_none());
    assert_eq!(git_output(dir, &["show", "HEAD:file.txt"]), "MERGE RESULT");
    let parents = git_output(dir, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3);
    let completed = backend.resolution_buffer_from_repo_with_autosave().unwrap();
    assert_inspection_only(&completed, &["MERGE RESULT\n"]);
    assert_eq!(saved_path, completed.autosave().unwrap());
    assert_inspection_only(
        &open_backend(dir)
            .resolution_buffer_from_repo_with_autosave()
            .unwrap(),
        &["MERGE RESULT\n"],
    );
    assert_eq!(git_output(dir, &["show", ":file.txt"]), "MERGE RESULT");
}
