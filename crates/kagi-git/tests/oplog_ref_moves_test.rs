//! #334 slice 2a (ADR-0214 §4): every write through `Backend::run` records
//! the refs it moved, by OID — HEAD of the worktree it ran in and every
//! `refs/heads/*` — whatever the outcome.

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

use kagi_domain::plan_note::{BranchRecovery, RecoveryKind};
use kagi_domain::ref_moves::RefMove;
use kagi_git::oplog::{read_oplog_tail_for_repo, OpLogEntry, OpOutcome};
use kagi_git::{Backend, CommitId, Operation};
use std::path::{Path, PathBuf};

fn repo(tmp: &Path) -> PathBuf {
    let repo = tmp.join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo, "main");
    write_file(&repo, "a.txt", "a\n");
    commit_all(&repo, "base");
    repo
}

fn tip(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
}

/// Plan and run `op` through the recorded pipeline; the newest entry.
fn run(dir: &Path, op: Operation) -> OpLogEntry {
    let mut backend = Backend::open(dir).unwrap();
    backend.set_auto_snapshot(false);
    let plan = backend.plan(&op).unwrap();
    let _ = backend.run(&op, &plan);
    read_oplog_tail_for_repo(dir, 1)
        .pop()
        .expect("the run was recorded")
}

#[test]
fn recorded_create_branch_retains_approved_recovery_kind_and_commands() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let mut backend = Backend::open(&repo).unwrap();
    backend.set_auto_snapshot(false);
    let op = Operation::CreateBranch {
        name: "new-feature".into(),
        at: CommitId(tip(&repo, "HEAD")),
    };
    let plan = backend.plan(&op).unwrap();
    let approved = plan.recovery.clone().expect("plan has recovery");
    let report = backend.run_recorded(&op, &plan);
    assert!(matches!(
        report.recording.entry().outcome,
        OpOutcome::Success { .. }
    ));
    assert!(matches!(
        approved.kind,
        RecoveryKind::Branch(BranchRecovery::CreateBranch { .. })
    ));
    assert!(!approved.commands.is_empty(), "command was approved");
    let recorded = read_oplog_tail_for_repo(&repo, 1).pop().unwrap();
    assert_eq!(recorded.recovery_plan, Some(approved));
}

fn moved(refname: &str, old: Option<&str>, new: Option<&str>) -> RefMove {
    RefMove {
        refname: refname.into(),
        old: old.map(Into::into),
        new: new.map(Into::into),
        old_symbolic: None,
        new_symbolic: None,
    }
}

#[test]
fn a_checkout_records_heads_new_target_and_no_branch() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    git(&repo, &["branch", "feature"]);
    let head = tip(&repo, "HEAD");

    let entry = run(
        &repo,
        Operation::Checkout {
            branch: "feature".into(),
        },
    );
    assert!(matches!(entry.outcome, OpOutcome::Success { .. }));
    assert_eq!(
        entry.ref_moves,
        Some(vec![RefMove {
            refname: "HEAD".into(),
            old: Some(head.clone()),
            new: Some(head),
            old_symbolic: Some("refs/heads/main".into()),
            new_symbolic: Some("refs/heads/feature".into()),
        }])
    );
}

#[test]
fn a_commit_records_heads_commit_and_its_branch() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    let before = tip(&repo, "HEAD");
    write_file(&repo, "a.txt", "changed\n");
    git(&repo, &["add", "a.txt"]);

    let entry = run(
        &repo,
        Operation::Commit {
            message: "change".into(),
        },
    );
    assert!(
        matches!(entry.outcome, OpOutcome::Success { .. }),
        "{:?}",
        entry.outcome
    );
    let after = tip(&repo, "HEAD");
    assert_ne!(before, after);
    assert_eq!(
        entry.ref_moves,
        Some(vec![
            RefMove {
                refname: "HEAD".into(),
                old: Some(before.clone()),
                new: Some(after.clone()),
                old_symbolic: Some("refs/heads/main".into()),
                new_symbolic: Some("refs/heads/main".into()),
            },
            moved("refs/heads/main", Some(&before), Some(&after)),
        ])
    );
}

/// The branch is checked out in another worktree: its move is the record; the
/// other worktree's HEAD follows it and is not read separately.
#[test]
fn a_replay_records_only_the_replayed_branch() {
    if !test_support::run_isolated() || !kagi_git::cli::GitFeatures::detected().replay_onto {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let main = repo(tmp.path());
    git(&main, &["branch", "feat"]);
    write_file(&main, "c.txt", "main2\n");
    commit_all(&main, "main2");
    let wt = tmp.path().join("wt-feat");
    git(&main, &["worktree", "add", wt.to_str().unwrap(), "feat"]);
    write_file(&wt, "b.txt", "feat1\n");
    commit_all(&wt, "feat1");
    let feat_before = tip(&main, "feat");

    let entry = run(
        &main,
        Operation::ReplayOnto {
            branch: "feat".into(),
            onto: "main".into(),
        },
    );
    assert!(
        matches!(entry.outcome, OpOutcome::Success { .. }),
        "{:?}",
        entry.outcome
    );
    let feat_after = tip(&main, "feat");
    assert_ne!(feat_before, feat_after);
    assert_eq!(
        entry.ref_moves,
        Some(vec![moved(
            "refs/heads/feat",
            Some(&feat_before),
            Some(&feat_after)
        )])
    );
}

/// A refused or failed attempt is still a record: of nothing having moved.
#[test]
fn a_failed_attempt_records_that_nothing_moved() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    git(&repo, &["branch", "taken"]);
    let at = CommitId(tip(&repo, "HEAD"));

    let entry = run(
        &repo,
        Operation::CreateBranch {
            name: "taken".into(),
            at,
        },
    );
    assert!(
        !matches!(entry.outcome, OpOutcome::Success { .. }),
        "creating an existing branch must not succeed: {:?}",
        entry.outcome
    );
    assert_eq!(entry.ref_moves, Some(Vec::new()));
}

/// Only writes through the recorded pipeline are recorded: a ref moved by
/// plain git in between is in no entry, and is not attributed to the next one.
#[test]
fn a_move_outside_the_pipeline_is_not_in_the_next_entry() {
    if !test_support::run_isolated() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo(tmp.path());
    git(&repo, &["branch", "outside"]);
    git(&repo, &["branch", "feature"]);

    let entry = run(
        &repo,
        Operation::Checkout {
            branch: "feature".into(),
        },
    );
    let names: Vec<String> = entry
        .ref_moves
        .unwrap()
        .into_iter()
        .map(|m| m.refname)
        .collect();
    assert_eq!(
        names,
        vec!["HEAD"],
        "the earlier `git branch` is not this op's"
    );
}
