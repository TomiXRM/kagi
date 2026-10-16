//! #1132: commit policy is enforced by the user's Git, not skipped by libgit2.
#![cfg(unix)]
use kagi_git::{
    oplog::{read_oplog_tail_for_repo, OpOutcome},
    Backend, Operation,
};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tempfile::TempDir;
#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};
#[path = "../../../tests/support/isolated.rs"]
mod test_support;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    init_repo(dir.path(), "main");
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(dir.path(), &["config", "core.hooksPath", ".git/hooks"]);
    write_file(dir.path(), "a", "base\n");
    commit_all(dir.path(), "base");
    write_file(dir.path(), "a", "approved\n");
    git(dir.path(), &["add", "a"]);
    dir
}
fn hook(repo: &Path, name: &str, script: &str) {
    let path = repo.join(".git/hooks").join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn commit(repo: &Path) -> kagi_git::backend::recording::RunReport {
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::Commit {
        message: "approved message".into(),
    };
    let plan = backend.plan(&op).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    backend.run_recorded(&op, &plan)
}
#[test]
fn commit_hooks_failing_pre_commit_preserves_head_index_and_stderr() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(
        repo,
        "pre-commit",
        "#!/bin/sh\necho ran > hook-ran\necho project-policy-rejected >&2\nexit 1\n",
    );
    let head = git_output(repo, &["rev-parse", "HEAD"]);
    let index = fs::read(repo.join(".git/index")).unwrap();
    let worktree = fs::read(repo.join("a")).unwrap();
    let report = commit(repo);
    assert!(
        report.result.is_err(),
        "rejecting hook must fail, got {:?}",
        report.result
    );
    assert_eq!(git_output(repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(repo.join("a")).unwrap(), worktree);
    assert!(repo.join("hook-ran").exists());
    let tail = read_oplog_tail_for_repo(repo, 10);
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].id, report.recording.entry().id);
    assert!(
        matches!(&tail[0].outcome, OpOutcome::Failed { error } if error.contains("project-policy-rejected"))
    );
}
#[test]
fn commit_hooks_passing_and_post_commit_hooks_run() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(repo, "pre-commit", "#!/bin/sh\necho ran > hook-ran\n");
    hook(repo, "post-commit", "#!/bin/sh\necho ran > post-ran\n");
    let tree = git_output(repo, &["write-tree"]);
    commit(repo).result.unwrap();
    assert!(repo.join("hook-ran").exists());
    assert!(repo.join("post-ran").exists());
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
}
#[test]
fn commit_hooks_commit_msg_rewrites_message() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(
        repo,
        "commit-msg",
        "#!/bin/sh\nprintf 'rewritten by project\\n' > \"$1\"\n",
    );
    commit(repo).result.unwrap();
    assert_eq!(
        git_output(repo, &["log", "-1", "--format=%B"]),
        "rewritten by project"
    );
}
#[test]
fn commit_hooks_respects_disabled_hooks_path() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(
        repo,
        "pre-commit",
        "#!/bin/sh\necho ran > hook-ran\nexit 1\n",
    );
    git(repo, &["config", "core.hooksPath", "/dev/null"]);
    commit(repo).result.unwrap();
    assert!(!repo.join("hook-ran").exists());
}

#[test]
fn commit_hooks_external_index_write_cannot_replace_approved_tree() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    let tree = git_output(repo, &["write-tree"]);
    hook(
        repo,
        "pre-commit",
        "#!/bin/sh\nprintf 'NOT_APPROVED\\n' > a\nGIT_INDEX_FILE=\"$PWD/.git/index\" git add a\n",
    );
    commit(repo).result.unwrap();
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(git_output(repo, &["show", "HEAD:a"]), "approved");
    assert_eq!(
        git_output(repo, &["show", ":a"]),
        "NOT_APPROVED",
        "do not overwrite an external writer's index after the commit"
    );
}

#[test]
fn commit_hooks_private_index_rewrite_is_not_reported_as_success() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(
        repo,
        "pre-commit",
        "#!/bin/sh\nprintf 'NOT_APPROVED\\n' > a\ngit add a\n",
    );
    let index = fs::read(repo.join(".git/index")).unwrap();
    let report = commit(repo);
    assert!(matches!(
        report.result,
        Err(kagi_git::GitError::TerminationUnknown(_))
    ));
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(matches!(
        &report.recording.entry().outcome,
        OpOutcome::Unknown { .. }
    ));
}

