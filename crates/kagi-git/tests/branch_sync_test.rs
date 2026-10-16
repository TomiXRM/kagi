//! Branch context menu sync/manage operation tests.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::{
    execute_pull_branch_ff, execute_push_branch, execute_rename_branch, execute_set_upstream,
};
use std::path::{Path, PathBuf};

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi_git::{
    plan_pull_branch_ff, plan_push_branch, plan_rename_branch, plan_set_upstream,
    validate_branch_rename, BranchRenameValidation, PullOutcome,
};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};

fn rev_parse(dir: &Path, rev: &str) -> String {
    git_output(dir, &["rev-parse", rev])
}

struct Repos {
    _tmp: TempDir,
    remote: PathBuf,
    local: PathBuf,
    other: PathBuf,
}

fn setup() -> Repos {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    let other = tmp.path().join("other");

    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    std::fs::create_dir(&local).unwrap();
    init_repo(&local, "main");
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    commit_all(&local, "base");
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    git(&local, &["checkout", "-q", "-b", "feature/x"]);
    write_file(&local, "feature.txt", "one\n");
    commit_all(&local, "feature one");
    git(&local, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&local, &["checkout", "-q", "main"]);

    git(
        tmp.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["config", "commit.gpgsign", "false"]);

    Repos {
        _tmp: tmp,
        remote,
        local,
        other,
    }
}

