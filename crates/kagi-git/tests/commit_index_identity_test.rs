//! #1126: approval binds commit/amend to staged path, OID, mode and stage.
use std::path::Path;

use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    AmendMode, Backend, GitError, Operation,
};
use tempfile::TempDir;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path(), "main");
    write_file(dir.path(), "a", "base\n");
    commit_all(dir.path(), "base");
    write_file(dir.path(), "other", "second\n");
    commit_all(dir.path(), "second"); // Amend requires a non-root HEAD.
    write_file(dir.path(), "a", "APPROVED\n");
    git(dir.path(), &["add", "a"]);
    dir
}

fn refuse_drift(op: Operation, drift: impl FnOnce(&Path)) {
    let dir = fixture();
    let path = dir.path();
    let mut backend = Backend::open(path).unwrap();
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let head = git_output(path, &["rev-parse", "HEAD"]);
    drift(path);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let worktree = std::fs::read(path.join("a")).unwrap();
    let refs = git_output(path, &["show-ref"]);
    let report = backend.run_recorded(&op, &plan);
    let error = report
        .result
        .expect_err("stale staged content must not be committed");
    assert!(matches!(error, GitError::Preflight(_)), "{error:?}");
    assert!(
        error.to_string().contains("Staged content changed"),
        "{error}"
    );
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
    assert_eq!(std::fs::read(path.join("a")).unwrap(), worktree);
    assert_eq!(
        git_output(path, &["show-ref"]),
        refs,
        "no amend savepoint on refusal"
    );
    let tail = read_oplog_tail_for_repo(path, 10);
    assert_eq!(tail.len(), 1, "one durable refusal receipt");
    assert_eq!(tail[0].id, report.recording.entry().id);
    assert!(
        matches!(&tail[0].outcome, OpOutcome::Failed { error: reason } if reason.contains("Staged content changed"))
    );
}

fn swap_blob(path: &Path) {
    write_file(path, "a", "NOT_APPROVED\n");
    git(path, &["add", "a"]);
}

#[test]
fn commit_index_identity_rejects_same_path_blob_swap() {
    if !test_support::run_isolated() {
        return;
    }
    refuse_drift(
        Operation::Commit {
            message: "approved".into(),
        },
        swap_blob,
    );
}

#[test]
fn commit_index_identity_rejects_added_path() {
    if !test_support::run_isolated() {
        return;
    }
    refuse_drift(
        Operation::Commit {
            message: "approved".into(),
        },
        |path| {
            write_file(path, "new", "NOT_APPROVED\n");
            git(path, &["add", "new"]);
        },
    );
}

#[test]
fn commit_index_identity_rejects_mode_only_change() {
    if !test_support::run_isolated() {
        return;
    }
    refuse_drift(
        Operation::Commit {
            message: "approved".into(),
        },
        |path| {
            git(path, &["update-index", "--chmod=+x", "a"]);
        },
    );
}

#[test]
fn commit_index_identity_rejects_amend_staged_and_both() {
    if !test_support::run_isolated() {
        return;
    }
    for mode in [AmendMode::Staged, AmendMode::Both] {
        refuse_drift(
            Operation::Amend {
                mode,
                message: Some("approved amend".into()),
            },
            swap_blob,
        );
    }
}

#[test]
fn commit_index_identity_rejects_fixup_blob_swap() {
    if !test_support::run_isolated() {
        return;
    }
    // Fixup is the ordinary Commit operation with a fixup! message (ADR-0045).
    refuse_drift(
        Operation::Commit {
            message: "fixup! second".into(),
        },
        swap_blob,
    );
}

#[test]
fn commit_index_identity_no_drift_commits_approved_tree() {
    if !test_support::run_isolated() {
        return;
    }
    for op in [
        Operation::Commit {
            message: "approved".into(),
        },
        Operation::Commit {
            message: "fixup! second".into(),
        },
        Operation::Amend {
            mode: AmendMode::Staged,
            message: None,
        },
        Operation::Amend {
            mode: AmendMode::Both,
            message: Some("approved amend".into()),
        },
    ] {
        let dir = fixture();
        let path = dir.path();
        let mut backend = Backend::open(path).unwrap();
        let index = std::fs::read(path.join(".git/index")).unwrap();
        let op_plan = backend.plan(&op).unwrap();
        assert_eq!(
            std::fs::read(path.join(".git/index")).unwrap(),
            index,
            "planning is read-only"
        );
        let approved_tree = git_output(path, &["write-tree"]);
        // An unstaged edit is not approval drift and must never enter the tree.
        write_file(path, "a", "UNSTAGED\n");
        backend.run(&op, &op_plan).unwrap();
        assert_eq!(
            git_output(path, &["rev-parse", "HEAD^{tree}"]),
            approved_tree
        );
        assert_eq!(git_output(path, &["show", "HEAD:a"]), "APPROVED");
        assert_eq!(
            std::fs::read_to_string(path.join("a")).unwrap(),
            "UNSTAGED\n"
        );
    }
}

#[test]
fn commit_index_identity_missing_approval_is_refused() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    let mut backend = Backend::open(path).unwrap();
    let op = Operation::Commit {
        message: "approved".into(),
    };
    let mut plan = backend.plan(&op).unwrap();
    plan.approved_index_digest = None;
    let head = git_output(path, &["rev-parse", "HEAD"]);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let error = backend.run(&op, &plan).unwrap_err();
    assert!(matches!(error, GitError::Preflight(_)));
    assert!(error.to_string().contains("Staged content changed"));
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
}

#[test]
fn commit_index_identity_message_only_amend_keeps_old_tree() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    let mut backend = Backend::open(path).unwrap();
    let op = Operation::Amend {
        mode: AmendMode::MessageOnly,
        message: Some("reword".into()),
    };
    let plan = backend.plan(&op).unwrap();
    let old_tree = git_output(path, &["rev-parse", "HEAD^{tree}"]);
    swap_blob(path);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    backend.run(&op, &plan).unwrap();
    assert_eq!(git_output(path, &["rev-parse", "HEAD^{tree}"]), old_tree);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
}
