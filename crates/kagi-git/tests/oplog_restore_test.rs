//! #334 slice 2b (ADR-0214 §5): op-revert and restore-to-point put branches
//! back from **recorded** ref moves only, through plan → preflight → execute →
//! verify → oplog, and are themselves recorded — so they can be undone too.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use kagi_domain::plan_note::{OplogRestoreNote, PlanNote};
use kagi_git::oplog::{append_oplog, read_oplog_tail_for_repo, OpLogEntry, OpOutcome};
use kagi_git::{Backend, CommitId, GitError, Operation, OperationPlan, StateSummary};
use std::path::{Path, PathBuf};

fn repo(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "a.txt", "a\n");
    commit_all(&repo, "base");
    repo
}

fn backend(dir: &Path) -> Backend {
    let mut b = Backend::open(dir).unwrap();
    b.set_auto_snapshot(false);
    b
}

/// Run `op` through the recorded pipeline (it must succeed); its entry id.
fn run(dir: &Path, op: Operation) -> u64 {
    let mut b = backend(dir);
    let plan = b.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{op:?}: {:?}", plan.blockers);
    b.run(&op, &plan).unwrap_or_else(|e| panic!("{op:?}: {e}"));
    newest(dir).id
}

fn newest(dir: &Path) -> OpLogEntry {
    read_oplog_tail_for_repo(dir, 1).pop().expect("recorded")
}

fn plan(dir: &Path, op: &Operation) -> OperationPlan {
    backend(dir).plan(op).unwrap()
}

fn restore_blockers(plan: &OperationPlan) -> Vec<OplogRestoreNote> {
    plan.blockers
        .iter()
        .filter_map(|b| match b {
            PlanNote::OplogRestore(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

fn branches(dir: &Path) -> String {
    git_output(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
        ],
    )
}

fn create(dir: &Path, name: &str) -> u64 {
    let at = CommitId(git_output(dir, &["rev-parse", "HEAD"]));
    run(
        dir,
        Operation::CreateBranch {
            name: name.into(),
            at,
        },
    )
}

fn commit(dir: &Path, content: &str) -> u64 {
    write_file(dir, "a.txt", content);
    git(dir, &["add", "a.txt"]);
    run(
        dir,
        Operation::Commit {
            message: content.trim().into(),
        },
    )
}

#[test]
fn restoring_three_operations_back_puts_every_branch_where_it_was() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let at_point = branches(&repo);
    commit(&repo, "two\n");
    create(&repo, "b");
    commit(&repo, "three\n");
    assert_ne!(branches(&repo), at_point);

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    assert!(p.destructive, "two-stage confirm");
    let mut b = backend(&repo);
    b.run(&op, &p).unwrap();

    assert_eq!(branches(&repo), at_point, "main back, b deleted, a kept");
    let entry = newest(&repo);
    assert_eq!(entry.op, "restore-to-point");
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    // Both branches it moved keep their pre-restore tips.
    assert_eq!(entry.backup_refs.len(), 2, "{:?}", entry.backup_refs);
    let moved: Vec<String> = entry
        .ref_moves
        .expect("the restore records its own moves")
        .into_iter()
        .map(|m| m.refname)
        .collect();
    assert!(moved.contains(&"refs/heads/main".to_string()), "{moved:?}");
    assert!(moved.contains(&"refs/heads/b".to_string()), "{moved:?}");
}

/// Reverting the restore puts back exactly what it moved (PM: round trip).
#[test]
fn reverting_a_restore_returns_to_before_it() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    commit(&repo, "two\n");
    create(&repo, "b");
    let before_restore = branches(&repo);

    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    backend(&repo).run(&op, &p).unwrap();
    assert_ne!(branches(&repo), before_restore);

    let restore = newest(&repo).id;
    let undo = Operation::OpRevert { entry_id: restore };
    let p = plan(&repo, &undo);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    backend(&repo).run(&undo, &p).unwrap();
    assert_eq!(branches(&repo), before_restore);
    assert_eq!(newest(&repo).op, "op-revert");
}

#[test]
fn reverting_a_middle_operation_keeps_the_later_ones() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    create(&repo, "a");
    let middle = create(&repo, "b");
    create(&repo, "c");

    let op = Operation::OpRevert { entry_id: middle };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);
    backend(&repo).run(&op, &p).unwrap();
    let names = git_output(
        &repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    );
    assert_eq!(names, "a\nc\nmain", "only b's creation was undone");
}

