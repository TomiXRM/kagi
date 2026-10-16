//! #1126: approval binds commit/amend to staged path, OID, mode and stage.
use std::path::Path;

use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    AmendMode, Backend, GitError, Operation,
};
use tempfile::TempDir;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, git_succeeds, init_repo, write_file};
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

fn remove_effective_identity(path: &Path) {
    // These tests run in a dedicated child; no sibling shares its environment.
    for key in [
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "EMAIL",
    ] {
        std::env::remove_var(key);
    }
    std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    git(path, &["config", "--local", "user.name", ""]);
    git(path, &["config", "--local", "user.email", ""]);
    git(path, &["config", "--local", "user.useConfigOnly", "true"]);
}

#[test]
fn commit_missing_effective_identity_blocks_without_writes() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    remove_effective_identity(path);
    let mut backend = Backend::open(path).unwrap();
    let op = Operation::Commit {
        message: "approved".into(),
    };
    let head = git_output(path, &["rev-parse", "HEAD"]);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let plan = backend.plan(&op).unwrap();
    assert!(plan
        .blockers
        .contains(&kagi_domain::plan_note::PlanNote::Commit(
            kagi_domain::plan_note::commit::CommitNote::IdentityUnavailable,
        )));
    assert!(read_oplog_tail_for_repo(path, 10).is_empty());
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
    let report = backend.run_recorded(&op, &plan);
    assert!(report.result.is_err());
    assert_eq!(read_oplog_tail_for_repo(path, 10).len(), 1);
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
    git(path, &["config", "--local", "user.name", "Explicit Test"]);
    git(
        path,
        &["config", "--local", "user.email", "explicit@example.com"],
    );
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(backend.run_recorded(&op, &plan).result.is_ok());
    assert_ne!(git_output(path, &["rev-parse", "HEAD"]), head);
}

#[test]
fn commit_identity_is_rechecked_after_approval() {
    if !test_support::run_isolated() {
        return;
    }
    for op in [
        Operation::Commit {
            message: "approved".into(),
        },
        Operation::Amend {
            mode: AmendMode::MessageOnly,
            message: Some("reword".into()),
        },
        Operation::Amend {
            mode: AmendMode::Staged,
            message: None,
        },
        Operation::Amend {
            mode: AmendMode::Both,
            message: Some("amended".into()),
        },
    ] {
        let dir = fixture();
        let path = dir.path();
        let mut backend = Backend::open(path).unwrap();
        let plan = backend.plan(&op).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        remove_effective_identity(path);
        let head = git_output(path, &["rev-parse", "HEAD"]);
        let refs = git_output(path, &["show-ref"]);
        let index = std::fs::read(path.join(".git/index")).unwrap();
        let error = backend.run_recorded(&op, &plan).result.unwrap_err();
        assert!(matches!(error, GitError::Preflight(_)), "{error:?}");
        assert!(error.to_string().contains("user.name and user.email"));
        assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
        assert_eq!(git_output(path, &["show-ref"]), refs);
        assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
        assert_eq!(read_oplog_tail_for_repo(path, 10).len(), 1);
    }
}

#[test]
fn commit_effective_environment_identity_is_accepted() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    remove_effective_identity(path);
    std::env::set_var("GIT_AUTHOR_NAME", "Environment Author");
    std::env::set_var("GIT_AUTHOR_EMAIL", "author@example.com");
    std::env::set_var("GIT_COMMITTER_NAME", "Environment Committer");
    std::env::set_var("GIT_COMMITTER_EMAIL", "committer@example.com");
    let mut backend = Backend::open(path).unwrap();
    let op = Operation::Commit {
        message: "environment identity".into(),
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(backend.run_recorded(&op, &plan).result.is_ok());
    assert_eq!(
        git_output(
            path,
            &["show", "-s", "--format=%an <%ae>|%cn <%ce>", "HEAD"]
        )
        .trim(),
        "Environment Author <author@example.com>|Environment Committer <committer@example.com>",
    );
}