#[test]
fn commit_hooks_failing_commit_msg_preserves_head_and_index() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    hook(
        repo,
        "commit-msg",
        "#!/bin/sh\necho message-policy-rejected >&2\nexit 1\n",
    );
    let head = git_output(repo, &["rev-parse", "HEAD"]);
    let index = fs::read(repo.join(".git/index")).unwrap();
    let report = commit(repo);
    assert!(report.result.is_err());
    assert_eq!(git_output(repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert!(
        matches!(&report.recording.entry().outcome, OpOutcome::Failed { error }
        if error.contains("message-policy-rejected"))
    );
}

#[test]
fn commit_hooks_amend_preserves_author_parent_and_message_only_tree() {
    if !test_support::run_isolated() {
        return;
    }
    for mode in [
        kagi_git::AmendMode::MessageOnly,
        kagi_git::AmendMode::Staged,
        kagi_git::AmendMode::Both,
    ] {
        let dir = fixture();
        let repo = dir.path();
        git(
            repo,
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                "second",
                "--author=Original <original@example.com>",
            ],
        );
        write_file(repo, "a", "next staged\n");
        git(repo, &["add", "a"]);
        let tree = if mode.includes_staged() {
            git_output(repo, &["write-tree"])
        } else {
            git_output(repo, &["rev-parse", "HEAD^{tree}"])
        };
        let parent = git_output(repo, &["rev-parse", "HEAD^"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        hook(repo, "pre-commit", "#!/bin/sh\necho ran > hook-ran\n");
        hook(
            repo,
            "commit-msg",
            "#!/bin/sh\nprintf 'amend hook message\\n' > \"$1\"\n",
        );
        let mut backend = Backend::open(repo).unwrap();
        let op = Operation::Amend {
            mode,
            message: Some("amended".into()),
        };
        let plan = backend.plan(&op).unwrap();
        backend.run(&op, &plan).unwrap();
        assert!(repo.join("hook-ran").exists());
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^"]), parent);
        assert_eq!(
            git_output(repo, &["log", "-1", "--format=%an <%ae>"]),
            "Original <original@example.com>"
        );
        assert_eq!(
            git_output(repo, &["log", "-1", "--format=%B"]),
            "amend hook message"
        );
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    }
}

#[test]
fn commit_hooks_resolved_merge_preserves_parents_and_state_on_failure() {
    if !test_support::run_isolated() {
        return;
    }
    for reject in [true, false] {
        let dir = fixture();
        let repo = dir.path();
        commit_all(repo, "base approved");
        git(repo, &["checkout", "-qb", "side"]);
        write_file(repo, "side", "side\n");
        commit_all(repo, "side");
        let side = git_output(repo, &["rev-parse", "HEAD"]);
        git(repo, &["checkout", "-q", "main"]);
        write_file(repo, "main", "main\n");
        commit_all(repo, "main");
        git(repo, &["merge", "--no-commit", "--no-ff", "side"]);
        hook(
            repo,
            "pre-commit",
            if reject {
                "#!/bin/sh\necho merge-policy-rejected >&2\nexit 1\n"
            } else {
                "#!/bin/sh\necho ran > hook-ran\n"
            },
        );
        let head = git_output(repo, &["rev-parse", "HEAD"]);
        let tree = git_output(repo, &["write-tree"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        let merge_head = fs::read(repo.join(".git/MERGE_HEAD")).unwrap();
        let mut backend = Backend::open(repo).unwrap();
        let op = Operation::MergeCommit {
            message: "approved merge".into(),
        };
        let plan = backend.plan(&op).unwrap();
        let report = backend.run_recorded(&op, &plan);
        if reject {
            assert!(report.result.is_err());
            assert_eq!(git_output(repo, &["rev-parse", "HEAD"]), head);
            assert_eq!(fs::read(repo.join(".git/MERGE_HEAD")).unwrap(), merge_head);
            assert!(
                matches!(&report.recording.entry().outcome, OpOutcome::Failed { error }
                if error.contains("merge-policy-rejected"))
            );
        } else {
            report.result.unwrap();
            assert!(repo.join("hook-ran").exists());
            assert!(!repo.join(".git/MERGE_HEAD").exists());
            assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
            assert_eq!(
                git_output(repo, &["log", "-1", "--format=%P"]),
                format!("{head} {side}")
            );
        }
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    }
}

#[test]
fn commit_hooks_linked_worktree_respects_absolute_hooks_path() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    commit_all(repo, "approved base");
    let linked = TempDir::new().unwrap();
    let linked_repo = linked.path().join("linked");
    git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            linked_repo.to_str().unwrap(),
        ],
    );
    hook(repo, "pre-commit", "#!/bin/sh\necho ran > hook-ran\n");
    git(
        repo,
        &[
            "config",
            "core.hooksPath",
            repo.join(".git/hooks").to_str().unwrap(),
        ],
    );
    write_file(&linked_repo, "a", "linked approved\n");
    git(&linked_repo, &["add", "a"]);
    let original_head = git_output(repo, &["rev-parse", "HEAD"]);
    let tree = git_output(&linked_repo, &["write-tree"]);
    commit(&linked_repo).result.unwrap();
    assert!(linked_repo.join("hook-ran").exists());
    assert_eq!(
        git_output(&linked_repo, &["rev-parse", "HEAD^{tree}"]),
        tree
    );
    assert_eq!(git_output(repo, &["rev-parse", "HEAD"]), original_head);
}

#[test]
fn commit_hooks_initial_commit_runs_hooks() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    init_repo(repo, "main");
    git(repo, &["config", "core.hooksPath", ".git/hooks"]);
    hook(repo, "pre-commit", "#!/bin/sh\necho ran > hook-ran\n");
    write_file(repo, "a", "approved\n");
    git(repo, &["add", "a"]);
    let tree = git_output(repo, &["write-tree"]);
    commit(repo).result.unwrap();
    assert!(repo.join("hook-ran").exists());
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(git_output(repo, &["log", "-1", "--format=%P"]), "");
}