#[test]
fn non_current_pull_ff_updates_ref_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.other, &["checkout", "-q", "feature/x"]);
    write_file(&r.other, "remote.txt", "remote\n");
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "remote feature"]);
    git(&r.other, &["push", "-q", "origin", "feature/x"]);
    git(&r.local, &["fetch", "-q", "origin"]);

    let before_head = rev_parse(&r.local, "HEAD");
    let remote_feature = rev_parse(&r.local, "origin/feature/x");
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x").expect("execute");
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));
    assert_eq!(rev_parse(&r.local, "feature/x"), remote_feature);
    assert_eq!(
        rev_parse(&r.local, "HEAD"),
        before_head,
        "HEAD must not move"
    );
    assert!(
        !r.local.join("remote.txt").exists(),
        "working tree must not change"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct Checkout {
    head: String,
    index: String,
    files: std::collections::BTreeMap<PathBuf, Vec<u8>>,
}

impl Checkout {
    fn read(path: &Path) -> Self {
        fn files(
            root: &Path,
            dir: &Path,
            result: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
        ) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path == root.join(".git") {
                    continue;
                }
                if path.is_dir() {
                    files(root, &path, result);
                } else {
                    result.insert(
                        path.strip_prefix(root).unwrap().into(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut worktree = std::collections::BTreeMap::new();
        files(path, path, &mut worktree);
        Self {
            head: rev_parse(path, "HEAD"),
            index: git_output(path, &["ls-files", "--stage", "-z"]),
            files: worktree,
        }
    }
}

fn advance_feature(r: &Repos) -> String {
    git(&r.other, &["checkout", "-q", "feature/x"]);
    write_file(&r.other, "feature.txt", "updated tracked content\n");
    std::fs::remove_file(r.other.join("base.txt")).unwrap();
    write_file(&r.other, "incoming.txt", "added tracked path\n");
    commit_all(&r.other, "modify add and remove");
    git(&r.other, &["push", "-q", "origin", "feature/x"]);
    rev_parse(&r.other, "HEAD")
}

#[test]
fn current_pull_ff_synchronizes_head_index_and_worktree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "feature/x"]);
    let target = advance_feature(&r);
    git(&r.local, &["fetch", "-q", "origin"]);
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
    assert!(plan.blockers.is_empty());
    let outcome = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x").unwrap();
    assert!(matches!(outcome, PullOutcome::FastForward { .. }));
    assert_eq!(rev_parse(&r.local, "HEAD"), target);
    assert_eq!(Checkout::read(&r.local), Checkout::read(&r.other));
    assert!(git_output(&r.local, &["diff", "--cached", "--name-status"]).is_empty());
    assert!(git_output(&r.local, &["status", "--porcelain"]).is_empty());
}

#[test]
fn current_pull_ff_keeps_dirty_paths_safe_and_accepts_newer_approved_upstream() {
    if !crate::test_support::run_isolated() {
        return;
    }
    for overlapping in [false, true] {
        let r = setup();
        git(&r.local, &["checkout", "-q", "feature/x"]);
        advance_feature(&r);
        git(&r.local, &["fetch", "-q", "origin"]);
        let dirty = if overlapping {
            "feature.txt"
        } else {
            "unrelated.txt"
        };
        write_file(&r.local, dirty, "approved staged local content\n");
        git(&r.local, &["add", dirty]);
        write_file(&r.local, dirty, "approved unstaged local content\n");
        write_file(&r.local, "untracked.txt", "preserve untracked content\n");
        let repo = Repository::open(&r.local).unwrap();
        let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
        assert!(plan.blockers.is_empty());
        let before = Checkout::read(&r.local);
        let staged = git_output(&r.local, &["ls-files", "--stage", "--", dirty]);
        // The same approved tracking ref may discover a newer tip at execution.
        write_file(&r.other, "incoming.txt", "newer content after approval\n");
        commit_all(&r.other, "newer approved upstream");
        git(&r.other, &["push", "-q", "origin", "feature/x"]);
        let outcome = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x");
        if overlapping {
            assert!(
                outcome.is_err(),
                "strict FF must never overwrite dirty paths"
            );
            assert_eq!(Checkout::read(&r.local), before);
        } else {
            assert!(matches!(outcome.unwrap(), PullOutcome::FastForward { .. }));
            assert_eq!(rev_parse(&r.local, "HEAD"), rev_parse(&r.other, "HEAD"));
            assert_eq!(
                std::fs::read(r.local.join("feature.txt")).unwrap(),
                b"updated tracked content\n"
            );
            assert_eq!(
                std::fs::read(r.local.join("incoming.txt")).unwrap(),
                b"newer content after approval\n"
            );
            assert!(!r.local.join("base.txt").exists());
            assert_eq!(
                git_output(&r.local, &["ls-files", "--stage", "--", dirty]),
                staged
            );
            assert_eq!(
                std::fs::read(r.local.join(dirty)).unwrap(),
                b"approved unstaged local content\n"
            );
            assert_eq!(
                std::fs::read(r.local.join("untracked.txt")).unwrap(),
                b"preserve untracked content\n"
            );
        }
        assert!(
            git_output(&r.local, &["stash", "list"]).is_empty(),
            "strict FF is not auto-stash or merge-capable Pull"
        );
    }
}

#[test]
fn pull_ff_refuses_branch_checked_out_in_another_worktree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    advance_feature(&r);
    git(&r.local, &["fetch", "-q", "origin"]);
    let linked = r._tmp.path().join("linked");
    git(
        &r.local,
        &[
            "worktree",
            "add",
            "-q",
            linked.to_str().unwrap(),
            "feature/x",
        ],
    );
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
    let main_before = Checkout::read(&r.local);
    let linked_before = Checkout::read(&linked);
    let refs_before = git_output(&r.local, &["show-ref"]);
    let refused = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x");
    assert!(
        refused.is_err(),
        "an occupied branch must never be advanced ref-only"
    );
    assert!(
        !plan.blockers.is_empty(),
        "occupation must be explained in the plan"
    );
    assert_eq!(Checkout::read(&r.local), main_before);
    assert_eq!(Checkout::read(&linked), linked_before);
    assert_eq!(git_output(&r.local, &["show-ref"]), refs_before);
}

