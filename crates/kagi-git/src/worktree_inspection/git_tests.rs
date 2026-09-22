//! Removal-fact observation tests (#633).
//!
//! Every case here feeds a snapshot row whose fields are wrong on purpose, so
//! any fact taken from the caller instead of from the repository shows up as a
//! wrong answer.

use super::*;
use kagi_domain::remove::{worktree_removal_verdict, WorktreeRemovalVerdict};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("spawn git")
        .success();
    assert!(ok, "git {args:?} failed");
}

/// A row whose every field except `path` is a lie, in the worst direction:
/// claims the main worktree, claims clean (`wip: None`), claims unlocked.
fn misleading_row(path: &Path) -> Worktree {
    Worktree {
        name: "not-the-registry-name".to_string(),
        path: path.to_path_buf(),
        branch: Some("not-the-branch".to_string()),
        is_current: false,
        is_main: true,
        wip: None,
        head: Some(CommitId("0".repeat(40))),
        locked: false,
        lock_reason: None,
    }
}

fn inspect(repo: &Path, worktree: &Path) -> WorktreeInspection {
    inspect_worktree(repo, &misleading_row(worktree), &AtomicBool::new(false))
}

/// Main repo with one commit on `main`, plus a linked worktree on `feat` whose
/// commit is already in `main`.
fn merged_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let base = tempfile::tempdir().unwrap();
    let main = base.path().join("main");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main", "."]);
    std::fs::write(main.join("README.md"), "x\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "init"]);
    let wt = base.path().join("wt");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
    );
    (base, main, wt)
}

#[test]
fn a_clean_linked_worktree_on_mains_commit_is_provably_safe() {
    let (_base, main, wt) = merged_fixture();
    let seen = inspect(&main, &wt);

    assert!(
        !seen.removal.is_main,
        "a linked worktree must not inherit the snapshot's is_main claim"
    );
    assert_eq!(seen.removal.clean, WorktreeEvidence::Yes);
    assert_eq!(seen.removal.unlocked, WorktreeEvidence::Yes);
    assert_eq!(seen.removal.merged, WorktreeEvidence::Yes);
    // No upstream is configured: a positive observation, not ignorance.
    assert_eq!(seen.removal.pushed, WorktreeEvidence::No);
    assert_eq!(seen.default_branch.as_deref(), Some("main"));
    assert!(seen.head.is_some());
    assert!(
        seen.errors.is_empty(),
        "unexpected errors: {:?}",
        seen.errors
    );
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::SafeMerged
    );
}

/// The registry is reachable from any worktree of the repository, and the main
/// worktree identifies itself even with linked worktrees registered.
#[test]
fn identity_resolves_from_either_side_of_the_registry() {
    let (_base, main, wt) = merged_fixture();

    let from_linked = inspect(&wt, &wt);
    assert!(!from_linked.removal.is_main);
    assert_eq!(from_linked.removal.merged, WorktreeEvidence::Yes);

    let main_seen = inspect(&wt, &main);
    assert!(main_seen.removal.is_main);
    assert_eq!(main_seen.removal.clean, WorktreeEvidence::Yes);
    assert!(main_seen.disk_usage.is_ok());
    assert_eq!(
        worktree_removal_verdict(&main_seen.removal),
        WorktreeRemovalVerdict::MainWorktree,
        "the main worktree is excluded regardless of every other fact"
    );
}

#[test]
fn an_untracked_file_makes_it_dirty_even_when_the_snapshot_says_otherwise() {
    let (_base, main, wt) = merged_fixture();
    std::fs::write(wt.join("scratch.txt"), "work in progress\n").unwrap();

    let seen = inspect(&main, &wt);
    assert_eq!(seen.removal.clean, WorktreeEvidence::No);
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::Dirty
    );
}

#[test]
fn ignored_build_output_does_not_make_it_dirty() {
    let (_base, main, wt) = merged_fixture();
    std::fs::write(wt.join(".gitignore"), "target/\n").unwrap();
    git(&wt, &["add", ".gitignore"]);
    git(&wt, &["commit", "-qm", "ignore target"]);
    std::fs::create_dir(wt.join("target")).unwrap();
    std::fs::write(wt.join("target").join("big.bin"), vec![0u8; 64 * 1024]).unwrap();

    let seen = inspect(&main, &wt);
    assert_eq!(
        seen.removal.clean,
        WorktreeEvidence::Yes,
        "ignored build output is the occupancy answer, not a dirty working tree"
    );
    assert!(seen.disk_usage.unwrap().target_bytes > 0);
}

#[test]
fn a_locked_worktree_is_observed_locked_not_read_from_the_snapshot() {
    let (_base, main, wt) = merged_fixture();
    git(&main, &["worktree", "lock", wt.to_str().unwrap()]);

    let seen = inspect(&main, &wt);
    assert_eq!(seen.removal.unlocked, WorktreeEvidence::No);
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::Locked
    );
}

#[test]
fn a_detached_head_can_be_merged_but_never_pushed() {
    let (_base, main, wt) = merged_fixture();
    git(&wt, &["checkout", "-q", "--detach"]);

    let seen = inspect(&main, &wt);
    assert_eq!(seen.removal.merged, WorktreeEvidence::Yes);
    assert_eq!(
        seen.removal.pushed,
        WorktreeEvidence::No,
        "a detached HEAD has no branch, so it can have no upstream"
    );
}