#[test]
fn commit_hooks_message_only_amend_can_reword_an_empty_commit() {
    if !test_support::run_isolated() {
        return;
    }
    let dir = fixture();
    let repo = dir.path();
    commit_all(repo, "approved base");
    git(repo, &["commit", "--allow-empty", "-m", "empty"]);
    let tree = git_output(repo, &["rev-parse", "HEAD^{tree}"]);
    let parent = git_output(repo, &["rev-parse", "HEAD^"]);
    let mut backend = Backend::open(repo).unwrap();
    let op = Operation::Amend {
        mode: kagi_git::AmendMode::MessageOnly,
        message: Some("reworded empty".into()),
    };
    let plan = backend.plan(&op).unwrap();
    backend.run(&op, &plan).unwrap();
    assert_eq!(
        git_output(repo, &["log", "-1", "--format=%B"]),
        "reworded empty"
    );
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^"]), parent);
}

#[test]
fn commit_hooks_private_index_preserves_skip_worktree_entries() {
    if !test_support::run_isolated() {
        return;
    }
    for message_only in [false, true] {
        let dir = fixture();
        let repo = dir.path();
        write_file(repo, "sparse.txt", "outside checkout\n");
        commit_all(repo, "sparse base");
        git(repo, &["update-index", "--skip-worktree", "sparse.txt"]);
        fs::remove_file(repo.join("sparse.txt")).unwrap();
        write_file(repo, "a", "next approved\n");
        git(repo, &["add", "a"]);
        let tree = if message_only {
            git_output(repo, &["rev-parse", "HEAD^{tree}"])
        } else {
            git_output(repo, &["write-tree"])
        };
        hook(repo, "pre-commit", "#!/bin/sh\n[ \"$(git ls-files -t sparse.txt)\" = 'S sparse.txt' ] || { echo skip-worktree-lost >&2; exit 1; }\n[ -z \"$(git diff --name-only -- sparse.txt)\" ] || exit 1\n");
        if message_only {
            let mut backend = Backend::open(repo).unwrap();
            let op = Operation::Amend {
                mode: kagi_git::AmendMode::MessageOnly,
                message: Some("metadata-preserving amend".into()),
            };
            let plan = backend.plan(&op).unwrap();
            backend.run(&op, &plan).unwrap();
        } else {
            commit(repo).result.unwrap();
        }
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(
            git_output(repo, &["ls-files", "-t", "sparse.txt"]),
            "S sparse.txt"
        );
        assert!(!repo.join("sparse.txt").exists());
    }
}

#[test]
fn commit_hooks_staged_amend_can_remove_heads_diff() {
    if !test_support::run_isolated() {
        return;
    }
    for mode in [kagi_git::AmendMode::Staged, kagi_git::AmendMode::Both] {
        let dir = fixture();
        let repo = dir.path();
        commit_all(repo, "approved head");
        let parent = git_output(repo, &["rev-parse", "HEAD^"]);
        let tree = git_output(repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
        write_file(repo, "a", "base\n");
        git(repo, &["add", "a"]);
        let index = fs::read(repo.join(".git/index")).unwrap();
        let mut backend = Backend::open(repo).unwrap();
        let op = Operation::Amend {
            mode,
            message: Some("empty amended diff".into()),
        };
        let plan = backend.plan(&op).unwrap();
        backend.run(&op, &plan).unwrap();
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(git_output(repo, &["rev-parse", "HEAD^"]), parent);
        assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    }
}
