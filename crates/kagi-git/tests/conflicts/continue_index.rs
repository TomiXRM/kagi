//! #1721: Continue reads the real index after another handle performs Save.
use super::*;
use kagi_git::{Backend, ContinueBlocker, ContinueRoute};

fn open_backend(path: &Path) -> Backend {
    Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false)).unwrap()
}

fn replay_first(backend: &Backend, path: &Path) {
    let repo = Repository::open(path).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "RESOLVED ONE\n")
        .unwrap();
    backend
        .execute_conflict_continue(&session, &buffer)
        .unwrap();
    assert_eq!(git_output(path, &["show", "HEAD:file.txt"]), "RESOLVED ONE");
}

#[test]
fn public_continue_reloads_cached_index_after_separate_save() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = rebase_two_conflicting_steps_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    replay_first(&backend, path);
    let repo = Repository::open(path).unwrap();
    let cached_index = repo.index().unwrap();
    assert!(cached_index.has_conflicts());
    let session = detect_conflict_session(&repo).unwrap();
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(Path::new("file.txt"), "RESOLVED TWO\n")
        .unwrap();
    // Prime the same public UI backend while its index is still unmerged.
    assert!(backend
        .plan_conflict_continue_route(&session, &buffer, "side")
        .is_ok());
    open_backend(path)
        .execute_conflict_save(&buffer, Path::new("file.txt"))
        .unwrap();
    assert_eq!(git_output(path, &["ls-files", "-u"]), "");
    assert!(
        cached_index.has_conflicts(),
        "a retained libgit2 view still has the old stages"
    );

    let fresh = Repository::open(path).unwrap();
    let saved_session = detect_conflict_session(&fresh).unwrap();
    assert!(saved_session.files.is_empty());
    let saved_buffer = ResolutionBuffer::from_repo(&fresh).unwrap();
    let index_before = std::fs::read(path.join(".git/index")).unwrap();
    let file_before = std::fs::read(path.join("file.txt")).unwrap();
    let route = backend
        .plan_conflict_continue_route(&saved_session, &saved_buffer, "side")
        .unwrap();
    match route {
        ContinueRoute::SequencerPlan(plan) => assert!(plan.blockers.is_empty()),
        other => panic!("unexpected continuation route: {other:?}"),
    }
    assert_eq!(
        std::fs::read(path.join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(std::fs::read(path.join("file.txt")).unwrap(), file_before);
    backend
        .execute_conflict_continue(&saved_session, &saved_buffer)
        .unwrap();
    assert_eq!(git_output(path, &["show", "HEAD:file.txt"]), "RESOLVED TWO");
    assert_eq!(
        git_output(path, &["show", "HEAD^:file.txt"]),
        "RESOLVED ONE"
    );
    assert_eq!(git_output(path, &["status", "--porcelain"]), "");
    assert_eq!(
        Repository::open(path).unwrap().state(),
        git2::RepositoryState::Clean
    );
}

#[test]
fn public_continue_propagates_unreadable_index_without_writing() {
    if !test_support::run_isolated() {
        return;
    }
    for directory_instead_of_index in [false, true] {
        let fixture = merge_conflict_repo();
        let path = fixture.path();
        let backend = open_backend(path);
        let repo = Repository::open(path).unwrap();
        let session = detect_conflict_session(&repo).unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
            .unwrap();
        assert!(backend
            .plan_conflict_continue_route(&session, &buffer, "main")
            .is_ok());
        let index_path = path.join(".git/index");
        let original_index = std::fs::read(&index_path).unwrap();
        let original_file = std::fs::read(path.join("file.txt")).unwrap();
        let original_head = git_output(path, &["rev-parse", "HEAD"]);
        if directory_instead_of_index {
            std::fs::remove_file(&index_path).unwrap();
            std::fs::create_dir(&index_path).unwrap();
        } else {
            std::fs::write(&index_path, b"not a Git index").unwrap();
        }
        assert!(continue_blockers(&repo, &session, &buffer).is_err());
        assert!(backend.continue_blockers(&session, &buffer).is_err());
        assert!(backend.plan_conflict_continue(&session, &buffer).is_err());
        assert!(backend
            .plan_conflict_continue_route(&session, &buffer, "main")
            .is_err());
        assert!(backend
            .execute_conflict_continue(&session, &buffer)
            .is_err());
        assert!(backend
            .stage_conflict_resolution(&session, &buffer)
            .is_err());
        assert_eq!(std::fs::read(path.join("file.txt")).unwrap(), original_file);
        assert_eq!(git_output(path, &["rev-parse", "HEAD"]), original_head);
        assert!(!path.join(".git/index.lock").exists());
        if directory_instead_of_index {
            assert!(index_path.is_dir());
            std::fs::remove_dir(&index_path).unwrap();
        } else {
            assert_eq!(std::fs::read(&index_path).unwrap(), b"not a Git index");
        }
        std::fs::write(&index_path, original_index).unwrap();
        assert_eq!(git_output(path, &["status", "--porcelain"]), "UU file.txt");
    }
}

#[test]
fn public_continue_still_refuses_untracked_unmerged_path() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = merge_conflict_repo();
    let path = fixture.path();
    let backend = open_backend(path);
    let repo = Repository::open(path).unwrap();
    let mut session = detect_conflict_session(&repo).unwrap();
    session.files.clear();
    let buffer = ResolutionBuffer::new(path);
    let original_index = std::fs::read(path.join(".git/index")).unwrap();
    let original_file = std::fs::read(path.join("file.txt")).unwrap();
    let blockers = backend.continue_blockers(&session, &buffer).unwrap();
    assert!(blockers.iter().any(|blocker| matches!(
        blocker, ContinueBlocker::IndexUnmerged(paths)
            if paths.iter().any(|path| path == "file.txt")
    )));
    assert!(backend
        .plan_conflict_continue_route(&session, &buffer, "main")
        .is_err());
    assert!(backend
        .execute_conflict_continue(&session, &buffer)
        .is_err());
    assert!(backend
        .stage_conflict_resolution(&session, &buffer)
        .is_err());
    assert_eq!(
        std::fs::read(path.join(".git/index")).unwrap(),
        original_index
    );
    assert_eq!(std::fs::read(path.join("file.txt")).unwrap(), original_file);
}
