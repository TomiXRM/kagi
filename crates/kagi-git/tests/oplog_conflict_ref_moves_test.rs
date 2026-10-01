//! #884 (ADR-0214 §4): conflict writes record the refs they moved, through
//! the same `Backend::observe_ref_moves` every other write uses — so a restore
//! can cross a merge or cherry-pick that was resolved in Kagi.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use kagi_domain::conflict_family::ConflictRequest;
use kagi_domain::ref_moves::RefMove;
use kagi_git::backend::ExecutionPolicy;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry};
use kagi_git::{Backend, CommitId, Operation, ResolutionChoice};
use std::path::{Path, PathBuf};

fn tip(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
}

/// `base` on main; `side` and `main` each change `file.txt`, so replaying
/// one onto the other conflicts. Left checked out on main.
fn diverged(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "file.txt", "base\n");
    commit_all(&repo, "base");
    git(&repo, &["checkout", "-qb", "side"]);
    write_file(&repo, "file.txt", "SIDE\n");
    commit_all(&repo, "side");
    git(&repo, &["checkout", "-q", "main"]);
    write_file(&repo, "file.txt", "MAIN\n");
    commit_all(&repo, "main");
    repo
}

fn backend(dir: &Path) -> Backend {
    let mut b = Backend::open(dir).unwrap();
    b.set_auto_snapshot(false);
    b
}

/// Run `op` through the recorded pipeline; its entry.
fn run(dir: &Path, op: Operation) -> OpLogEntry {
    let mut b = backend(dir);
    let plan = b.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{op:?}: {:?}", plan.blockers);
    b.run(&op, &plan).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    read_oplog_tail_for_repo(dir, 1).pop().expect("recorded")
}

/// A conflict request through the production recorded boundary
/// (`plan_recorded_conflict` → `run_recorded_conflict`); its entry.
fn run_conflict(dir: &Path, request: ConflictRequest) -> OpLogEntry {
    let plan = Backend::plan_recorded_conflict(dir, request).unwrap();
    let report = Backend::run_recorded_conflict(&plan, ExecutionPolicy::human(false), None);
    report.recording.entry().clone()
}

fn head_on(branch: &str, old: &str, new: &str) -> RefMove {
    RefMove {
        refname: "HEAD".into(),
        old: Some(old.into()),
        new: Some(new.into()),
        old_symbolic: Some(format!("refs/heads/{branch}")),
        new_symbolic: Some(format!("refs/heads/{branch}")),
    }
}

fn branch(name: &str, old: &str, new: &str) -> RefMove {
    RefMove {
        refname: format!("refs/heads/{name}"),
        old: Some(old.into()),
        new: Some(new.into()),
        old_symbolic: None,
        new_symbolic: None,
    }
}

#[test]
fn a_conflict_save_records_nothing_moved_and_a_restore_crosses_the_merge() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged(tmp.path());
    let before_merge = tip(&repo, "main");
    let mark = run(
        &repo,
        Operation::CreateBranch {
            name: "mark".into(),
            at: CommitId(before_merge.clone()),
        },
    );
    run(
        &repo,
        Operation::MergeIntoConflict {
            target: "side".into(),
        },
    );

    // Resolving the file: the save is a write, but moves no ref.
    let b = backend(&repo);
    let snapshot = b.conflict_snapshot().unwrap().expect("merge in conflict");
    let mut buffer = b.resolution_buffer_from_repo().unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
        .unwrap();
    let request = Backend::conflict_save_request(
        snapshot.observation.revision,
        &buffer,
        Path::new("file.txt"),
        snapshot.observation.kind,
        "",
    )
    .unwrap();
    let save = run_conflict(&repo, request);
    assert_eq!(save.ref_moves, Some(Vec::new()), "recorded, nothing moved");

    // The merge commit moves main and HEAD with it.
    let merge = run(
        &repo,
        Operation::MergeCommit {
            message: "merge side".into(),
        },
    );
    let merged = tip(&repo, "main");
    assert_eq!(
        merge.ref_moves,
        Some(vec![
            head_on("main", &before_merge, &merged),
            branch("main", &before_merge, &merged),
        ])
    );

    // Every entry after `mark` is recorded: the restore plans and runs.
    let op = Operation::RestoreToPoint { entry_id: mark.id };
    let plan = backend(&repo).plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend(&repo).run(&op, &plan).unwrap();
    assert_eq!(
        tip(&repo, "main"),
        before_merge,
        "main is back before the merge"
    );
}

