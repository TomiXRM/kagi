//! Integration tests for the merged-branch cleanup pipeline (ADR-0128).
//!
//! | # | test | verifies |
//! |---|------|----------|
//! | 1 | `collect_classifies_merged_grown_stale` | class table: FullyMerged (+merge date), MergedThenGrown, stale-only, fresh hidden, main/current excluded |
//! | 2 | `collect_flags_gone_upstream_as_squash_candidate` | `[gone]` upstream → SquashMergedLikely, individual-only delete |
//! | 3 | `plan_blocks_when_tip_moved` | tip moved after listing → plan blocker |
//! | 4 | `execute_deletes_local_and_remote` | full pipeline vs a file:// bare origin: both halves deleted, tips recorded |
//! | 5 | `execute_refuses_moved_local_tip` | commit lands after plan → per-branch failure, branch survives |

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use git2::Repository;
use kagi_git::ops::{
    collect_branch_cleanup, execute_delete_merged_branches, plan_delete_merged_branches,
    MergedBranchStatus,
};

/// 2026-01-10T00:00:00Z — all fixture dates are relative to this "now".
const NOW: i64 = 1_768_003_200;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

/// Run a git command whose commit/merge dates must be deterministic.
fn git_at(dir: &Path, date: &str, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn commit_file(dir: &Path, date: &str, name: &str, msg: &str) {
    std::fs::write(dir.join(name), msg).unwrap();
    git(dir, &["add", "-A"]);
    git_at(dir, date, &["commit", "-m", msg]);
}

fn init(tmp: &TempDir) -> &Path {
    let dir = tmp.path();
    git(dir, &["init", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    commit_file(dir, "2026-01-01T00:00:00Z", "base.txt", "base");
    dir
}

#[test]
fn collect_classifies_merged_grown_stale() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);

    // merged: branch merged back into main via a merge commit.
    git(dir, &["checkout", "-b", "merged"]);
    commit_file(dir, "2026-01-02T00:00:00Z", "merged.txt", "merged work");
    git(dir, &["checkout", "main"]);
    git_at(
        dir,
        "2026-01-03T00:00:00Z",
        &["merge", "--no-ff", "-m", "merge merged", "merged"],
    );

    // grown: merged the same way, then two more commits on top (develop pattern).
    git(dir, &["checkout", "-b", "grown"]);
    commit_file(dir, "2026-01-04T00:00:00Z", "grown.txt", "grown work");
    git(dir, &["checkout", "main"]);
    git_at(
        dir,
        "2026-01-05T00:00:00Z",
        &["merge", "--no-ff", "-m", "merge grown", "grown"],
    );
    git(dir, &["checkout", "grown"]);
    commit_file(dir, "2026-01-06T00:00:00Z", "grown2.txt", "post-merge 1");
    commit_file(dir, "2026-01-07T00:00:00Z", "grown3.txt", "post-merge 2");
    git(dir, &["checkout", "main"]);

    // dead: unmerged, last commit 130+ days before NOW.
    git(dir, &["checkout", "-b", "dead"]);
    commit_file(dir, "2025-09-01T00:00:00Z", "dead.txt", "old work");
    git(dir, &["checkout", "main"]);

    // fresh-wip: unmerged and recent — must not appear at all.
    git(dir, &["checkout", "-b", "fresh-wip"]);
    commit_file(dir, "2026-01-08T00:00:00Z", "wip.txt", "wip");
    git(dir, &["checkout", "main"]);

    let repo = Repository::open(dir).unwrap();
    let rows = collect_branch_cleanup(&repo, NOW).unwrap();

    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(!names.contains(&"main"), "default branch must be excluded");
    assert!(!names.contains(&"fresh-wip"), "fresh unmerged is hidden");

    let merged = rows
        .iter()
        .find(|r| r.name == "merged")
        .expect("merged row");
    assert_eq!(merged.status, MergedBranchStatus::FullyMerged);
    assert!(merged.bulk_deletable);
    // merged_at = the merge commit's committer date (2026-01-03).
    assert_eq!(merged.merged_at, Some(1_767_398_400));

    let grown = rows.iter().find(|r| r.name == "grown").expect("grown row");
    assert_eq!(
        grown.status,
        MergedBranchStatus::MergedThenGrown { ahead: 2 }
    );
    assert!(!grown.deletable, "grown branch gets no delete affordance");
    assert_eq!(grown.merged_at, Some(1_767_571_200)); // 2026-01-05 merge

    let dead = rows.iter().find(|r| r.name == "dead").expect("dead row");
    assert_eq!(dead.status, MergedBranchStatus::NotMerged);
    assert!(dead.stale);
    assert!(!dead.deletable);
}

#[test]
fn collect_flags_gone_upstream_as_squash_candidate() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);
    // origin exists (URL only, never contacted) so upstream config resolves.
    git(
        dir,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );

    // squashed: tip is NOT an ancestor of main, but its upstream is [gone]
    // (configured, remote-tracking ref absent) — the GitHub squash-merge shape.
    git(dir, &["checkout", "-b", "squashed"]);
    commit_file(dir, "2026-01-06T00:00:00Z", "sq.txt", "squashed work");
    git(dir, &["config", "branch.squashed.remote", "origin"]);
    git(
        dir,
        &["config", "branch.squashed.merge", "refs/heads/squashed"],
    );
    git(dir, &["checkout", "main"]);

    let repo = Repository::open(dir).unwrap();
    let rows = collect_branch_cleanup(&repo, NOW).unwrap();

    let row = rows.iter().find(|r| r.name == "squashed").expect("row");
    assert_eq!(row.status, MergedBranchStatus::SquashMergedLikely);
    assert!(row.deletable, "individually deletable");
    assert!(!row.bulk_deletable, "never part of the bulk action");
}

