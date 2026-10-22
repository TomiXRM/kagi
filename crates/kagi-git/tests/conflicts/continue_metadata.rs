//! #1721: a frozen Continue also owns the commit-producing sequencer metadata.
use super::continue_revision::{open_backend, physical_state};
use super::*;

fn commit_author(path: &Path, revision: &str) -> String {
    git_output(path, &["show", "-s", "--format=%an%n%ae%n%aI", revision])
}

fn changed_message_requires_new_approval(fixture: TempDir, relative: &str, source: &str) {
    let path = fixture.path();
    let backend = open_backend(path);
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "METADATA SAVED RESOLUTION\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let expected = &approved.observation.revision;
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(approved.session.files.is_empty());
    let tree = git_output(path, &["write-tree"]);
    let author = commit_author(path, source);
    backend
        .plan_conflict_continue_route(&approved.session, &buffer, "side", expected)
        .unwrap();

    let metadata = Repository::open(path).unwrap().path().join(relative);
    let original_message = std::fs::read(&metadata).unwrap();
    let changed_message = b"EXTERNALLY CHANGED APPROVAL MESSAGE\n";
    assert_ne!(original_message, changed_message);
    std::fs::write(&metadata, changed_message).unwrap();
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(current.session.op, approved.session.op);
    let before = physical_state(path);
    let before_commit = git_output(path, &["cat-file", "-p", "HEAD"]);
    let objects = git_output(path, &["count-objects", "-v"]);
    let plan = backend.plan_conflict_continue(&approved.session, &buffer, expected);
    let route =
        backend.plan_conflict_continue_route(&approved.session, &buffer, "side", expected);
    let stage = backend.stage_conflict_resolution(&approved.session, &buffer, expected);
    let execution = backend.execute_conflict_continue(&approved.session, &buffer, expected);
    let after = physical_state(path);
    let after_commit = git_output(path, &["cat-file", "-p", "HEAD"]);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    let message_after = std::fs::read(&metadata).ok();
    eprintln!(
        "sequencer metadata approval: file={relative}; original={expected:?}; current={:?}; \
         original_message={original_message:?}; changed_message={changed_message:?}; \
         approved_tree={tree}; approved_author={author:?}; plan_admitted={} route_admitted={} \
         stage_admitted={} execute_admitted={} error={:?}\nbefore={before:#?}\nafter={after:#?}\n\
         before_commit={before_commit}\nafter_commit={after_commit}\n\
         objects_before={objects}\nobjects_after={objects_after}\nmetadata_after={message_after:?}",
        current.observation.revision,
        plan.is_ok(),
        route.is_ok(),
        stage.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
    assert!(
        plan.is_err()
            && route.is_err()
            && stage.is_err()
            && execution.is_err()
            && before == after
            && objects == objects_after
            && message_after.as_deref() == Some(changed_message.as_slice()),
        "an old Continue approval must not commit changed sequencer metadata",
    );
    assert_ne!(current.observation.revision, *expected);

    // A message-only change does not expire the same file's editing identity.
    // Only a new explicit full approval may authorize the changed commit message.
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
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git_output(path, &["show", "-s", "--format=%B", "HEAD"]),
        "EXTERNALLY CHANGED APPROVAL MESSAGE",
    );
    assert_eq!(commit_author(path, "HEAD"), author);
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean,
    );
}

#[test]
fn rebase_message_change_requires_new_empty_continue_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_message_requires_new_approval(
        rebase_conflict_repo(),
        "rebase-merge/message",
        "REBASE_HEAD",
    );
}

#[test]
fn cherry_pick_message_change_requires_new_empty_continue_approval() {
    if !test_support::run_isolated() {
        return;
    }
    changed_message_requires_new_approval(
        cherry_pick_conflict_repo(),
        "MERGE_MSG",
        "CHERRY_PICK_HEAD",
    );
}
