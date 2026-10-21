//! Integration tests for fetch-remote-branch (branch-menu "Sync" group,
//! "Fetch remote branch").
//!
//! All repositories (local + bare remote) are created inside `TempDir`s. No
//! network access: the "remote" is a local bare repository on disk.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::fetch_remote_branch;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, repo_with_bare_origin_seed, write_file, RemoteFixture};

use git2::Repository;

/// Layout: tmp/remote.git (bare, `main` only at setup) + tmp/local (clone,
/// tracks only `main`) + tmp/other (a second clone used to push `feature/x`
/// "from elsewhere" without `local` ever fetching it).
fn setup() -> RemoteFixture {
    let r = repo_with_bare_origin_seed("main", "base.txt", "base\n", "base");
    let other = r.clone_other();
    git(&other, &["checkout", "-qb", "feature/x"]);
    write_file(&other, "feat.txt", "feature\n");
    commit_all(&other, "feature work");
    git(&other, &["push", "-q", "-u", "origin", "feature/x"]);
    r
}

#[test]
fn test_fetch_creates_the_remote_tracking_ref() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    assert!(
        repo.find_reference("refs/remotes/origin/feature/x")
            .is_err(),
        "local must not have fetched feature/x yet"
    );

    let outcome = fetch_remote_branch(&repo, &r.local, "origin", "feature/x")
        .expect("fetch_remote_branch failed");

    assert_eq!(outcome.remote, "origin");
    assert!(
        outcome.changed,
        "first fetch of a new branch must report changed"
    );

    let repo2 = Repository::open(&r.local).expect("re-open local");
    assert!(
        repo2
            .find_reference("refs/remotes/origin/feature/x")
            .is_ok(),
        "refs/remotes/origin/feature/x should exist after fetch"
    );
}

#[test]
fn test_fetch_is_noop_when_nothing_moved() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    fetch_remote_branch(&repo, &r.local, "origin", "feature/x").expect("first fetch failed");

    let repo2 = Repository::open(&r.local).expect("re-open local");
    let outcome =
        fetch_remote_branch(&repo2, &r.local, "origin", "feature/x").expect("second fetch failed");

    assert!(
        !outcome.changed,
        "unchanged remote should report changed=false"
    );
}

#[test]
fn test_fetch_never_moves_head_or_local_branches() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");

    fetch_remote_branch(&repo, &r.local, "origin", "feature/x").expect("fetch failed");

    let repo2 = Repository::open(&r.local).expect("re-open local");
    let head_ref = repo2.head().expect("repo.head()");
    assert_eq!(head_ref.shorthand().unwrap_or(""), "main");
    assert!(
        repo2
            .find_branch("feature/x", git2::BranchType::Local)
            .is_err(),
        "fetch must not create a local branch"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