fn refuse_drift(op: Operation, drift: impl FnOnce(&Path)) {
    refuse_drift_in(fixture(), op, drift);
}

fn refuse_drift_in(dir: TempDir, op: Operation, drift: impl FnOnce(&Path)) {
    let path = dir.path();
    let mut backend = Backend::open(path).unwrap();
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let head = git_output(path, &["rev-parse", "HEAD"]);
    drift(path);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let worktree = std::fs::read(path.join("a")).unwrap();
    let refs = git_output(path, &["show-ref"]);
    let merge_head = std::fs::read(path.join(".git/MERGE_HEAD")).ok();
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
    assert_eq!(std::fs::read(path.join(".git/MERGE_HEAD")).ok(), merge_head);
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

#[test]
fn commit_index_identity_rejects_resolved_merge_blob_swap() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    commit_all(path, "approved base");
    git(path, &["checkout", "-q", "-b", "side"]);
    write_file(path, "a", "SIDE\n");
    commit_all(path, "side");
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "a", "MAIN\n");
    commit_all(path, "main");
    assert!(!git_succeeds(path, &["merge", "--no-commit", "side"]));
    write_file(path, "a", "APPROVED\n");
    git(path, &["add", "a"]);
    assert!(path.join(".git/MERGE_HEAD").exists());
    refuse_drift_in(
        dir,
        Operation::MergeCommit {
            message: "approved merge".into(),
        },
        swap_blob,
    );
}

fn checkout_state(path: &Path) -> (String, String, String, String, Vec<u8>) {
    (
        git_output(path, &["symbolic-ref", "HEAD"]),
        git_output(path, &["show-ref"]),
        git_output(path, &["ls-files", "--stage"]),
        git_output(path, &["status", "--porcelain", "--untracked-files=all"]),
        std::fs::read(path.join("a")).unwrap(),
    )
}

fn assert_generic_identity_refusal(path: &Path, op: Operation) {
    let mut backend = Backend::open(path).unwrap();
    let before = checkout_state(path);
    let plan = backend
        .plan(&op)
        .expect("missing identity must yield a plan");
    assert!(
        plan.blockers
            .contains(&kagi_domain::plan_note::PlanNote::Common(
                kagi_domain::plan_note::CommonNote::GitIdentityUnavailable,
            )),
        "{:?}",
        plan.blockers
    );
    assert_eq!(checkout_state(path), before, "planning changed checkout");
    assert!(backend.list_snapshots().unwrap().is_empty());
    assert!(read_oplog_tail_for_repo(path, 10).is_empty());
    let report = backend.run_recorded(&op, &plan);
    assert!(
        matches!(&report.result, Err(GitError::Other(reason)) if reason == "plan has blockers"),
        "{:?}",
        report.result,
    );
    assert_eq!(checkout_state(path), before, "refusal changed checkout");
    assert!(backend.list_snapshots().unwrap().is_empty());
    assert_eq!(read_oplog_tail_for_repo(path, 10).len(), 1);
    git(path, &["config", "--local", "user.name", "Explicit Test"]);
    git(
        path,
        &["config", "--local", "user.email", "explicit@example.com"],
    );
    let approved = backend.plan(&op).unwrap();
    assert!(approved.blockers.is_empty(), "{:?}", approved.blockers);
    remove_effective_identity(path);
    let report = backend.run_recorded(&op, &approved);
    assert!(
        matches!(&report.result, Err(GitError::Preflight(_))),
        "{:?}",
        report.result
    );
    assert_eq!(
        checkout_state(path),
        before,
        "late refusal changed checkout"
    );
    assert!(backend.list_snapshots().unwrap().is_empty());
    assert_eq!(read_oplog_tail_for_repo(path, 10).len(), 2);
}

#[test]
fn merge_branch_missing_identity_blocks_before_checkout() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    commit_all(path, "approved base");
    git(path, &["checkout", "-q", "-b", "side"]);
    write_file(path, "side", "side work\n");
    commit_all(path, "side work");
    git(path, &["checkout", "-q", "main"]);
    write_file(path, "main", "main work\n");
    commit_all(path, "main work");
    remove_effective_identity(path);
    assert_generic_identity_refusal(
        path,
        Operation::MergeBranch {
            target: "side".into(),
        },
    );
    assert!(
        !path.join("side").exists(),
        "merge checked out the source tree"
    );
    assert!(!path.join(".git/MERGE_HEAD").exists());
}

