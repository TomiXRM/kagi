//! #1127: abort restores the whole sequence, not only the conflicting pick.
#[path = "../../../tests/support/cherry_pick_sequence.rs"]
mod cherry_pick_sequence;
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
#[path = "../../../tests/support/isolated.rs"]
mod test_support;
use cherry_pick_sequence::Fixture;
use git2::{Oid, Repository, RepositoryState};
use kagi_domain::conflict_family::ConflictProgress;
use kagi_git::{backend::ExecutionPolicy, oplog::OpOutcome, Backend};

fn abort_and_assert_restored(fixture: &Fixture, detached: bool) {
    let root = fixture.path();
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let preview = backend.plan_operation_abort().unwrap();
    assert!(preview.blockers.is_empty());
    assert!(preview.predicted.head.contains(&fixture.start[..7]));
    let moved_head = Repository::open(root)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
        .to_string()
        != fixture.start;
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let plan = Backend::plan_recorded_conflict(root, request).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::default(), None);
    assert_eq!(
        report.evidence.progress,
        ConflictProgress::Verified,
        "{report:?}"
    );
    let repo = Repository::open(root).unwrap();
    assert_eq!(
        repo.head().unwrap().target(),
        Some(Oid::from_str(&fixture.start).unwrap())
    );
    assert_eq!(repo.head_detached().unwrap(), detached);
    let mut index = repo.index().unwrap();
    assert!(!index.has_conflicts());
    assert_eq!(
        index.write_tree().unwrap(),
        repo.head().unwrap().peel_to_tree().unwrap().id()
    );
    assert!(
        repo.statuses(None).unwrap().is_empty(),
        "all picked files must be removed"
    );
    assert_eq!(repo.state(), RepositoryState::Clean);
    for name in [
        "first.txt",
        "second.txt",
        "third.txt",
        ".git/sequencer",
        ".git/CHERRY_PICK_HEAD",
    ] {
        assert!(!root.join(name).exists(), "leftover {name}");
    }
    let entries = kagi_git::oplog::read_oplog_tail_for_repo(root, 10);
    let entry = entries
        .iter()
        .find(|entry| entry.op == "cherry-pick-abort")
        .unwrap();
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    if moved_head {
        assert!(entry
            .ref_moves
            .as_ref()
            .unwrap()
            .iter()
            .any(|movement| movement.new.as_deref() == Some(fixture.start.as_str())));
    }
}

#[test]
fn cherry_pick_abort_sequence_restores_start_head_index_worktree_and_receipt() {
    if !test_support::run_isolated() {
        return;
    }
    abort_and_assert_restored(&Fixture::new(true, false), false);
}

#[test]
fn cherry_pick_abort_single_commit_restores_current_head() {
    if !test_support::run_isolated() {
        return;
    }
    abort_and_assert_restored(&Fixture::new(false, false), false);
}

#[test]
fn cherry_pick_abort_sequence_ignores_stale_orig_head_and_supports_detached_head() {
    if !test_support::run_isolated() {
        return;
    }
    for detached in [false, true] {
        let fixture = Fixture::new(true, detached);
        let repo = Repository::open(fixture.path()).unwrap();
        let stale = repo.revparse_single("side").unwrap().id().to_string();
        std::fs::write(repo.path().join("ORIG_HEAD"), stale).unwrap();
        abort_and_assert_restored(&fixture, detached);
    }
}

#[test]
fn cherry_pick_abort_single_commit_ignores_stale_orig_head() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(false, false);
    let repo = Repository::open(fixture.path()).unwrap();
    let stale = repo.revparse_single("side").unwrap().id().to_string();
    std::fs::write(repo.path().join("ORIG_HEAD"), stale).unwrap();
    abort_and_assert_restored(&fixture, false);
}

