//! Integration tests for delete-branch — W2-DELETE
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_delete_branch_merged_success` | merged branch deleted successfully |
//! | 2 | `test_plan_delete_branch_unmerged_blocker` | unmerged branch → plan returns blocker |
//! | 3 | `test_plan_delete_branch_current_branch_blocker` | current branch → plan returns blocker |
//! | 4 | `test_plan_delete_branch_nonexistent_blocker` | non-existent branch → plan returns blocker |
//! | 5 | `test_delete_branch_recovery_sha` | recovery string contains the tip SHA |
//! | 6 | `test_execute_delete_branch_preflight_mismatch` | HEAD moved → execute returns Refused |
//! | 7 | `test_delete_branch_upstream_warning` | upstream configured → plan shows warning |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::{BranchNote, PlanNote};
use kagi_git::{execute_delete_branch, plan_delete_branch};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git command failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

#[allow(dead_code)]
fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repo with:
///   - `main`: two commits (initial + "base")
///   - `merged`: branched from initial commit, one commit, merged into main via --no-ff
///   - `unmerged`: branched from main, one commit, NOT merged into main
struct TestRepo {
    _tmp: TempDir,
    path: PathBuf,
}

fn setup_repo() -> TestRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(&path, "README.md", "# test\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    // Create `merged` branch and commit.
    git(&path, &["checkout", "-q", "-b", "merged"]);
    write_file(&path, "merged.txt", "merged content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "merged branch commit"]);

    // Merge `merged` into main (no-ff so a real merge commit is created).
    git(&path, &["checkout", "-q", "main"]);
    git(
        &path,
        &["merge", "--no-ff", "-m", "merge merged into main", "merged"],
    );

    // Create `unmerged` branch and commit (NOT merged into main).
    git(&path, &["checkout", "-q", "-b", "unmerged"]);
    write_file(&path, "unmerged.txt", "unmerged content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "unmerged branch commit"]);

    // Back to main.
    git(&path, &["checkout", "-q", "main"]);

    TestRepo { _tmp: tmp, path }
}

// ────────────────────────────────────────────────────────────
// Test 1: merged branch → plan has no blockers, execute succeeds
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_merged_success() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();

    // Plan should have no blockers for the merged branch.
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");
    assert!(
        plan.blockers.is_empty(),
        "merged branch must not have blockers, got: {:?}",
        plan.blockers
    );

    // Branch must exist before deletion.
    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_ok(),
        "merged branch should exist before delete"
    );

    // Execute must succeed.
    execute_delete_branch(&repo, &plan, "merged").expect("delete should succeed");

    // Branch must be gone.
    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_err(),
        "merged branch must be gone after delete"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: unmerged branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_unmerged_blocker() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "unmerged").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "unmerged branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("unmerged") || msg.contains("not reachable"),
        "blocker must mention unmerged/not-reachable: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: current branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_current_branch_blocker() {
    let r = setup_repo();

    // HEAD is on `main`.
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "main").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "current branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("checked-out") || msg.contains("current"),
        "blocker must mention current/checked-out: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 4: non-existent branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_nonexistent_blocker() {
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "does-not-exist").expect("plan should succeed");

    assert!(
        !plan.blockers.is_empty(),
        "non-existent branch must be a blocker, got: {:?}",
        plan.blockers
    );

    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("does not exist") || msg.contains("not found"),
        "blocker must mention does-not-exist/not-found: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: recovery string contains the tip SHA
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_recovery_sha() {
    let r = setup_repo();

    // Get the tip SHA of `merged`.
    let tip_sha_out = Command::new("git")
        .args(["rev-parse", "--short", "merged"])
        .current_dir(&r.path)
        .output()
        .expect("rev-parse failed");
    let tip_short = String::from_utf8_lossy(&tip_sha_out.stdout)
        .trim()
        .to_string();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");

    assert!(
        plan.recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains(&tip_short),
        "recovery string must contain tip SHA '{}', got: {:?}",
        tip_short,
        plan.recovery
    );

    // Also check that predicted text contains the tip SHA.
    assert!(
        plan.title.message_en().contains(&tip_short),
        "plan title must contain tip SHA '{}', got: {}",
        tip_short,
        plan.title
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: preflight mismatch → execute returns Err
// ────────────────────────────────────────────────────────────

#[test]
fn test_execute_delete_branch_preflight_mismatch() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    // Initial commit.
    write_file(&path, "base.txt", "base\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "base"]);

    // Create and merge a branch.
    git(&path, &["checkout", "-q", "-b", "to-delete"]);
    write_file(&path, "td.txt", "td\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "to delete commit"]);
    git(&path, &["checkout", "-q", "main"]);
    git(
        &path,
        &["merge", "--no-ff", "-m", "merge to-delete", "to-delete"],
    );

    // Build plan (captures current HEAD).
    let repo = Repository::open(&path).unwrap();
    let plan = plan_delete_branch(&repo, "to-delete").expect("plan should succeed");
    assert!(
        plan.blockers.is_empty(),
        "should have no blockers: {:?}",
        plan.blockers
    );

    // Simulate HEAD movement: add a new commit on main after planning.
    drop(repo);
    write_file(&path, "extra.txt", "extra\n");
    git(&path, &["add", "-A"]);
    git(
        &path,
        &["commit", "-qm", "extra commit (moves HEAD after planning)"],
    );

    // Execute must fail because HEAD moved.
    let repo2 = Repository::open(&path).unwrap();
    let result = execute_delete_branch(&repo2, &plan, "to-delete");
    assert!(
        result.is_err(),
        "execute must fail when HEAD moved since planning"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("changed") || err.contains("re-plan") || err.contains("plan"),
        "error must mention state change/re-plan: {}",
        err
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: upstream configured → plan shows warning
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_upstream_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");

    // Create bare remote.
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

    // Create local repo.
    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    // Initial commit + push.
    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    // Create a branch, push it (sets upstream), merge it into main.
    git(&local, &["checkout", "-q", "-b", "feat"]);
    write_file(&local, "feat.txt", "feat\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "feat commit"]);
    git(&local, &["push", "-q", "-u", "origin", "feat"]);
    git(&local, &["checkout", "-q", "main"]);
    git(&local, &["merge", "--no-ff", "-m", "merge feat", "feat"]);

    // Plan should have no blockers but a warning about upstream.
    let repo = Repository::open(&local).unwrap();
    let plan = plan_delete_branch(&repo, "feat").expect("plan should succeed");

    assert!(
        plan.blockers.is_empty(),
        "merged branch with upstream must not have blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        !plan.warnings.is_empty(),
        "plan must have a warning about the upstream not being deleted, got none"
    );
    let warn_msg = plan
        .warnings
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        warn_msg.contains("upstream") || warn_msg.contains("remote"),
        "warning must mention upstream/remote: {}",
        warn_msg
    );
}

// ────────────────────────────────────────────────────────────
// Regression: duplicated gh CLI branch config keys must not
// break deletion (user repo had dozens of duplicated
// `branch.<name>.github-pr-owner-number` entries written by
// `gh pr`, making the 1st delete fail and the 2nd succeed).
// ────────────────────────────────────────────────────────────

#[test]
fn test_delete_branch_with_duplicated_gh_config_keys() {
    let r = setup_repo();

    // Simulate gh CLI's duplicated-key pollution on the merged branch.
    for _ in 0..3 {
        git(
            &r.path,
            &[
                "config",
                "--add",
                "branch.merged.github-pr-owner-number",
                "owner#repo#42",
            ],
        );
    }
    // A second polluted key, plus a normal upstream-style key.
    git(
        &r.path,
        &["config", "--add", "branch.merged.gh-merge-base", "main"],
    );
    git(
        &r.path,
        &["config", "--add", "branch.merged.gh-merge-base", "main"],
    );

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "merged").expect("plan should succeed");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    // First attempt must succeed (this used to fail with
    // "could not find key '…github-pr-owner-number' to delete").
    execute_delete_branch(&repo, &plan, "merged")
        .expect("delete must succeed on the FIRST attempt despite duplicated config keys");

    assert!(
        repo.find_branch("merged", git2::BranchType::Local).is_err(),
        "branch must be gone"
    );
    // The polluted section must be cleaned up.
    let cfg = repo.config().unwrap().snapshot().unwrap();
    let mut leftover = 0;
    let mut entries = cfg.entries(Some("branch\\.merged\\..*")).unwrap();
    while entries.next().is_some() {
        leftover += 1;
    }
    assert_eq!(
        leftover, 0,
        "branch.merged.* config entries must be removed"
    );
}

// ────────────────────────────────────────────────────────────
// Worktree-checkout handling (user report: agent worktrees pin
// their branch and git's raw refusal was opaque)
// ────────────────────────────────────────────────────────────

/// A CLEAN linked worktree pinning the branch: the plan carries a warning
/// (remove-then-delete), and execute removes the worktree and the branch.
#[test]
fn clean_worktree_is_removed_then_branch_deleted() {
    let repo = setup_repo();
    let wt_path = repo.path.join("wt-merged");
    git(
        &repo.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "merged"],
    );

    let r = git2::Repository::open(&repo.path).unwrap();
    let plan = plan_delete_branch(&r, "merged").unwrap();
    assert!(
        plan.blockers.is_empty(),
        "clean worktree must not block: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("worktree")),
        "plan must warn about the worktree removal: {:?}",
        plan.warnings
    );

    execute_delete_branch(&r, &plan, "merged").unwrap();
    assert!(!wt_path.exists(), "worktree dir must be removed");
    assert!(
        r.find_branch("merged", git2::BranchType::Local).is_err(),
        "branch must be deleted"
    );
}

/// A DIRTY linked worktree blocks the plan with a readable message and
/// execute refuses (no data loss).
#[test]
fn dirty_worktree_blocks_delete() {
    let repo = setup_repo();
    let wt_path = repo.path.join("wt-merged");
    git(
        &repo.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "merged"],
    );
    write_file(&wt_path, "wip.txt", "uncommitted");

    let r = git2::Repository::open(&repo.path).unwrap();
    let plan = plan_delete_branch(&r, "merged").unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("uncommitted") && b.message_en().contains("worktree")),
        "dirty worktree must block with a readable message: {:?}",
        plan.blockers
    );
    // Execute (simulating a stale confirm) must refuse, not destroy work.
    assert!(execute_delete_branch(&r, &plan, "merged").is_err());
    assert!(wt_path.join("wip.txt").exists(), "work must be untouched");
}

/// A LOCKED worktree blocks with an unlock hint.
#[test]
fn locked_worktree_blocks_delete() {
    let repo = setup_repo();
    let wt_path = repo.path.join("wt-merged");
    git(
        &repo.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "merged"],
    );
    git(&repo.path, &["worktree", "lock", wt_path.to_str().unwrap()]);

    let r = git2::Repository::open(&repo.path).unwrap();
    let plan = plan_delete_branch(&r, "merged").unwrap();
    assert!(
        plan.blockers.iter().any(
            |b| b.message_en().contains("LOCKED") && b.message_en().contains("Unlock worktree")
        ),
        "locked worktree must block and point at the sidebar unlock flow: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Squash-merge: the tip is never an ancestor of main, so the
// reachability check alone blocks the delete forever (user report:
// "squash merge済みのローカルブランチをkagiでは消せなかった").
// ────────────────────────────────────────────────────────────

/// `git merge --squash` + commit is what `gh pr merge --squash` produces:
/// the branch's whole diff lands as one new commit whose parent is main.
fn setup_squash_repo() -> TestRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "README.md", "# test\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    // A feature branch with two commits — a squash folds both into one.
    git(&path, &["checkout", "-q", "-b", "feat"]);
    write_file(&path, "feat.txt", "one\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "feat: one"]);
    write_file(&path, "feat.txt", "one\ntwo\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "feat: two"]);

    // A branch that is genuinely unmerged, to prove the check still blocks.
    git(&path, &["checkout", "-q", "main"]);
    git(&path, &["checkout", "-q", "-b", "orphan"]);
    write_file(&path, "orphan.txt", "nothing merged this\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "orphan commit"]);

    git(&path, &["checkout", "-q", "main"]);
    git(&path, &["merge", "--squash", "feat"]);
    git(&path, &["commit", "-qm", "feat: one and two (#1)"]);

    TestRepo { _tmp: tmp, path }
}

#[test]
fn squash_merged_branch_is_deletable_with_a_warning() {
    let r = setup_squash_repo();
    let repo = Repository::open(&r.path).unwrap();

    // Precondition: the tip really is NOT reachable from HEAD, so the plain
    // ancestor check would call this unmerged.
    let head = repo.head().unwrap().target().unwrap();
    let tip = repo
        .find_branch("feat", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    assert!(
        !repo.graph_descendant_of(head, tip).unwrap(),
        "squash-merged tip must not be an ancestor of HEAD"
    );

    let plan = plan_delete_branch(&repo, "feat").expect("plan should succeed");
    assert!(
        plan.blockers.is_empty(),
        "squash-merged branch must not be blocked, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("squash-merged")),
        "the plan must explain why a dead-end branch is safe, got: {:?}",
        plan.warnings
    );

    execute_delete_branch(&repo, &plan, "feat").expect("delete should succeed");
    assert!(repo.find_branch("feat", git2::BranchType::Local).is_err());
}

#[test]
fn a_genuinely_unmerged_branch_is_still_blocked() {
    let r = setup_squash_repo();
    let repo = Repository::open(&r.path).unwrap();

    let plan = plan_delete_branch(&repo, "orphan").expect("plan should succeed");
    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Branch(BranchNote::DeleteUnmerged { name, .. }) if name == "orphan"
        )),
        "an unmerged branch must still be blocked as unmerged — patch-id \
         equivalence must not become a back door to force delete. Got: {:?}",
        plan.blockers
    );
}

/// `git patch-id` normalises whitespace away, so a branch whose only
/// difference from what landed on main is indentation has an *identical*
/// patch-id — in Python (and every whitespace-significant language) that is a
/// real behaviour difference, and `git branch -d` refuses to delete it.
///
/// The squash-merge detector therefore confirms each patch-id hit
/// byte-exactly before downgrading the unmerged blocker to a warning
/// (ADR-0138). Drop that confirmation and this branch becomes deletable —
/// irreversibly, with no `-D` escape hatch in kagi. Fixture mirrors
/// `squash_links_test.rs::a_whitespace_only_difference_is_not_a_squash_merge`.
#[test]
fn a_whitespace_only_difference_must_not_unblock_the_delete() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "m.py", "def f(a):\n    if a:\n        return 1\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "root"]);

    // The branch adds the line INSIDE the `if` (eight spaces).
    git(&path, &["checkout", "-q", "-b", "ws"]);
    write_file(
        &path,
        "m.py",
        "def f(a):\n    if a:\n        return 1\n        return 2\n",
    );
    git(&path, &["commit", "-qam", "add return 2"]);

    // main gets the same line OUTSIDE the `if` (four spaces) — different
    // behaviour, identical patch-id.
    git(&path, &["checkout", "-q", "main"]);
    write_file(
        &path,
        "m.py",
        "def f(a):\n    if a:\n        return 1\n    return 2\n",
    );
    git(&path, &["commit", "-qam", "add return 2 (outside the if)"]);

    let repo = Repository::open(&path).unwrap();

    // Precondition: patch-id alone cannot tell the two changes apart, so the
    // cheap index really does report a hit here.
    let patch_id = |from: git2::Oid, to: git2::Oid| {
        let a = repo.find_commit(from).unwrap().tree().unwrap();
        let b = repo.find_commit(to).unwrap().tree().unwrap();
        repo.diff_tree_to_tree(Some(&a), Some(&b), None)
            .unwrap()
            .patchid(None)
            .unwrap()
    };
    let head = repo.head().unwrap().target().unwrap();
    let tip = repo
        .find_branch("ws", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let base = repo.merge_base(head, tip).unwrap();
    let head_parent = repo.find_commit(head).unwrap().parent(0).unwrap().id();
    assert_eq!(
        patch_id(base, tip),
        patch_id(head_parent, head),
        "fixture is stale: the two changes must share a patch-id for this test \
         to exercise the exact-match confirmation"
    );

    let plan = plan_delete_branch(&repo, "ws").expect("plan should succeed");
    assert!(
        plan.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Branch(BranchNote::DeleteUnmerged { name, .. }) if name == "ws"
        )),
        "a whitespace-only patch-id collision is NOT a squash merge — the \
         delete must stay blocked. Got blockers: {:?}, warnings: {:?}",
        plan.blockers,
        plan.warnings
    );
}