#[test]
fn pull_ff_refuses_worktree_occupation_after_approval_before_fetch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    advance_feature(&r);
    git(&r.local, &["fetch", "-q", "origin"]);
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
    assert!(plan.blockers.is_empty());
    let linked = r._tmp.path().join("linked");
    git(
        &r.local,
        &[
            "worktree",
            "add",
            "-q",
            linked.to_str().unwrap(),
            "feature/x",
        ],
    );
    // A newer remote tip makes an unintended fetch observable, not just ref movement.
    write_file(
        &r.other,
        "newer.txt",
        "not approved to touch the linked checkout\n",
    );
    commit_all(&r.other, "advance again after approval");
    git(&r.other, &["push", "-q", "origin", "feature/x"]);
    let main_before = Checkout::read(&r.local);
    let linked_before = Checkout::read(&linked);
    let refs_before = git_output(&r.local, &["show-ref"]);
    let fetch_before = std::fs::read(repo.path().join("FETCH_HEAD")).unwrap();
    assert!(execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x").is_err());
    assert_eq!(Checkout::read(&r.local), main_before);
    assert_eq!(Checkout::read(&linked), linked_before);
    assert_eq!(git_output(&r.local, &["show-ref"]), refs_before);
    assert_eq!(
        std::fs::read(repo.path().join("FETCH_HEAD")).unwrap(),
        fetch_before
    );
}

#[test]
fn current_pull_ff_refuses_divergence_discovered_after_approval() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "feature/x"]);
    advance_feature(&r);
    git(&r.local, &["fetch", "-q", "origin"]);
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
    assert!(plan.blockers.is_empty());
    let before = Checkout::read(&r.local);
    git(&r.other, &["checkout", "-q", "-b", "divergent", "main"]);
    write_file(&r.other, "divergent.txt", "a sibling history\n");
    commit_all(&r.other, "diverge after approval");
    let divergent = rev_parse(&r.other, "HEAD");
    // Publish the real objects under a new ref before simulating upstream drift.
    git(&r.other, &["push", "-q", "origin", "divergent"]);
    git(
        &r.remote,
        &["update-ref", "refs/heads/feature/x", &divergent],
    );
    assert!(execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x").is_err());
    assert_eq!(rev_parse(&r.local, "origin/feature/x"), divergent);
    assert_eq!(Checkout::read(&r.local), before, "strict FF must not merge");
    assert!(!repo.path().join("MERGE_HEAD").exists());
}

#[cfg(unix)]
#[test]
fn pull_ff_refuses_worktree_occupation_during_fetch() {
    use std::os::unix::fs::PermissionsExt;

    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "feature/x"]);
    let feature_before = Checkout::read(&r.local);
    git(&r.local, &["checkout", "-q", "main"]);
    advance_feature(&r);
    git(&r.local, &["fetch", "-q", "origin"]);
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_pull_branch_ff(&repo, "feature/x").unwrap();
    assert!(plan.blockers.is_empty());
    write_file(&r.other, "newer.txt", "advance while approved\n");
    commit_all(&r.other, "trigger a real fetch before occupation");
    git(&r.other, &["push", "-q", "origin", "feature/x"]);
    let fetched = rev_parse(&r.other, "HEAD");
    let linked = r._tmp.path().join("linked-during-fetch");
    let marker = r._tmp.path().join("fetched-before-occupation");
    // Kagi intentionally disables repository hooks. Use the existing
    // stash_push_cli fixture convention instead: a PATH wrapper delegates to
    // the actual Git binary, then changes occupation before the fetch job
    // returns to the backend. No Git result or transport is synthesized.
    let real_git = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    assert!(real_git.status.success());
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let bin = r._tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    let quote = kagi_domain::remote::shell_quote;
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nreal={}\n\"$real\" \"$@\" || exit $?\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = fetch ]; then\n\
                 unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_INDEX_FILE\n\
                 \"$real\" -C {} rev-parse refs/remotes/origin/feature/x > {} || exit 1\n\
                 \"$real\" -C {} worktree add -q -- {} feature/x || exit 1\n\
                 break\n\
               fi\n\
             done\n",
            quote(real_git.trim()),
            quote(r.local.to_str().unwrap()),
            quote(marker.to_str().unwrap()),
            quote(r.local.to_str().unwrap()),
            quote(linked.to_str().unwrap()),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let before = Checkout::read(&r.local);
    let old_path = std::env::var_os("PATH").unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&old_path));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let result = execute_pull_branch_ff(&repo, &r.local, &plan, "feature/x");
    std::env::set_var("PATH", old_path);
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().trim(),
        fetched,
        "the real fetch completed before occupation changed"
    );
    assert_eq!(rev_parse(&r.local, "origin/feature/x"), fetched);
    assert!(
        linked.exists(),
        "the real fetch job must create the occupancy race before returning"
    );
    assert!(
        result.is_err(),
        "occupation must be checked again after fetch"
    );
    assert_eq!(Checkout::read(&r.local), before);
    assert_eq!(Checkout::read(&linked), feature_before);
    assert_eq!(rev_parse(&r.local, "feature/x"), feature_before.head);
}