#[test]
fn cherry_pick_abort_sequence_missing_invalid_or_unavailable_start_is_refused_without_mutation() {
    if !test_support::run_isolated() {
        return;
    }
    for start in [
        None,
        Some("not-an-oid"),
        Some("0000000000000000000000000000000000000001"),
    ] {
        let fixture = Fixture::new(true, false);
        let root = fixture.path();
        let path = root.join(".git/sequencer/head");
        match start {
            Some(start) => std::fs::write(&path, start).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }
        let backend = Backend::open(root).unwrap();
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let head = Repository::open(root)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        let worktree = std::fs::read(root.join("a.txt")).unwrap();
        let preview = backend.plan_operation_abort().unwrap();
        assert!(matches!(
            preview.blockers.as_slice(),
            [kagi_domain::plan_note::PlanNote::Conflicts(
                kagi_domain::plan_note::ConflictsNote::AbortStartUnavailable
            )]
        ));
        let request = Backend::conflict_abort_request(&snapshot.in_progress());
        let error = Backend::plan_recorded_conflict(root, request).unwrap_err();
        assert!(error
            .to_string()
            .contains("pre-sequence HEAD cannot be established"));
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        backend
            .execute_conflict_abort(&snapshot.session, &buffer)
            .expect_err("raw executor must refuse too");
        assert_eq!(
            Repository::open(root).unwrap().head().unwrap().target(),
            Some(head)
        );
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), worktree);
        assert!(root.join("first.txt").exists());
        assert!(root.join("second.txt").exists());
        assert!(root.join(".git/CHERRY_PICK_HEAD").exists());
    }
}

#[test]
fn cherry_pick_abort_sequence_start_drift_after_confirmation_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(true, false);
    let root = fixture.path();
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let plan = Backend::plan_recorded_conflict(root, request).unwrap();
    let head = Repository::open(root)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    std::fs::write(root.join(".git/sequencer/head"), head.to_string()).unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::default(), None);
    assert_eq!(report.evidence.progress, ConflictProgress::NotStarted);
    assert!(matches!(
        report.blocker,
        Some(kagi_domain::plan_note::PlanNote::Conflicts(
            kagi_domain::plan_note::ConflictsNote::PlanChanged
        ))
    ));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(
        Repository::open(root).unwrap().head().unwrap().target(),
        Some(head)
    );
}

#[test]
fn cherry_pick_abort_sequence_preserves_guard_on_earlier_pick_edits() {
    if !test_support::run_isolated() {
        return;
    }
    for staged in [false, true] {
        let fixture = Fixture::new(true, false);
        let root = fixture.path();
        std::fs::write(root.join("first.txt"), "user edit during conflict\\n").unwrap();
        if staged {
            let repository = Repository::open(root).unwrap();
            let mut index = repository.index().unwrap();
            index.add_path(std::path::Path::new("first.txt")).unwrap();
            index.write().unwrap();
        }
        let backend = Backend::open(root).unwrap();
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let request = Backend::conflict_abort_request(&snapshot.in_progress());
        let plan = Backend::plan_recorded_conflict(root, request).unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::default(), None);
        assert_eq!(report.evidence.progress, ConflictProgress::NotStarted);
        assert!(report.evidence.detail.contains("first.txt"), "{report:?}");
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(root.join("first.txt")).unwrap(),
            "user edit during conflict\\n"
        );
    }
}

#[test]
fn cherry_pick_abort_stale_sequence_refuses_to_rewind_unrelated_commits() {
    if !test_support::run_isolated() {
        return;
    }
    use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    init_repo(root, "main");
    write_file(root, "a.txt", "base\n");
    commit_all(root, "base");
    git(root, &["switch", "-c", "topic"]);
    write_file(root, "a.txt", "topic A\n");
    commit_all(root, "A");
    let a = git_output(root, &["rev-parse", "HEAD"]);
    write_file(root, "b.txt", "B\n");
    commit_all(root, "B");
    let b = git_output(root, &["rev-parse", "HEAD"]);
    write_file(root, "c.txt", "C\n");
    commit_all(root, "C");
    let c = git_output(root, &["rev-parse", "HEAD"]);
    write_file(root, "a.txt", "new single pick\n");
    commit_all(root, "new pick");
    let next = git_output(root, &["rev-parse", "HEAD"]);
    git(root, &["switch", "main"]);
    write_file(root, "a.txt", "main\n");
    commit_all(root, "main diverges");
    assert!(!git_succeeds(root, &["cherry-pick", &a, &b, &c]));
    write_file(root, "a.txt", "manually resolved A\n");
    commit_all(root, "manual A resolution");
    assert!(root.join(".git/sequencer").exists());
    assert!(!root.join(".git/CHERRY_PICK_HEAD").exists());
    for name in ["D", "E"] {
        write_file(root, name, &format!("unrelated commit {name}\n"));
        commit_all(root, name);
    }
    let head = git_output(root, &["rev-parse", "HEAD"]);
    // libgit2 writes a real new conflict without refreshing Git's old
    // sequencer safety marker. Kagi's guarded single-pick executor currently
    // refuses conflicts before writing; abort must still handle this state.
    {
        let repo = Repository::open(root).unwrap();
        let commit = repo.find_commit(Oid::from_str(&next).unwrap()).unwrap();
        repo.cherrypick(&commit, None).unwrap();
        assert!(repo.index().unwrap().has_conflicts());
    }
    assert_ne!(
        std::fs::read_to_string(root.join(".git/sequencer/abort-safety"))
            .unwrap()
            .trim(),
        head
    );
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let conflict = std::fs::read(root.join("a.txt")).unwrap();
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let preview = backend.plan_operation_abort().unwrap();
    assert!(preview
        .blockers
        .iter()
        .any(|note| note.message_en().contains("HEAD moved")));
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let error = Backend::plan_recorded_conflict(root, request).unwrap_err();
    assert!(error.to_string().contains("HEAD moved"));
    let buffer = backend.resolution_buffer_from_repo().unwrap();
    let error = backend
        .execute_conflict_abort(&snapshot.session, &buffer)
        .unwrap_err();
    assert!(error.to_string().contains("HEAD moved"));
    assert_eq!(git_output(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), conflict);
    for name in ["D", "E"] {
        assert_eq!(
            std::fs::read_to_string(root.join(name)).unwrap(),
            format!("unrelated commit {name}\n")
        );
    }
    assert!(root.join(".git/sequencer").exists());
    assert!(root.join(".git/CHERRY_PICK_HEAD").exists());
}

