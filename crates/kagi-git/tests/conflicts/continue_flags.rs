//! #1721: the accepted staged tree includes semantic extended index flags.
use super::continue_revision::{open_backend, physical_state};
use super::*;

fn new_empty_entry(repo: &Repository) -> git2::IndexEntry {
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    index.get_path(Path::new("new-empty"), 0).unwrap()
}

fn saved_rebase_with_intent() -> (
    tempfile::TempDir,
    kagi_git::Backend,
    Repository,
    git2::IndexEntry,
) {
    let fixture = rebase_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let mut resolving = backend.resolution_buffer_from_repo().unwrap();
    resolving
        .set_manual_text(Path::new("file.txt"), "SAVED RESOLUTION\n")
        .unwrap();
    backend
        .execute_conflict_save(&resolving, Path::new("file.txt"))
        .unwrap();
    std::fs::write(path.join("new-empty"), b"").unwrap();
    git(path, &["add", "-N", "new-empty"]);
    git(path, &["update-index", "--skip-worktree", "new-empty"]);
    let repo = Repository::open(path).unwrap();
    let intent = new_empty_entry(&repo);
    assert_eq!(intent.flags_extended & 0x6000, 0x6000);
    (fixture, backend, repo, intent)
}

#[test]
fn intent_to_add_stage_change_requires_new_empty_continue_approval() {
    if !test_support::run_isolated() {
        return;
    }
    let (fixture, backend, repo, intent) = saved_rebase_with_intent();
    let path = fixture.path();
    let old_tree = git_output(path, &["write-tree"]);
    let approved = backend.conflict_snapshot().unwrap().unwrap();
    let session = &approved.session;
    let expected = &approved.observation.revision;
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    assert!(session.files.is_empty());
    backend
        .plan_conflict_continue_route(session, &buffer, "side", expected)
        .unwrap();

    // Only intent-to-add changes. Keep the same entry path, blob, mode and
    // short flags by restoring skip-worktree after the external real staging.
    git(path, &["update-index", "--no-skip-worktree", "new-empty"]);
    git(path, &["add", "new-empty"]);
    git(path, &["update-index", "--skip-worktree", "new-empty"]);
    let committed = new_empty_entry(&repo);
    assert_eq!(intent.path, committed.path);
    assert_eq!(intent.id, committed.id);
    assert_eq!(intent.mode, committed.mode);
    assert_eq!(intent.flags, committed.flags);
    assert_eq!(committed.flags_extended & 0x6000, 0x4000);
    let current = backend.conflict_snapshot().unwrap().unwrap();
    assert_eq!(current.session.op, session.op);
    let new_tree = git_output(path, &["write-tree"]);
    assert_ne!(old_tree, new_tree);
    let before = physical_state(path);
    let objects = git_output(path, &["count-objects", "-v"]);
    let plan = backend.plan_conflict_continue(session, &buffer, expected);
    let route = backend.plan_conflict_continue_route(session, &buffer, "side", expected);
    let staging = backend.stage_conflict_resolution(session, &buffer, expected);
    let execution = backend.execute_conflict_continue(session, &buffer, expected);
    let after = physical_state(path);
    let objects_after = git_output(path, &["count-objects", "-v"]);
    eprintln!(
        "intent-to-add approval: old={expected:?} current={:?}; flags={} extended={:#x}->{:#x}; \
         trees={old_tree}->{new_tree}; plan_admitted={} route_admitted={} stage_admitted={} \
         execute_admitted={} error={:?}\nbefore={before:#?}\nafter={after:#?}\n\
         objects_before={objects}\nobjects_after={objects_after}",
        current.observation.revision,
        intent.flags,
        intent.flags_extended,
        committed.flags_extended,
        plan.is_ok(),
        route.is_ok(),
        staging.is_ok(),
        execution.is_ok(),
        execution.as_ref().err().map(ToString::to_string),
    );
    assert!(
        plan.is_err()
            && route.is_err()
            && staging.is_err()
            && execution.is_err()
            && before == after
            && objects == objects_after,
        "an already approved empty Continue must refuse a changed staged tree without mutation",
    );
    assert_ne!(current.observation.revision, *expected);

    // The same live input may be reused only under a new explicit tree approval.
    let revision = &current.observation.revision;
    backend
        .plan_conflict_continue_route(&current.session, &buffer, "side", revision)
        .unwrap();
    backend
        .execute_conflict_continue(&current.session, &buffer, revision)
        .unwrap();
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), new_tree);
    assert!(!git_output(path, &["ls-tree", "HEAD", "new-empty"]).is_empty());
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}