#[test]
fn non_current_push_uses_branch_upstream() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "feature/x"]);
    write_file(&r.local, "local.txt", "local\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "local feature"]);
    let feature_tip = rev_parse(&r.local, "feature/x");
    git(&r.local, &["checkout", "-q", "main"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_branch(&repo, "feature/x", false).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_push_branch(&repo, &r.local, &plan, "feature/x", false).expect("push");
    assert_eq!(outcome.pushed, 1);
    assert_eq!(rev_parse(&r.remote, "refs/heads/feature/x"), feature_tip);
    assert_eq!(rev_parse(&r.local, "HEAD"), rev_parse(&r.local, "main"));
}

/// The menu offers plain Push whenever an upstream exists; one that tracks
/// its base (`origin/main`) is published to `origin/<branch>` and moved there.
#[test]
fn non_current_push_of_branch_tracking_base_moves_upstream() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(
        &r.local,
        &[
            "checkout",
            "-q",
            "-b",
            "topic/base",
            "--track",
            "origin/main",
        ],
    );
    write_file(&r.local, "topic.txt", "topic\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qm", "topic"]);
    let tip = rev_parse(&r.local, "topic/base");
    git(&r.local, &["checkout", "-q", "main"]);

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_push_branch(&repo, "topic/base", false).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_push_branch(&repo, &r.local, &plan, "topic/base", false).expect("push");
    assert!(outcome.set_upstream);
    assert_eq!(outcome.pushed, 1);
    assert_eq!(rev_parse(&r.remote, "refs/heads/topic/base"), tip);
    let branch = repo.find_branch("topic/base", BranchType::Local).unwrap();
    assert_eq!(
        branch.upstream().unwrap().name().unwrap(),
        Some("origin/topic/base")
    );
}

#[test]
fn set_upstream_is_config_only() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    git(&r.local, &["checkout", "-q", "-b", "topic/no-upstream"]);
    git(&r.local, &["checkout", "-q", "main"]);
    let before_head = rev_parse(&r.local, "HEAD");

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_set_upstream(&repo, "topic/no-upstream", "origin/main").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_set_upstream(&repo, &plan, "topic/no-upstream", "origin/main").expect("set upstream");

    let branch = repo
        .find_branch("topic/no-upstream", BranchType::Local)
        .unwrap();
    let upstream = branch.upstream().expect("upstream");
    assert_eq!(upstream.name().unwrap().unwrap(), "origin/main");
    assert_eq!(rev_parse(&r.local, "HEAD"), before_head);
}

#[test]
fn rename_current_branch_carries_tracking_config() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_rename_branch(&repo, "main", "trunk").expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_rename_branch(&repo, &plan, "main", "trunk").expect("rename");

    assert!(repo.find_branch("main", BranchType::Local).is_err());
    assert!(repo.find_branch("trunk", BranchType::Local).is_ok());
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "trunk");
    let branch = repo.find_branch("trunk", BranchType::Local).unwrap();
    assert_eq!(
        branch.upstream().unwrap().name().unwrap().unwrap(),
        "origin/main"
    );
}

#[test]
fn branch_rename_validation_is_pure() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let existing = vec!["main".to_string(), "feature/x".to_string()];
    assert_eq!(
        validate_branch_rename("main", "topic/new", &existing),
        BranchRenameValidation::Valid
    );
    assert!(matches!(
        validate_branch_rename("main", "feature/x", &existing),
        BranchRenameValidation::Invalid(_)
    ));
    assert!(matches!(
        validate_branch_rename("main", "bad name.lock", &existing),
        BranchRenameValidation::Invalid(_)
    ));
    assert!(matches!(
        validate_branch_rename("main", " main", &existing),
        BranchRenameValidation::Invalid(_)
    ));
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