#[test]
fn sync_to_remote_missing_identity_blocks_before_checkout() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    commit_all(path, "local work");
    let remote = TempDir::new().unwrap();
    let origin = remote.path();
    git(path, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    git(path, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(path, &["push", "-q", "-u", "origin", "main"]);
    write_file(path, "a", "upstream work\n");
    commit_all(path, "upstream work");
    git(path, &["push", "-q", "origin", "main"]);
    git(path, &["checkout", "-q", "-b", "local", "HEAD~1"]);
    git(path, &["branch", "--set-upstream-to=origin/main", "local"]);
    write_file(path, "a", "precious local edits\n");
    remove_effective_identity(path);
    assert_generic_identity_refusal(
        path,
        Operation::SyncToRemote {
            branch: "local".into(),
        },
    );
}

#[test]
fn stash_push_missing_identity_yields_generic_blocker() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    write_file(path, "untracked", "precious untracked content\n");
    remove_effective_identity(path);
    assert_generic_identity_refusal(
        path,
        Operation::StashPush {
            message: Some("save local edits".into()),
            include_untracked: true,
        },
    );
    assert_eq!(
        std::fs::read_to_string(path.join("untracked")).unwrap(),
        "precious untracked content\n"
    );
    assert!(!git_succeeds(
        path,
        &["rev-parse", "--verify", "refs/stash"]
    ));
}

fn assert_internal_snapshot_signature(path: &Path, id: &str) {
    let reference = format!("refs/kagi/snapshots/{id}");
    assert_eq!(
        git_output(
            path,
            &["show", "-s", "--format=%an <%ae>|%cn <%ce>", &reference]
        ),
        "kagi <kagi@local>|kagi <kagi@local>"
    );
}

#[test]
fn create_snapshot_without_git_identity_uses_internal_signature() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    remove_effective_identity(path);
    let backend = Backend::open(path).unwrap();
    let before = checkout_state(path);
    let index = std::fs::read(path.join(".git/index")).unwrap();
    let head = git_output(path, &["rev-parse", "HEAD"]);
    let saved = backend.create_snapshot("identity-free savepoint").unwrap();
    assert_internal_snapshot_signature(path, &saved.id);
    assert_eq!(
        git_output(
            path,
            &["show", &format!("refs/kagi/snapshots/{}:a", saved.id)]
        ),
        "APPROVED"
    );
    let after = checkout_state(path);
    assert_eq!(after.0, before.0);
    assert_eq!(after.2, before.2);
    assert_eq!(after.3, before.3);
    assert_eq!(after.4, before.4);
    assert_eq!(std::fs::read(path.join(".git/index")).unwrap(), index);
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
    assert_eq!(backend.list_snapshots().unwrap().len(), 1);
}

#[test]
fn auto_snapshot_without_git_identity_preserves_edits_before_discard() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let path = dir.path();
    commit_all(path, "approved base");
    write_file(path, "a", "precious local edits\n");
    remove_effective_identity(path);
    let mut backend =
        Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(true)).unwrap();
    let op = Operation::Discard {
        paths: vec!["a".into()],
    };
    let head = git_output(path, &["rev-parse", "HEAD"]);
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    assert!(plan.destructive);
    backend.run(&op, &plan).unwrap();
    let saved = backend.list_snapshots().unwrap();
    assert_eq!(saved.len(), 1);
    assert_internal_snapshot_signature(path, &saved[0].id);
    assert_eq!(
        git_output(
            path,
            &["show", &format!("refs/kagi/snapshots/{}:a", saved[0].id)]
        ),
        "precious local edits"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("a")).unwrap(),
        "APPROVED\n"
    );
    assert_eq!(git_output(path, &["rev-parse", "HEAD"]), head);
}