#[test]
fn cherry_pick_abort_sequence_preserves_unrelated_dirty_tracked_and_untracked_files() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(true, false);
    let root = fixture.path();
    let tracked = b"local tracked edit\0\n";
    let untracked = b"local untracked file\0\n";
    std::fs::write(root.join("unrelated.txt"), tracked).unwrap();
    std::fs::write(root.join("untracked.txt"), untracked).unwrap();
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let plan = Backend::plan_recorded_conflict(root, request).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::default(), None);
    assert_eq!(
        report.evidence.progress,
        ConflictProgress::Verified,
        "{report:?}"
    );
    let repo = Repository::open(root).unwrap();
    assert_eq!(
        repo.head().unwrap().target().unwrap().to_string(),
        fixture.start
    );
    assert_eq!(std::fs::read(root.join("unrelated.txt")).unwrap(), tracked);
    assert_eq!(
        std::fs::read(root.join("untracked.txt")).unwrap(),
        untracked
    );
    let mut index = repo.index().unwrap();
    assert_eq!(
        index.write_tree().unwrap(),
        repo.head().unwrap().peel_to_tree().unwrap().id()
    );
    assert_eq!(repo.state(), RepositoryState::Clean);
}

#[test]
fn cherry_pick_abort_sequence_safety_marker_drift_after_confirmation_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let fixture = Fixture::new(true, false);
    let root = fixture.path();
    let backend = Backend::open(root).unwrap();
    let snapshot = backend.conflict_snapshot().unwrap().unwrap();
    let request = Backend::conflict_abort_request(&snapshot.in_progress());
    let plan = Backend::plan_recorded_conflict(root, request).unwrap();
    std::fs::write(root.join(".git/sequencer/abort-safety"), &fixture.start).unwrap();
    let head = Repository::open(root)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::default(), None);
    assert_eq!(report.evidence.progress, ConflictProgress::NotStarted);
    assert!(matches!(
        report.blocker,
        Some(kagi_domain::plan_note::PlanNote::Conflicts(
            kagi_domain::plan_note::ConflictsNote::PlanChanged
        ))
    ));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(
        Repository::open(root).unwrap().head().unwrap().target(),
        Some(head)
    );
}

#[test]
fn cherry_pick_abort_sequence_missing_or_invalid_safety_marker_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    for marker in [None, Some("not-an-oid")] {
        let fixture = Fixture::new(true, false);
        let root = fixture.path();
        let path = root.join(".git/sequencer/abort-safety");
        match marker {
            Some(marker) => std::fs::write(&path, marker).unwrap(),
            None => std::fs::remove_file(&path).unwrap(),
        }
        let backend = Backend::open(root).unwrap();
        let preview = backend.plan_operation_abort().unwrap();
        assert!(matches!(
            preview.blockers.as_slice(),
            [kagi_domain::plan_note::PlanNote::Conflicts(
                kagi_domain::plan_note::ConflictsNote::AbortStartUnavailable
            )]
        ));
        let snapshot = backend.conflict_snapshot().unwrap().unwrap();
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let head = Repository::open(root)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        backend
            .execute_conflict_abort(&snapshot.session, &buffer)
            .unwrap_err();
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(
            Repository::open(root).unwrap().head().unwrap().target(),
            Some(head)
        );
    }
}
