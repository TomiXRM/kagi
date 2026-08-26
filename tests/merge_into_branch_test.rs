//! Merging into a branch that is not checked out (ADR-0144).
//!
//! The property that matters is negative: `refs/heads/<target>` moves and
//! *nothing else does*. Every test here asserts what stayed still as well as
//! what moved — a merge that quietly checked the target out would satisfy
//! "the target advanced" just as well.

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{execute_merge_into_branch, plan_merge_into_branch, MergeIntoKind};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git failed");
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).expect("write");
}

/// `main` and `feature` diverge: each adds a file the other does not have, so
/// the merge is a real two-parent merge and cannot fast-forward.
fn setup() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let p = tmp.path().to_path_buf();
    git(&p, &["init", "-q", "-b", "main", "."]);
    git(&p, &["config", "user.name", "Test"]);
    git(&p, &["config", "user.email", "test@example.com"]);
    git(&p, &["config", "commit.gpgsign", "false"]);
    write(&p, "base.txt", "base\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "base"]);

    git(&p, &["checkout", "-q", "-b", "feature"]);
    write(&p, "feature.txt", "feature\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "feature work"]);

    git(&p, &["checkout", "-q", "main"]);
    write(&p, "main.txt", "main\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "main work"]);

    // Stand somewhere that is neither side of the merge.
    git(&p, &["checkout", "-q", "-b", "bystander"]);
    (tmp, p)
}

#[test]
fn merging_into_a_branch_moves_only_that_branch() {
    let (_t, p) = setup();
    let repo = Repository::open(&p).unwrap();

    let head_before = out(&p, &["rev-parse", "HEAD"]);
    let bystander_before = out(&p, &["rev-parse", "bystander"]);
    let main_before = out(&p, &["rev-parse", "main"]);
    let feature_before = out(&p, &["rev-parse", "feature"]);
    let status_before = out(&p, &["status", "--porcelain"]);
    let files_before = out(&p, &["ls-files"]);

    let (plan, kind) = plan_merge_into_branch(&repo, "feature", "main").unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert_eq!(kind, MergeIntoKind::MergeCommit);

    execute_merge_into_branch(&repo, "feature", "main").expect("merge");

    // The target advanced, and the merge is a real two-parent commit whose
    // parents are the two branch tips.
    let main_after = out(&p, &["rev-parse", "main"]);
    assert_ne!(main_after, main_before, "main must advance");
    assert_eq!(
        out(&p, &["rev-parse", "main^1"]),
        main_before,
        "first parent must be main's old tip"
    );
    assert_eq!(
        out(&p, &["rev-parse", "main^2"]),
        feature_before,
        "second parent must be the merged branch"
    );
    // The merge really combined both sides rather than taking one of them.
    let tree = out(&p, &["ls-tree", "--name-only", "-r", "main"]);
    for f in ["base.txt", "main.txt", "feature.txt"] {
        assert!(tree.contains(f), "merged tree is missing {f}: {tree}");
    }

    // …and nothing else moved.
    assert_eq!(out(&p, &["rev-parse", "HEAD"]), head_before, "HEAD moved");
    assert_eq!(
        out(&p, &["rev-parse", "bystander"]),
        bystander_before,
        "the checked-out branch moved"
    );
    assert_eq!(
        out(&p, &["rev-parse", "feature"]),
        feature_before,
        "the source branch moved"
    );
    assert_eq!(
        out(&p, &["status", "--porcelain"]),
        status_before,
        "the working tree changed"
    );
    assert_eq!(out(&p, &["ls-files"]), files_before, "the index changed");
    // The merged-in file must NOT appear on disk: it belongs to a branch that
    // is not checked out.
    assert!(
        !p.join("feature.txt").exists(),
        "a file from the merged branch was written into the working tree"
    );
}

#[test]
fn an_uncommitted_change_survives_the_merge() {
    let (_t, p) = setup();
    write(&p, "base.txt", "MY UNCOMMITTED EDIT\n");

    let repo = Repository::open(&p).unwrap();
    execute_merge_into_branch(&repo, "feature", "main").expect("merge");

    assert_eq!(
        std::fs::read_to_string(p.join("base.txt")).unwrap(),
        "MY UNCOMMITTED EDIT\n",
        "the user's uncommitted work was overwritten"
    );
}

#[test]
fn a_fast_forward_target_just_moves_its_ref() {
    let (_t, p) = setup();
    // `behind` sits at main's parent, so merging main into it fast-forwards.
    git(&p, &["branch", "behind", "main~1"]);
    let repo = Repository::open(&p).unwrap();

    let (plan, kind) = plan_merge_into_branch(&repo, "main", "behind").unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert_eq!(kind, MergeIntoKind::FastForward);
    let warn = plan
        .warnings
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(warn.contains("fast-forward"), "{warn}");

    execute_merge_into_branch(&repo, "main", "behind").expect("merge");
    assert_eq!(
        out(&p, &["rev-parse", "behind"]),
        out(&p, &["rev-parse", "main"]),
        "a fast-forward must land exactly on the source, with no new commit"
    );
}

#[test]
fn a_conflicting_merge_is_blocked_rather_than_half_done() {
    let (_t, p) = setup();
    // Make both branches edit the same file differently.
    git(&p, &["checkout", "-q", "feature"]);
    write(&p, "base.txt", "feature version\n");
    git(&p, &["commit", "-qam", "feature edits base"]);
    git(&p, &["checkout", "-q", "main"]);
    write(&p, "base.txt", "main version\n");
    git(&p, &["commit", "-qam", "main edits base"]);
    git(&p, &["checkout", "-q", "bystander"]);

    let repo = Repository::open(&p).unwrap();
    let main_before = out(&p, &["rev-parse", "main"]);

    let (plan, _) = plan_merge_into_branch(&repo, "feature", "main").unwrap();
    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("conflict"),
        "a conflicting merge must be blocked, and say so: {msg}"
    );
    assert!(
        msg.contains("checked out"),
        "the blocker must say what to do about it: {msg}"
    );

    // Executing anyway must refuse and leave the branch where it was — this is
    // the case where a half-applied merge would be worst.
    assert!(execute_merge_into_branch(&repo, "feature", "main").is_err());
    assert_eq!(out(&p, &["rev-parse", "main"]), main_before);
}

#[test]
fn merging_into_the_current_branch_is_refused_here() {
    let (_t, p) = setup();
    git(&p, &["checkout", "-q", "main"]);
    let repo = Repository::open(&p).unwrap();

    let (plan, _) = plan_merge_into_branch(&repo, "feature", "main").unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "the current branch belongs to plan_merge_branch, not this path"
    );
}

