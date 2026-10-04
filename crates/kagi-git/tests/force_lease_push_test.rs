//! Integration tests for force-with-lease push (branch-menu "Advanced /
//! Dangerous" group, "Force-with-lease push...").
//!
//! All repositories (local + bare remote) are created inside `TempDir`s. No
//! network access: the "remote" is a local bare repository on disk.

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_force_with_lease_push;
use std::path::{Path, PathBuf};

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{commit_all, git, git_output, init_repo, write_file};

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::{ForceLeaseNote, PlanNote};
use kagi_git::ops::plan_force_with_lease_push;

fn head_sha(dir: &Path) -> String {
    git_output(dir, &["rev-parse", "HEAD"])
}

fn remote_head_sha(remote: &Path, branch: &str) -> String {
    git_output(remote, &["rev-parse", branch])
}

/// Layout: tmp/remote.git (bare) + tmp/local (clone, upstream set on main) +
/// tmp/other (a second clone used to push "from elsewhere").
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
fn test_plan_normal_no_blockers_after_amend() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    // Amend locally so `local`'s HEAD diverges from the remote-tracking ref
    // (the classic force-with-lease scenario).
    write_file(&r.local, "base.txt", "base amended\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);

    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan failed");

    assert!(
        plan.blockers.is_empty(),
        "expected no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.destructive,
        "force-with-lease push must be destructive"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("overwrites the remote branch")),
        "expected a rewrites-history warning, got: {:?}",
        plan.warnings
    );
    if cfg!(windows) {
        assert!(plan.equivalent_command.is_none());
    } else {
        let command = plan.equivalent_command.as_deref().expect("POSIX command");
        assert!(
            command.starts_with("git push '--force-with-lease="),
            "{command}"
        );
        assert!(command.contains(" -- 'origin' 'main'"), "{command}");
    }
}

#[test]
fn test_plan_nothing_to_push_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan failed");
    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::ForceLease(ForceLeaseNote::NothingToPush { .. })
        )),
        "expected ForceLeaseNote::NothingToPush, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_execute_overwrites_remote_after_amend() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();
    write_file(&r.local, "base.txt", "base amended\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);
    let new_local_sha = head_sha(&r.local);

    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan");
    execute_force_with_lease_push(&repo, &r.local, &plan).expect("execute failed");

    assert_eq!(
        remote_head_sha(&r.remote, "main"),
        new_local_sha,
        "remote main should now match the amended local commit"
    );
}

#[test]
fn test_execute_rejects_when_remote_moved_since_last_fetch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();

    // Someone else pushes to the remote via `other`, without `local` ever
    // fetching it.
    write_file(&r.other, "other.txt", "other\n");
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "concurrent change"]);
    git(&r.other, &["push", "-q", "origin", "main"]);
    let concurrent_sha = remote_head_sha(&r.remote, "main");

    // `local` amends its own (now-stale) view of main.
    write_file(&r.local, "base.txt", "base amended locally\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);

    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan");
    let result = execute_force_with_lease_push(&repo, &r.local, &plan);

    assert!(
        result.is_err(),
        "the lease must reject a push when the remote moved since local's last known state"
    );
    assert_eq!(
        remote_head_sha(&r.remote, "main"),
        concurrent_sha,
        "the concurrent change on the remote must survive the rejected push"
    );
}

/// The bug this guard exists for: kagi's own auto-fetch ticker updates
/// `refs/remotes/origin/main` in the background. Execute used to re-read that
/// ref, so a colleague's push that landed while the confirm modal was open got
/// fetched, adopted as the new lease, and then overwritten — with the plan
/// still displaying the old value. The preflight cannot catch it: it compares
/// local HEAD, which a fetch does not move.
#[test]
fn a_fetch_between_plan_and_execute_does_not_refresh_the_lease() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup();

    // Plan while the remote is still where the user can see it.
    write_file(&r.local, "base.txt", "base amended locally\n");
    git(&r.local, &["add", "-A"]);
    git(&r.local, &["commit", "-qam", "--amend", "--no-edit"]);
    let repo = Repository::open(&r.local).expect("open local");
    let plan = plan_force_with_lease_push(&repo).expect("plan");
    assert!(plan.blockers.is_empty(), "plan should be executable");

    // A colleague pushes, and kagi's auto-fetch picks it up before the user
    // presses confirm.
    write_file(&r.other, "other.txt", "other\n");
    git(&r.other, &["add", "-A"]);
    git(&r.other, &["commit", "-qm", "concurrent change"]);
    git(&r.other, &["push", "-q", "origin", "main"]);
    let concurrent_sha = remote_head_sha(&r.remote, "main");
    git(&r.local, &["fetch", "-q", "origin"]);

    let result = execute_force_with_lease_push(&repo, &r.local, &plan);

    assert!(
        result.is_err(),
        "the lease must still be the plan's value, so the push is rejected"
    );
    assert_eq!(
        remote_head_sha(&r.remote, "main"),
        concurrent_sha,
        "the colleague's commit must survive"
    );
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
