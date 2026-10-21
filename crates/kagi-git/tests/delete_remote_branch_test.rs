//! Integration tests for delete-remote-branch (branch-menu "Advanced /
//! Dangerous" group).
//!
//! All repositories (local + bare remote) are created inside `TempDir`s. No
//! network access: the "remote" is a local bare repository on disk.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_delete_remote_branch;
use std::path::Path;

use git2::Repository;

use kagi_domain::plan_note::{CommonNote, PlanNote, RemoteBranchNote};
use kagi_git::ops::plan_delete_remote_branch;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{
    commit_all, git, git_output, repo_with_bare_origin_seed, write_file, RemoteFixture,
};

/// Layout: tmp/remote.git (bare, with `main` + `feature/x`) + tmp/local (clone,
/// fetched so `refs/remotes/origin/feature/x` exists locally).
fn setup() -> RemoteFixture {
    let r = repo_with_bare_origin_seed("main", "base.txt", "base\n", "base");
    git(&r.local, &["checkout", "-qb", "feature/x"]);
    write_file(&r.local, "feat.txt", "feature\n");
    commit_all(&r.local, "feature work");
    git(&r.local, &["push", "-q", "-u", "origin", "feature/x"]);
    git(&r.local, &["checkout", "-q", "main"]);
    r
}

fn remote_has_branch(remote: &Path, branch: &str) -> bool {
    !git_output(
        remote,
        &["ls-remote", "--heads", remote.to_str().unwrap(), branch],
    )
    .is_empty()
}

#[test]
fn test_plan_normal_no_blockers() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    let plan = plan_delete_remote_branch(&repo, "origin/feature/x")
        .expect("plan_delete_remote_branch failed");

    assert!(
        plan.blockers.is_empty(),
        "expected no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.destructive,
        "delete-remote-branch must be marked destructive"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("local branch")),
        "expected a local-branch-untouched warning, got: {:?}",
        plan.warnings
    );
}

#[test]
fn test_plan_not_found_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    let plan = plan_delete_remote_branch(&repo, "origin/does-not-exist")
        .expect("plan_delete_remote_branch failed");

    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::RemoteBranch(RemoteBranchNote::NotFound { remote, branch })
                if remote == "origin" && branch == "does-not-exist"
        )),
        "expected RemoteBranchNote::NotFound, got: {:?}",
        plan.blockers
    );
}

/// A name with no `/` cannot be split into remote + branch, so kagi has no
/// idea what it would be asked to delete. It must block rather than push a
/// guess at a remote.
#[test]
fn test_plan_unsplittable_name_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    let plan = plan_delete_remote_branch(&repo, "just-a-branch")
        .expect("plan_delete_remote_branch failed");

    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Common(CommonNote::GitErrorPassthrough { message })
                if message.contains("is not a <remote>/<branch> name")
        )),
        "expected CommonNote::GitErrorPassthrough, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_execute_deletes_the_remote_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();

    execute_delete_remote_branch(&r.local, "origin/feature/x")
        .expect("execute_delete_remote_branch failed");

    assert!(
        !remote_has_branch(&r.remote, "feature/x"),
        "feature/x should no longer exist on the remote"
    );
    assert!(
        remote_has_branch(&r.remote, "main"),
        "main must be untouched"
    );
}

#[test]
fn test_execute_does_not_touch_local_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();

    execute_delete_remote_branch(&r.local, "origin/feature/x")
        .expect("execute_delete_remote_branch failed");

    let repo = Repository::open(&r.local).expect("open local");
    assert!(
        repo.find_branch("feature/x", git2::BranchType::Local)
            .is_ok(),
        "local branch 'feature/x' must survive a remote-branch delete"
    );

    // HEAD must still be on main — this op never touches HEAD.
    let head_ref = repo.head().expect("repo.head()");
    assert_eq!(head_ref.shorthand().unwrap_or(""), "main");
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