#[test]
fn a_revert_whose_ref_moved_again_later_is_blocked() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let first = commit(&repo, "two\n");
    let later = commit(&repo, "three\n");
    let before = branches(&repo);

    let op = Operation::OpRevert { entry_id: first };
    let p = plan(&repo, &op);
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::LaterEntryMoved {
            refname: "refs/heads/main".into(),
            id: later,
            op: "commit".into(),
        }),
        "{:?}",
        p.blockers
    );
    let err = backend(&repo).run(&op, &p).unwrap_err();
    assert!(
        matches!(err, GitError::Other(_) | GitError::Preflight(_)),
        "{err}"
    );
    assert_eq!(branches(&repo), before, "a blocked plan moves nothing");
}

#[test]
fn an_unrecorded_entry_in_the_range_blocks_the_restore() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    // An entry of this repository written without a record (as before #334
    // slice 2a, or by a path that does not record moves).
    let state = StateSummary {
        head: "branch: main".into(),
        dirty: "clean".into(),
    };
    let unrecorded = OpLogEntry::new(
        "unrecorded",
        repo.display().to_string(),
        state.clone(),
        OpOutcome::Success { after: state },
    )
    .with_worktree(Some(repo.display().to_string()));
    append_oplog(&unrecorded).unwrap();
    let unrecorded_id = newest(&repo).id;
    create(&repo, "b");

    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::NotRecorded {
            id: unrecorded_id,
            op: "unrecorded".into()
        }),
        "{:?}",
        p.blockers
    );
}

#[test]
fn a_checkout_in_the_range_or_a_branch_moved_outside_blocks() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    let checkout = run(&repo, Operation::Checkout { branch: "a".into() });
    let p = plan(&repo, &Operation::RestoreToPoint { entry_id: point });
    assert!(
        restore_blockers(&p).contains(&OplogRestoreNote::HeadMoved {
            id: checkout,
            op: "checkout".into()
        }),
        "{:?}",
        p.blockers
    );

    let tmp = tempfile::tempdir().unwrap();
    let repo = self::repo(tmp.path());
    let created = create(&repo, "a");
    commit_all_on(&repo, "a", "outside\n");
    let p = plan(&repo, &Operation::OpRevert { entry_id: created });
    assert!(
        restore_blockers(&p)
            .iter()
            .any(|n| matches!(n, OplogRestoreNote::RefMovedSince { refname, .. } if refname == "refs/heads/a")),
        "{:?}",
        p.blockers
    );
}

/// Move `branch` with plain git, outside the recorded pipeline.
fn commit_all_on(dir: &Path, branch: &str, content: &str) {
    git(dir, &["checkout", "-q", branch]);
    write_file(dir, "a.txt", content);
    commit_all(dir, "outside");
    git(dir, &["checkout", "-q", "main"]);
}

/// A branch that moves between confirm and execute: preflight re-plans,
/// refuses, and nothing changes.
#[test]
fn a_branch_moved_after_planning_refuses_and_changes_nothing() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let point = create(&repo, "a");
    create(&repo, "b");
    let op = Operation::RestoreToPoint { entry_id: point };
    let p = plan(&repo, &op);
    assert!(p.blockers.is_empty(), "{:?}", p.blockers);

    commit_all_on(&repo, "b", "moved after planning\n");
    let after_move = branches(&repo);
    assert!(backend(&repo).run(&op, &p).is_err());
    assert_eq!(branches(&repo), after_move, "refused before any ref moved");
}

#[test]
fn deleting_a_checked_out_branch_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let created = create(&repo, "a");
    let wt = tmp.path().join("wt-a");
    git(&repo, &["worktree", "add", "-q", wt.to_str().unwrap(), "a"]);
    let p = plan(&repo, &Operation::OpRevert { entry_id: created });
    assert!(
        restore_blockers(&p).iter().any(|n| matches!(
            n,
            OplogRestoreNote::DeletesCheckedOutBranch { branch, .. } if branch == "a"
        )),
        "{:?}",
        p.blockers
    );
}
