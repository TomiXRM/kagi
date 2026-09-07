//! Integration tests for delete-branch — W2-DELETE
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_delete_branch_merged_success` | merged branch deleted successfully |
//! | 2 | `test_plan_delete_branch_unmerged_warning` | unmerged branch → warning and double confirmation |
//! | 3 | `test_plan_delete_branch_current_branch_blocker` | current branch → plan returns blocker |
//! | 4 | `test_plan_delete_branch_nonexistent_blocker` | non-existent branch → plan returns blocker |
//! | 5 | `test_delete_branch_recovery_sha` | recovery string contains the tip SHA |
//! | 6 | `test_execute_delete_branch_preflight_mismatch` | HEAD moved → execute returns Refused |
//! | 7 | `test_delete_branch_upstream_warning` | upstream configured → plan shows warning |

#[path = "support/backend_ops.rs"]
mod backend_ops;
use backend_ops::execute_delete_branch;
use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_domain::plan_note::{BranchNote, PlanNote};
use kagi_git::plan_delete_branch;

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
    if !crate::test_support::run_isolated() {
        return;
    }
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
// Test 2: unmerged branch → warning and double confirmation
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_unmerged_warning() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup_repo();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_delete_branch(&repo, "unmerged").expect("plan should succeed");

    assert!(
        plan.blockers.is_empty(),
        "unmerged branch must be confirmable, got: {:?}",
        plan.blockers
    );

    let msg = plan
        .warnings
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("unmerged") || msg.contains("not reachable"),
        "warning must mention unmerged/not-reachable: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: current branch → plan returns blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_delete_branch_current_branch_blocker() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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

/// Clean linked worktrees also retain their checked-out branch and directory.
#[test]
fn clean_worktree_blocks_delete() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let wt_path = fixture.path.join("wt-merged");
    git(
        &fixture.path,
        &["worktree", "add", wt_path.to_str().unwrap(), "merged"],
    );
    let repo = Repository::open(&fixture.path).unwrap();
    let plan = plan_delete_branch(&repo, "merged").unwrap();
    assert!(plan.blockers.iter().any(|note| matches!(
        note,
        PlanNote::Branch(BranchNote::DeleteBranchCheckedOut { .. })
    )));
    assert!(execute_delete_branch(&repo, &plan, "merged").is_err());
    assert!(wt_path.exists());
    assert!(repo.find_branch("merged", git2::BranchType::Local).is_ok());
}

/// A DIRTY linked worktree blocks the plan with a readable message and
/// execute refuses (no data loss).
#[test]
fn dirty_worktree_blocks_delete() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
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
fn a_genuinely_unmerged_branch_requires_confirmation() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let r = setup_squash_repo();
    let repo = Repository::open(&r.path).unwrap();

    let plan = plan_delete_branch(&repo, "orphan").expect("plan should succeed");
    assert!(
        plan.warnings.iter().any(|b| matches!(
            b,
            PlanNote::Branch(BranchNote::DeleteUnmerged { name, .. }) if name == "orphan"
        )),
        "an unmerged branch must require two confirmations — patch-id \
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
fn a_whitespace_only_difference_requires_two_confirmations() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
        plan.warnings.iter().any(|b| matches!(
            b,
            PlanNote::Branch(BranchNote::DeleteUnmerged { name, .. }) if name == "ws"
        )),
        "a whitespace-only patch-id collision is NOT a squash merge — the \
         delete must require two confirmations. Got blockers: {:?}, warnings: {:?}",
        plan.blockers,
        plan.warnings
    );
}

#[path = "support/isolated.rs"]
mod test_support;

// #584: the same arm transition used by Enter/button, then the public Backend.
#[test]
fn unmerged_armed_delete_receipt_gc_restore_and_retirement() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    let tip = repo
        .find_branch("unmerged", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let loose = repo.blob(b"unreachable GC control").unwrap();
    let mut backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    assert!(plan.blockers.is_empty());
    assert!(plan.warnings.iter().any(|w| matches!(
        w,
        PlanNote::Branch(BranchNote::DeleteUnmerged { commits: 1, .. })
    )));
    let mut modal = kagi::ui::modals::DeleteBranchModal {
        owner: delete_owner(&fixture.path),
        branch_name: "unmerged".into(),
        plan: std::sync::Arc::new(plan),
        error: None,
        confirm_armed: false,
    };
    assert!(modal.arm_if_required());
    assert!(repo
        .find_branch("unmerged", git2::BranchType::Local)
        .is_ok());
    assert!(!modal.arm_if_required());
    let report = backend.run_recorded(
        &kagi_git::Operation::DeleteBranch {
            name: "unmerged".into(),
        },
        &modal.plan,
    );
    let entry = report.recording.entry().clone();
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Appended { .. }
    ));
    report.result.unwrap();
    assert_eq!(entry.backup_refs.len(), 1);
    let reference = &entry.backup_refs[0];
    assert_eq!(repo.find_reference(reference).unwrap().target(), Some(tip));
    let kagi_git::oplog::OpOutcome::Success { after } = &entry.outcome else {
        panic!("{entry:?}");
    };
    assert!(after.dirty.contains(&tip.to_string()));
    let restore = after.dirty.split("; restore: ").nth(1).unwrap();
    assert_eq!(restore, format!("git branch unmerged {reference}"));
    git(
        &fixture.path,
        &["reflog", "expire", "--expire=now", "--all"],
    );
    git(&fixture.path, &["gc", "--prune=now"]);
    assert!(
        repo.find_blob(loose).is_err(),
        "GC must actually prune unrooted objects"
    );
    assert!(repo.find_commit(tip).is_ok());
    // Existing recovery command, now sourced from the persisted backup ref.
    let persisted = kagi_git::oplog::read_oplog_tail_for_repo(&fixture.path, 10);
    assert!(persisted
        .iter()
        .any(|e| e.id == entry.id && e.backup_refs == entry.backup_refs));
    let persisted_entry = persisted.iter().find(|e| e.id == entry.id).unwrap();
    let kagi_git::oplog::OpOutcome::Success {
        after: persisted_after,
    } = &persisted_entry.outcome
    else {
        panic!("persisted outcome");
    };
    let command = persisted_after.dirty.split("; restore: ").nth(1).unwrap();
    let args = command.split_whitespace().skip(1).collect::<Vec<_>>();
    git(&fixture.path, &args);
    assert_eq!(
        repo.find_branch("unmerged", git2::BranchType::Local)
            .unwrap()
            .get()
            .target(),
        Some(tip)
    );
    let retirement = backend.plan_forget_oplog_entry(&entry).unwrap();
    backend
        .execute_forget_oplog_entry(&retirement)
        .result
        .unwrap();
    assert!(repo.find_reference(reference).is_err());
    assert!(
        repo.find_commit(tip).is_ok(),
        "restored branch retains the commit"
    );
}

#[test]
fn merged_branch_confirmation_does_not_arm() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let mut backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let mut modal = kagi::ui::modals::DeleteBranchModal {
        owner: delete_owner(&fixture.path),
        branch_name: "merged".into(),
        plan: std::sync::Arc::new(backend.plan_delete_branch("merged").unwrap()),
        error: None,
        confirm_armed: false,
    };
    assert!(!modal.arm_if_required());
    backend
        .run(
            &kagi_git::Operation::DeleteBranch {
                name: "merged".into(),
            },
            &modal.plan,
        )
        .unwrap();
    assert!(Repository::open(&fixture.path)
        .unwrap()
        .find_branch("merged", git2::BranchType::Local)
        .is_err());
}

#[test]
fn changed_delete_tip_is_refused_without_backup_or_deletion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    let mut backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.reference("refs/heads/unmerged", head, true, "fixture drift")
        .unwrap();
    let report = backend.run_recorded(
        &kagi_git::Operation::DeleteBranch {
            name: "unmerged".into(),
        },
        &plan,
    );
    assert!(report.result.is_err());
    assert!(report.recording.entry().backup_refs.is_empty());
    assert_eq!(
        repo.find_reference("refs/heads/unmerged").unwrap().target(),
        Some(head)
    );
}

#[test]
fn shared_tip_does_not_overstate_unreachable_count() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    git(&fixture.path, &["branch", "also-keeps-tip", "unmerged"]);
    let backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    assert!(plan.warnings.iter().any(|w| matches!(
        w,
        PlanNote::Branch(BranchNote::DeleteUnmerged { commits: 0, .. })
    )));
}

#[test]
fn failed_backup_ref_creation_keeps_unmerged_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    let mut backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    // A file where the namespace directory must be makes exclusive creation fail.
    std::fs::create_dir_all(repo.path().join("refs/kagi")).unwrap();
    std::fs::write(
        repo.path().join("refs/kagi/backups"),
        repo.head().unwrap().target().unwrap().to_string(),
    )
    .unwrap();
    let report = backend.run_recorded(
        &kagi_git::Operation::DeleteBranch {
            name: "unmerged".into(),
        },
        &plan,
    );
    assert!(report.result.is_err());
    assert!(report.recording.entry().backup_refs.is_empty());
    assert!(repo
        .find_branch("unmerged", git2::BranchType::Local)
        .is_ok());
}

#[test]
fn failed_recording_keeps_branch_tip_recovery_root() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    let mut backend = kagi_git::Backend::open(&fixture.path).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    let tip = repo
        .find_branch("unmerged", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    let log = std::path::PathBuf::from(std::env::var_os("KAGI_LOG_DIR").unwrap());
    std::fs::create_dir(log.join("operations.jsonl")).unwrap();
    let report = backend.run_recorded(
        &kagi_git::Operation::DeleteBranch {
            name: "unmerged".into(),
        },
        &plan,
    );
    assert!(report.result.is_ok());
    assert!(matches!(
        report.recording,
        kagi_git::backend::recording::Recording::Failed { .. }
    ));
    let entry = report.recording.entry();
    assert_eq!(entry.backup_refs.len(), 1);
    assert_eq!(
        repo.find_reference(&entry.backup_refs[0]).unwrap().target(),
        Some(tip)
    );
    assert!(
        backend.read_backup(&entry.backup_refs[0]).is_err(),
        "blob reader must not reinterpret a commit root"
    );
}

fn delete_owner(path: &Path) -> kagi::app::Attachment {
    let mut sessions = kagi::app::Sessions::default();
    let session = sessions.attach(path.to_path_buf());
    sessions.attachment(session).unwrap()
}

#[test]
fn linked_worktree_cannot_delete_main_checked_out_branch() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let linked = fixture.path.join("linked");
    git(
        &fixture.path,
        &["worktree", "add", linked.to_str().unwrap(), "merged"],
    );
    let repo = Repository::open(&linked).unwrap();
    let head = head_sha(&fixture.path);
    let plan = plan_delete_branch(&repo, "main").unwrap();
    assert!(plan.blockers.iter().any(|note| matches!(
        note,
        PlanNote::Branch(BranchNote::DeleteBranchCheckedOut { .. })
    )));
    assert!(execute_delete_branch(&repo, &plan, "main").is_err());
    assert_eq!(head_sha(&fixture.path), head);
    assert_eq!(
        repo.find_reference("refs/heads/main")
            .unwrap()
            .target()
            .unwrap()
            .to_string(),
        head
    );
}

#[test]
fn checkout_in_main_after_linked_plan_refuses_deletion() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let linked = fixture.path.join("linked");
    git(
        &fixture.path,
        &["worktree", "add", linked.to_str().unwrap(), "merged"],
    );
    let repo = Repository::open(&linked).unwrap();
    let plan = plan_delete_branch(&repo, "unmerged").unwrap();
    assert!(plan.blockers.is_empty());
    git(&fixture.path, &["checkout", "-q", "unmerged"]);
    let head = head_sha(&fixture.path);
    assert!(execute_delete_branch(&repo, &plan, "unmerged").is_err());
    assert_eq!(head_sha(&fixture.path), head);
    assert!(repo.find_reference("refs/heads/unmerged").is_ok());
}

#[test]
fn symbolic_alias_chain_is_not_an_independent_reachability_root() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    repo.reference_symbolic("refs/heads/alias", "refs/heads/unmerged", false, "alias")
        .unwrap();
    repo.reference_symbolic(
        "refs/heads/alias-chain",
        "refs/heads/alias",
        false,
        "alias chain",
    )
    .unwrap();
    let plan = plan_delete_branch(&repo, "unmerged").unwrap();
    assert!(plan.warnings.iter().any(|note| matches!(
        note,
        PlanNote::Branch(BranchNote::DeleteUnmerged { commits: 1, .. })
    )));
    execute_delete_branch(&repo, &plan, "unmerged").unwrap();
    assert!(repo
        .find_reference("refs/heads/alias-chain")
        .unwrap()
        .resolve()
        .is_err());
}

#[test]
fn deleting_branch_removes_its_reflog_before_name_reuse() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let repo = Repository::open(&fixture.path).unwrap();
    let log = repo.path().join("logs/refs/heads/merged");
    assert!(log.exists());
    let old = std::fs::read_to_string(&log).unwrap();
    let plan = plan_delete_branch(&repo, "merged").unwrap();
    execute_delete_branch(&repo, &plan, "merged").unwrap();
    assert!(!log.exists());
    git(&fixture.path, &["branch", "merged", "HEAD"]);
    let new = std::fs::read_to_string(&log).unwrap();
    assert_eq!(new.lines().count(), 1);
    assert_ne!(old, new);
}

#[test]
fn main_head_lock_blocks_deletion_from_linked_worktree() {
    if !crate::test_support::run_isolated() {
        return;
    }
    let fixture = setup_repo();
    let linked = fixture.path.join("linked");
    git(
        &fixture.path,
        &["worktree", "add", linked.to_str().unwrap(), "merged"],
    );
    let mut backend = kagi_git::Backend::open(&linked).unwrap();
    let plan = backend.plan_delete_branch("unmerged").unwrap();
    assert!(plan.blockers.is_empty());
    // Model another process changing the main HEAD while the caller is linked.
    let main = Repository::open(&fixture.path).unwrap();
    let mut checkout = main.transaction().unwrap();
    checkout.lock_ref("HEAD").unwrap();
    let report = backend.run_recorded(
        &kagi_git::Operation::DeleteBranch {
            name: "unmerged".into(),
        },
        &plan,
    );
    assert!(report.result.is_err());
    assert!(report.recording.entry().backup_refs.is_empty());
    assert!(main.find_reference("refs/heads/unmerged").is_ok());
}

#[test]
fn departed_delete_plan_releases_busy_and_revisit_can_replan() {
    if !crate::test_support::run_isolated() {
        return;
    }
    use kagi::ui::modals::DeleteBranchModal;
    let a = setup_repo();
    let b = setup_repo();
    let mut sessions = kagi::app::Sessions::new();
    let session_a = sessions.attach(a.path.clone());
    let session_b = sessions.attach(b.path.clone());
    let owner = sessions.attachment(session_a).unwrap();
    let mut busy = Some("delete-branch-plan");
    sessions.depart(session_a);
    let current_b = sessions.attachment(session_b).unwrap();
    assert!(
        !DeleteBranchModal::settle_plan(&owner, Some(&current_b), false, &mut busy),
        "A's completion may not install a modal or footer on B"
    );
    assert_eq!(busy, None, "departed plan must release its latch");
    sessions.depart(session_b);
    let revisited_a = sessions.attachment(session_a).unwrap();
    assert!(
        !DeleteBranchModal::settle_plan(&owner, Some(&revisited_a), true, &mut busy),
        "the old visit cannot restore a modal on A"
    );
    assert_eq!(busy, None, "A must be free to re-plan");
    busy = Some("delete-branch-plan");
    let fresh_plan = kagi_git::Backend::open(&a.path)
        .unwrap()
        .plan_delete_branch("unmerged")
        .unwrap();
    assert!(fresh_plan.blockers.is_empty());
    assert!(DeleteBranchModal::settle_plan(
        &revisited_a,
        Some(&revisited_a),
        true,
        &mut busy
    ));
    assert_eq!(busy, None);
    busy = Some("checkout");
    assert!(!DeleteBranchModal::settle_plan(
        &owner,
        Some(&current_b),
        false,
        &mut busy
    ));
    assert_eq!(
        busy,
        Some("checkout"),
        "do not release an unrelated operation"
    );
}