#[test]
fn a_cherry_pick_continue_records_the_commit_on_head_and_its_branch() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = diverged(tmp.path());
    let before = tip(&repo, "main");
    assert!(!git_succeeds(&repo, &["cherry-pick", "side"]));

    let b = backend(&repo);
    let session = kagi_git::detect_conflict_session(&git2::Repository::open(&repo).unwrap())
        .expect("cherry-pick in conflict");
    let mut buffer = b.resolution_buffer_from_repo().unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();
    let (result, moves) = b.observe_ref_moves(|b| b.execute_conflict_continue(&session, &buffer));
    result.expect("continue");
    let after = tip(&repo, "main");
    assert_ne!(after, before);
    assert_eq!(
        moves,
        Some(vec![
            head_on("main", &before, &after),
            branch("main", &before, &after)
        ])
    );
}

/// `side` replayed onto `main` and left mid-conflict: HEAD is detached at
/// main's tip, `side` has not moved yet.
fn rebase_in_conflict(tmp: &Path) -> (PathBuf, String, String) {
    let repo = diverged(tmp);
    let main = tip(&repo, "main");
    let side = tip(&repo, "side");
    git(&repo, &["checkout", "-q", "side"]);
    assert!(!git_succeeds(&repo, &["rebase", "main"]));
    (repo, main, side)
}

#[test]
fn a_rebase_abort_records_head_going_back_to_its_branch() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (repo, main, side) = rebase_in_conflict(tmp.path());
    let observed = backend(&repo)
        .conflict_snapshot()
        .unwrap()
        .expect("rebase in progress")
        .in_progress();
    let abort = run_conflict(&repo, Backend::conflict_abort_request(&observed));
    assert_eq!(
        abort.ref_moves,
        Some(vec![RefMove {
            refname: "HEAD".into(),
            old: Some(main),
            new: Some(side.clone()),
            old_symbolic: None,
            new_symbolic: Some("refs/heads/side".into()),
        }]),
        "HEAD re-attaches to side at its old tip; side itself never moved"
    );
    assert_eq!(tip(&repo, "side"), side);
}

#[test]
fn skipping_the_last_rebase_step_records_the_branch_finishing() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (repo, main, side) = rebase_in_conflict(tmp.path());
    let b = backend(&repo);
    let session = kagi_git::detect_conflict_session(&git2::Repository::open(&repo).unwrap())
        .expect("rebase in conflict");
    let buffer = b.resolution_buffer_from_repo().unwrap();
    let (result, moves) = b.observe_ref_moves(|b| b.execute_conflict_skip(&session, &buffer));
    result.expect("skip");
    // The only commit was skipped: side ends on main's tip, HEAD back on side.
    assert_eq!(tip(&repo, "side"), main);
    let moves = moves.expect("recorded");
    assert!(moves.contains(&branch("side", &side, &main)), "{moves:?}");
    assert!(
        moves.iter().any(|m| m.refname == "HEAD"
            && m.old_symbolic.is_none()
            && m.new_symbolic.as_deref() == Some("refs/heads/side")),
        "{moves:?}"
    );
}

#[test]
fn a_refused_conflict_write_records_nothing_moved() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (repo, ..) = rebase_in_conflict(tmp.path());
    let observed = backend(&repo)
        .conflict_snapshot()
        .unwrap()
        .expect("rebase in progress")
        .in_progress();
    let recording = Backend::record_conflict_refusal(
        &repo,
        ExecutionPolicy::human(false),
        &Backend::conflict_abort_request(&observed),
        "refused before anything ran",
    );
    assert_eq!(recording.entry().ref_moves, Some(Vec::new()));
}