#[test]
fn an_unborn_head_is_unknown_with_the_unborn_reason() {
    let base = tempfile::tempdir().unwrap();
    let main = base.path().join("empty");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main", "."]);

    let seen = inspect(&main, &main);
    assert!(seen.removal.is_main);
    assert_eq!(seen.head, None);
    assert_eq!(
        seen.removal.merged,
        WorktreeEvidence::Unknown(WorktreeUnknownReason::Unborn)
    );
    assert_eq!(
        seen.removal.pushed,
        WorktreeEvidence::Unknown(WorktreeUnknownReason::Unborn)
    );
    assert_eq!(seen.default_branch, None);
}

#[test]
fn a_configured_upstream_whose_ref_is_missing_is_unavailable_not_absent() {
    let (_base, main, wt) = merged_fixture();
    git(&wt, &["config", "branch.feat.remote", "origin"]);
    git(&wt, &["config", "branch.feat.merge", "refs/heads/feat"]);
    git(
        &wt,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/r.git",
        ],
    );
    git(
        &wt,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    );

    let seen = inspect(&main, &wt);
    assert_eq!(
        seen.removal.pushed,
        WorktreeEvidence::Unknown(WorktreeUnknownReason::UpstreamUnavailable),
        "a lookup that failed must never become evidence of No"
    );
    assert!(
        seen.errors.iter().any(|e| e.contains("remote-tracking")),
        "the real detail is kept for diagnostics: {:?}",
        seen.errors
    );
}

#[test]
fn a_local_dot_upstream_is_never_proof_of_publication() {
    let (_base, main, wt) = merged_fixture();
    git(&wt, &["config", "branch.feat.remote", "."]);
    git(&wt, &["config", "branch.feat.merge", "refs/heads/main"]);

    let seen = inspect(&main, &wt);
    assert_eq!(seen.removal.pushed, WorktreeEvidence::No);
}

#[test]
fn pushed_follows_the_remote_tracking_ref_and_goes_back_to_no_when_ahead() {
    let (base, main, wt) = merged_fixture();
    let remote = base.path().join("remote.git");
    git(&main, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(&wt, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&wt, &["push", "-q", "-u", "origin", "feat"]);

    let seen = inspect(&main, &wt);
    assert_eq!(seen.removal.pushed, WorktreeEvidence::Yes);
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::SafeMerged,
        "merged proof outranks pushed proof"
    );

    std::fs::write(wt.join("new.txt"), "unpublished\n").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "-qm", "ahead of origin"]);

    let ahead = inspect(&main, &wt);
    assert_eq!(
        ahead.removal.pushed,
        WorktreeEvidence::No,
        "a commit the remote-tracking ref does not contain is not pushed"
    );
    assert_eq!(ahead.removal.merged, WorktreeEvidence::No);
    assert_eq!(
        worktree_removal_verdict(&ahead.removal),
        WorktreeRemovalVerdict::Unpublished
    );
}

#[test]
fn a_path_that_is_not_a_registered_worktree_is_refused_and_never_reads_safe() {
    let (base, main, _wt) = merged_fixture();
    let stranger = base.path().join("stranger");
    std::fs::create_dir(&stranger).unwrap();

    let seen = inspect(&main, &stranger);
    let failed = WorktreeEvidence::Unknown(WorktreeUnknownReason::ObservationFailed);
    assert_eq!(seen.removal.clean, failed);
    assert_eq!(seen.removal.unlocked, failed);
    assert_eq!(seen.removal.merged, failed);
    assert_eq!(seen.removal.pushed, failed);
    assert!(seen.disk_usage.is_err());
    assert!(!seen.errors.is_empty());
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::Unknown(WorktreeUnknownReason::ObservationFailed)
    );
}

#[test]
fn inspection_does_not_write_to_the_repository() {
    let (_base, main, wt) = merged_fixture();
    // Make the stat cache look stale, which is what tempts a status read into
    // writing the index back.
    std::fs::write(wt.join("README.md"), "x\n").unwrap();
    let index = main.join(".git").join("worktrees").join("wt").join("index");
    let before = std::fs::metadata(&index).unwrap();

    let seen = inspect(&main, &wt);
    assert!(seen.disk_usage.is_ok());

    let after = std::fs::metadata(&index).unwrap();
    assert_eq!(
        (after.modified().unwrap(), after.len()),
        (before.modified().unwrap(), before.len()),
        "an inspection pass must not rewrite the index"
    );
}

#[test]
fn a_cancelled_pass_reports_unknown_and_no_size() {
    let (_base, main, wt) = merged_fixture();
    let seen = inspect_worktree(&main, &misleading_row(&wt), &AtomicBool::new(true));

    assert_eq!(
        seen.removal.clean,
        WorktreeEvidence::Unknown(WorktreeUnknownReason::ObservationFailed),
        "a cancelled read is ignorance, never cleanliness"
    );
    assert_eq!(seen.disk_usage.unwrap_err(), CANCELLED);
    assert_eq!(
        worktree_removal_verdict(&seen.removal),
        WorktreeRemovalVerdict::Unknown(WorktreeUnknownReason::ObservationFailed)
    );
}