#[test]
fn merging_into_a_branch_checked_out_in_a_worktree_is_refused() {
    let (_t, p) = setup();
    // Its own TempDir: a worktree cannot live inside the repo, and a fixed
    // path under the system temp dir survives the test and collides with the
    // next run (and with a parallel one).
    let wt_home = TempDir::new().expect("tempdir");
    let wt = wt_home.path().join("wt-main");
    git(&p, &["worktree", "add", "-q", wt.to_str().unwrap(), "main"]);

    let repo = Repository::open(&p).unwrap();
    let (plan, _) = plan_merge_into_branch(&repo, "feature", "main").unwrap();
    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("worktree"),
        "moving the ref under a live worktree must be refused: {msg}"
    );
}

#[test]
fn a_target_that_already_contains_the_source_is_a_no_op() {
    let (_t, p) = setup();
    let repo = Repository::open(&p).unwrap();
    execute_merge_into_branch(&repo, "feature", "main").expect("first merge");

    let (plan, _) = plan_merge_into_branch(&repo, "feature", "main").unwrap();
    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(msg.contains("already contains"), "{msg}");
}

// ────────────────────────────────────────────────────────────
// Dropping onto a remote badge (ADR-0144)
// ────────────────────────────────────────────────────────────

/// Adds a bare remote and pushes `main` to it, then deletes the local `main`
/// so only `origin/main` remains.
fn with_remote_only_branch() -> (TempDir, PathBuf) {
    let (t, p) = setup();
    let remote = t.path().join("remote.git");
    git(&p, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(&p, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&p, &["push", "-q", "origin", "main"]);
    git(&p, &["branch", "-D", "main"]);
    (t, p)
}

#[test]
fn dropping_onto_a_remote_ref_creates_the_local_branch_and_merges_into_it() {
    let (_t, p) = with_remote_only_branch();
    let repo = Repository::open(&p).unwrap();

    let remote_tip = out(&p, &["rev-parse", "refs/remotes/origin/main"]);
    let feature_tip = out(&p, &["rev-parse", "feature"]);

    let (plan, _) = plan_merge_into_branch(&repo, "feature", "origin/main").unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let warn = plan
        .warnings
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        warn.contains("no local 'main' yet"),
        "the plan must say a branch is created: {warn}"
    );

    execute_merge_into_branch(&repo, "feature", "origin/main").expect("merge");

    // The local branch now exists, tracks the remote, and holds the merge.
    assert_eq!(
        out(&p, &["rev-parse", "main^1"]),
        remote_tip,
        "the new branch must start at the remote tip"
    );
    assert_eq!(out(&p, &["rev-parse", "main^2"]), feature_tip);
    assert_eq!(
        out(&p, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main",
        "the created branch must track the remote ref it came from"
    );

    // Nothing was pushed: the remote is exactly where it was.
    assert_eq!(
        out(&p, &["rev-parse", "refs/remotes/origin/main"]),
        remote_tip,
        "the remote-tracking ref must not move"
    );
    assert!(
        !p.join("main.txt").exists() || out(&p, &["rev-parse", "--abbrev-ref", "HEAD"]) != "main",
        "the created branch must not have been checked out"
    );
}

/// When the local branch already exists it is the destination, and the remote
/// ref is only read. A plan that silently retargeted the remote would be a way
/// to lose work.
#[test]
fn dropping_onto_a_remote_ref_whose_local_branch_exists_targets_the_local_one() {
    let (_t, p) = setup();
    // Its own TempDir: a fixed path under the system temp dir survives the run
    // and the next one pushes into a remote that is already ahead.
    let remote_home = TempDir::new().expect("tempdir");
    let remote = remote_home.path().join("remote-x.git");
    git(&p, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(&p, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&p, &["push", "-q", "origin", "main"]);
    // Move the local branch on past what the remote has.
    git(&p, &["checkout", "-q", "main"]);
    write(&p, "extra.txt", "extra\n");
    git(&p, &["add", "-A"]);
    git(&p, &["commit", "-qm", "local only"]);
    git(&p, &["checkout", "-q", "bystander"]);

    let local_tip = out(&p, &["rev-parse", "main"]);
    let repo = Repository::open(&p).unwrap();

    let (plan, _) = plan_merge_into_branch(&repo, "feature", "origin/main").unwrap();
    let warn = plan
        .warnings
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        warn.contains("not at 'origin/main'"),
        "the plan must flag that local and remote differ: {warn}"
    );

    execute_merge_into_branch(&repo, "feature", "origin/main").expect("merge");
    assert_eq!(
        out(&p, &["rev-parse", "main^1"]),
        local_tip,
        "the merge must build on the LOCAL tip, not the remote's"
    );
}
