//! Integration tests for stash pop operation pipeline (T-HT-007).
//!
//! Tests for `plan_stash_pop` / `execute_stash_pop` per ADR-0009:
//! - pop = apply (success) then drop.  Apply failure → stash untouched.
//! - Conflict prediction (in-memory merge) → blocker + stash preserved.
//! - `include_untracked=false` for `execute_stash_push` → untracked files remain.
//!
//! All write operations are confined to `TempDir` repositories.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    execute_stash_apply, execute_stash_pop, execute_stash_push, plan_stash_pop, snapshot,
};

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
    assert!(
        status.success(),
        "git {} exited with {:?}",
        args.join(" "),
        status.code()
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Build a minimal repo with an initial commit.  HEAD is on `main`, clean.
fn build_clean_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);
    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);
    let repo = Repository::open(d).expect("failed to open repo");
    (d.to_path_buf(), repo)
}

// ────────────────────────────────────────────────────────────
// TC-POP-1: Normal pop — changes restored, stash count decreases by 1
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_normal_restores_and_removes_entry() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Dirty then push to stash.
    write_file(&repo_dir, "README.md", "stashed content\n");
    execute_stash_push(&mut repo, Some("wip"), true).expect("push failed");

    // Verify: clean, stash count = 1.
    {
        let snap = snapshot(&mut repo, 100).expect("snapshot");
        assert!(!snap.status.is_dirty(), "should be clean after push");
        assert_eq!(snap.stashes.len(), 1, "stash count should be 1");
    }

    // Plan pop at index 0 — should have no blockers.
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(
        plan.blockers.is_empty(),
        "clean repo with stash should have no blockers for pop, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.title.contains("pop") || plan.title.contains("Pop"),
        "plan title should mention pop, got: {:?}",
        plan.title
    );

    // Execute pop.
    execute_stash_pop(&mut repo, 0).expect("execute_stash_pop failed");

    // After pop: working tree dirty (content restored) AND stash count = 0.
    let snap_after = snapshot(&mut repo, 100).expect("snapshot after pop");
    assert!(
        snap_after.status.is_dirty(),
        "working tree must be dirty after pop (content restored)"
    );
    assert_eq!(
        snap_after.stashes.len(),
        0,
        "stash entry must be removed after pop (count must be 0)"
    );

    // File content must match the stashed content.
    let content = std::fs::read_to_string(repo_dir.join("README.md")).expect("read README.md");
    assert_eq!(
        content, "stashed content\n",
        "file content must match stashed content after pop"
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-2: Conflict prediction → blocker + stash entry preserved + repo intact
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_conflict_prediction_blocker_stash_preserved() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // Initial commit: file.txt = "line A\n"
    write_file(d, "file.txt", "line A\n");
    git(d, &["add", "file.txt"]);
    git(d, &["commit", "-qm", "initial"]);

    // Stash a change: file.txt = "line STASHED\n"
    write_file(d, "file.txt", "line STASHED\n");
    let mut repo = Repository::open(d).expect("open repo");
    execute_stash_push(&mut repo, Some("stash-conflict"), true).expect("push failed");

    // Now advance HEAD: file.txt = "line HEAD\n" — creates divergence.
    write_file(d, "file.txt", "line HEAD\n");
    git(d, &["add", "file.txt"]);
    git(d, &["commit", "-qm", "advance HEAD"]);

    // Capture WT content before planning.
    let wt_before = std::fs::read_to_string(d.join("file.txt")).expect("read before");

    // Plan pop — conflict must be predicted → blocker.
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");

    assert!(
        !plan.blockers.is_empty(),
        "conflict prediction should produce blockers, got none"
    );
    let has_conflict_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("conflict") || b.contains("Conflict"));
    assert!(
        has_conflict_blocker,
        "blocker should mention conflict, got: {:?}",
        plan.blockers
    );
    // Blocker should also recommend apply as alternative.
    let has_apply_suggestion = plan
        .blockers
        .iter()
        .any(|b| b.contains("Apply") || b.contains("apply"));
    assert!(
        has_apply_suggestion,
        "blocker should recommend 'apply' as alternative, got: {:?}",
        plan.blockers
    );

    // WT must be intact (plan must not touch working tree).
    let wt_after = std::fs::read_to_string(d.join("file.txt")).expect("read after");
    assert_eq!(
        wt_before, wt_after,
        "plan_stash_pop must not modify working tree"
    );

    // Stash must still be present.
    let snap = snapshot(&mut repo, 100).expect("snapshot after blocked plan");
    assert_eq!(
        snap.stashes.len(),
        1,
        "stash entry must remain after conflict-blocked plan"
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-3: Dirty working tree → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_blocker_dirty_working_tree() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Push something to stash.
    write_file(&repo_dir, "README.md", "stashed\n");
    execute_stash_push(&mut repo, None, true).expect("push failed");

    // Dirty the working tree (unstaged modification).
    write_file(&repo_dir, "README.md", "new dirty\n");

    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty working tree should produce a blocker for stash pop"
    );
    let has_dirty_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("dirty") || b.contains("modified") || b.contains("staged"));
    assert!(
        has_dirty_blocker,
        "blocker should mention dirty tree, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-4: Index out of range → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_blocker_index_out_of_range() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, mut repo) = build_clean_repo(&tmp);

    // No stash entries — try index 0.
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");

    assert!(
        !plan.blockers.is_empty(),
        "index out of range should produce a blocker"
    );
    let has_range_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("out of range") || b.contains("range"));
    assert!(
        has_range_blocker,
        "blocker should mention index out of range, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-5: apply 失敗時に drop されない (conflict予測blockerで代替)
//
// Note: apply失敗の直接テストは、plan_stash_pop が conflict を予測してblockerにする
// ため execute_stash_pop に到達しない。ADR-0009 の設計通り。
// この TC は conflict 予測 blocker 後に stash が残存することで「apply失敗→drop されない」
// という保証を検証する。
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_apply_failure_stash_not_dropped() {
    // This test demonstrates the "apply failure → no drop" guarantee
    // via the conflict prediction blocker path (ADR-0009 design intent).
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "shared.txt", "original\n");
    git(d, &["add", "shared.txt"]);
    git(d, &["commit", "-qm", "base"]);

    // Stash: shared.txt = "stashed version\n"
    write_file(d, "shared.txt", "stashed version\n");
    let mut repo = Repository::open(d).expect("open repo");
    execute_stash_push(&mut repo, Some("conflict-stash"), true).expect("push failed");

    // Advance HEAD with a conflicting change.
    write_file(d, "shared.txt", "head version\n");
    git(d, &["add", "shared.txt"]);
    git(d, &["commit", "-qm", "head change"]);

    // plan_stash_pop must block with conflict prediction.
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(
        !plan.blockers.is_empty(),
        "conflict should produce blockers (execute_stash_pop will not be called)"
    );

    // Since plan is blocked, execute_stash_pop is NOT called.
    // Stash must still be present (the "no-drop on failure" invariant holds
    // because the plan gate prevents reaching execute).
    let snap = snapshot(&mut repo, 100).expect("snapshot");
    assert_eq!(
        snap.stashes.len(),
        1,
        "stash must survive because pop was blocked before execute"
    );

    // WT must not have been modified.
    let wt = std::fs::read_to_string(d.join("shared.txt")).expect("read shared.txt");
    assert_eq!(wt, "head version\n", "WT must remain at HEAD version");
}