#[test]
fn plan_blocks_when_tip_moved() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);

    git(dir, &["checkout", "-b", "merged"]);
    commit_file(dir, "2026-01-02T00:00:00Z", "m.txt", "work");
    git(dir, &["checkout", "main"]);
    git_at(
        dir,
        "2026-01-03T00:00:00Z",
        &["merge", "--no-ff", "-m", "m", "merged"],
    );

    let repo = Repository::open(dir).unwrap();
    let rows = collect_branch_cleanup(&repo, NOW).unwrap();
    let target = rows
        .iter()
        .find(|r| r.name == "merged")
        .and_then(|r| r.delete_target())
        .expect("target");

    // The branch grows a commit between listing and planning.
    git(dir, &["checkout", "merged"]);
    commit_file(dir, "2026-01-08T00:00:00Z", "late.txt", "late work");
    git(dir, &["checkout", "main"]);

    let plan = plan_delete_merged_branches(&repo, NOW, &[target]).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "moved tip must block: {:?}",
        plan.blockers
    );
}

#[test]
fn execute_deletes_local_and_remote() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);
    let remote_tmp = TempDir::new().unwrap();
    git(remote_tmp.path(), &["init", "--bare", "."]);
    let remote_url = remote_tmp.path().to_str().unwrap().to_string();
    git(dir, &["remote", "add", "origin", &remote_url]);

    git(dir, &["checkout", "-b", "merged"]);
    commit_file(dir, "2026-01-02T00:00:00Z", "m.txt", "work");
    git(dir, &["push", "-u", "origin", "merged"]);
    git(dir, &["checkout", "main"]);
    git_at(
        dir,
        "2026-01-03T00:00:00Z",
        &["merge", "--no-ff", "-m", "m", "merged"],
    );
    git(dir, &["push", "-u", "origin", "main"]);

    let repo = Repository::open(dir).unwrap();
    let rows = collect_branch_cleanup(&repo, NOW).unwrap();
    let row = rows.iter().find(|r| r.name == "merged").expect("row");
    assert!(row.local_tip.is_some() && row.remote_tip.is_some());
    let target = row.delete_target().expect("target");
    let expected_tip = target.local_tip.clone().unwrap();

    let plan = plan_delete_merged_branches(&repo, NOW, std::slice::from_ref(&target)).unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_delete_merged_branches(&repo, dir, &plan, &[target]).unwrap();
    assert!(outcome.failed.is_empty(), "failed: {:?}", outcome.failed);
    assert_eq!(outcome.deleted.len(), 1);
    let deleted = &outcome.deleted[0];
    assert_eq!(deleted.name, "merged");
    // Recovery OIDs recorded for the oplog.
    assert_eq!(deleted.local_tip.as_ref(), Some(&expected_tip));
    assert!(deleted.remote_tip.is_some());

    // Local branch, tracking ref, and the branch on the bare remote are gone.
    assert!(repo.find_branch("merged", git2::BranchType::Local).is_err());
    assert!(repo.find_reference("refs/remotes/origin/merged").is_err());
    let bare = Repository::open(remote_tmp.path()).unwrap();
    assert!(bare.find_reference("refs/heads/merged").is_err());
}

#[test]
fn execute_refuses_moved_local_tip() {
    let tmp = TempDir::new().unwrap();
    let dir = init(&tmp);

    git(dir, &["checkout", "-b", "merged"]);
    commit_file(dir, "2026-01-02T00:00:00Z", "m.txt", "work");
    git(dir, &["checkout", "main"]);
    git_at(
        dir,
        "2026-01-03T00:00:00Z",
        &["merge", "--no-ff", "-m", "m", "merged"],
    );

    let repo = Repository::open(dir).unwrap();
    let rows = collect_branch_cleanup(&repo, NOW).unwrap();
    let target = rows
        .iter()
        .find(|r| r.name == "merged")
        .and_then(|r| r.delete_target())
        .expect("target");
    let plan = plan_delete_merged_branches(&repo, NOW, std::slice::from_ref(&target)).unwrap();
    assert!(plan.blockers.is_empty());

    // A commit lands on the branch between plan and execute (HEAD unmoved,
    // so the global preflight passes — the per-branch OID check must catch it).
    git(dir, &["checkout", "merged"]);
    commit_file(dir, "2026-01-08T00:00:00Z", "late.txt", "late work");
    git(dir, &["checkout", "main"]);

    let outcome = execute_delete_merged_branches(&repo, dir, &plan, &[target]).unwrap();
    assert!(outcome.deleted.is_empty());
    assert_eq!(outcome.failed.len(), 1);
    assert!(outcome.failed[0].1.contains("moved since plan"));
    assert!(repo.find_branch("merged", git2::BranchType::Local).is_ok());
}