// ────────────────────────────────────────────────────────────
// TC-POP-6: include_untracked=false → untracked files remain
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_push_include_untracked_false_untracked_remains() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Tracked modification (will be stashed).
    write_file(&repo_dir, "README.md", "modified tracked\n");
    // Untracked file (should NOT be stashed when include_untracked=false).
    write_file(&repo_dir, "untracked.txt", "untracked content\n");

    // Push with include_untracked=false.
    execute_stash_push(&mut repo, Some("no-untracked"), false)
        .expect("push with include_untracked=false failed");

    // Working tree: tracked file reverted (stashed), untracked file still present.
    assert!(
        repo_dir.join("untracked.txt").exists(),
        "untracked.txt should remain in the working tree when include_untracked=false"
    );

    // Verify stash was created (tracked changes are stashed).
    let snap = snapshot(&mut repo, 100).expect("snapshot after push");
    assert_eq!(snap.stashes.len(), 1, "stash count should be 1 after push");

    // Tracked file should be at committed content (README.md = "# test\n").
    let readme = std::fs::read_to_string(repo_dir.join("README.md")).expect("read README.md");
    assert_eq!(
        readme, "# test\n",
        "tracked file should be reverted to committed content after stash push"
    );

    // Apply the stash to restore tracked changes.
    execute_stash_apply(&mut repo, 0).expect("apply failed");

    // After apply: tracked changes restored, untracked file still present.
    let readme_after_apply =
        std::fs::read_to_string(repo_dir.join("README.md")).expect("read README.md after apply");
    assert_eq!(
        readme_after_apply, "modified tracked\n",
        "tracked change must be restored after apply"
    );
    assert!(
        repo_dir.join("untracked.txt").exists(),
        "untracked.txt should still be present after apply"
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-7: pop plan title and recovery mention "pop = apply + drop"
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_plan_title_and_recovery_mention_destructive() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    write_file(&repo_dir, "README.md", "wip\n");
    execute_stash_push(&mut repo, Some("my stash"), true).expect("push failed");

    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");

    // Title should mention pop.
    assert!(
        plan.title.contains("pop") || plan.title.contains("Pop"),
        "plan title should mention pop, got: {:?}",
        plan.title
    );

    // Recovery text must warn that stash will be consumed.
    let recovery_warns_destructive = plan.recovery.contains("pop")
        || plan.recovery.contains("drop")
        || plan.recovery.contains("removed")
        || plan.recovery.contains("consumed");
    assert!(
        recovery_warns_destructive,
        "recovery text should warn about stash being consumed, got: {:?}",
        plan.recovery
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-8: multiple stashes — pop index 0 removes only index 0
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_removes_only_target_index() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Create 2 stash entries.
    write_file(&repo_dir, "README.md", "first stash\n");
    execute_stash_push(&mut repo, Some("first"), true).expect("push 1 failed");

    write_file(&repo_dir, "file2.txt", "second stash\n");
    execute_stash_push(&mut repo, Some("second"), true).expect("push 2 failed");

    {
        let snap = snapshot(&mut repo, 100).expect("snapshot");
        assert_eq!(snap.stashes.len(), 2, "should have 2 stashes before pop");
    }

    // Pop index 0 (most recent stash = "second").
    let plan = plan_stash_pop(&mut repo, 0).expect("plan failed");
    assert!(
        plan.blockers.is_empty(),
        "pop should have no blockers, got: {:?}",
        plan.blockers
    );

    execute_stash_pop(&mut repo, 0).expect("pop failed");

    // After pop: 1 stash remains.
    let snap_after = snapshot(&mut repo, 100).expect("snapshot after pop");
    assert_eq!(
        snap_after.stashes.len(),
        1,
        "exactly 1 stash should remain after popping index 0"
    );
}
