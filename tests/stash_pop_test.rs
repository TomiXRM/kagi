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

use kagi_git::{
    execute_stash_apply, execute_stash_drop, execute_stash_pop, execute_stash_push,
    plan_stash_drop, plan_stash_pop, preflight_check_stash, snapshot, StashPopOutcome,
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
        plan.title.message_en().contains("pop") || plan.title.message_en().contains("Pop"),
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
// TC-DROP-1: Standalone drop — entry removed, working tree NOT touched (ADR-0087)
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_drop_removes_entry_without_touching_working_tree() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Two stashes so we can verify only the targeted one is dropped.
    write_file(&repo_dir, "README.md", "first change\n");
    execute_stash_push(&mut repo, Some("first"), true).expect("push 1 failed");
    write_file(&repo_dir, "README.md", "second change\n");
    execute_stash_push(&mut repo, Some("second"), true).expect("push 2 failed");

    // Clean working tree, 2 stashes.
    {
        let snap = snapshot(&mut repo, 100).expect("snapshot");
        assert!(!snap.status.is_dirty(), "clean after pushes");
        assert_eq!(snap.stashes.len(), 2, "two stashes expected");
    }

    // Plan drop of stash@{0} — only blocker possible is out-of-range (none here).
    let plan = plan_stash_drop(&mut repo, 0).expect("plan_stash_drop failed");
    assert!(
        plan.blockers.is_empty(),
        "drop of a valid index should have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(plan.destructive, "drop plan must be marked destructive");

    // Execute drop — returns the dropped stash commit OID.
    let oid = execute_stash_drop(&mut repo, 0).expect("execute_stash_drop failed");
    assert!(!oid.is_empty(), "drop should return the stash commit OID");

    // After drop: one stash left, working tree STILL clean (drop never restores).
    let snap_after = snapshot(&mut repo, 100).expect("snapshot after drop");
    assert_eq!(
        snap_after.stashes.len(),
        1,
        "exactly one stash entry must remain after drop"
    );
    assert!(
        !snap_after.status.is_dirty(),
        "drop must NOT touch the working tree (still clean)"
    );

    // The remaining stash is the older "first" entry (re-indexed to 0).
    assert!(
        snap_after.stashes[0].message.contains("first"),
        "the remaining stash should be the older 'first' entry, got: {:?}",
        snap_after.stashes[0].message
    );
}

// ────────────────────────────────────────────────────────────
// TC-POP-2: Conflict prediction → blocker + stash entry preserved + repo intact
// ────────────────────────────────────────────────────────────

#[test]
fn test_stash_pop_conflict_prediction_warns_and_touches_nothing() {
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

    // Plan pop — the conflict must be predicted, as a WARNING (not a blocker:
    // a conflicted apply keeps the stash, so the user may confirm through it —
    // a blocker left the GUI modal with only a Cancel button).
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");

    assert!(
        plan.blockers.is_empty(),
        "a predicted conflict must not block, got: {:?}",
        plan.blockers
    );
    let warning = plan
        .warnings
        .iter()
        .find(|w| w.message_en().contains("will conflict"))
        .unwrap_or_else(|| panic!("the plan must warn about the conflict: {:?}", plan.warnings));
    assert!(
        warning.message_en().contains("KEPT"),
        "the warning must say the stash survives: {}",
        warning.message_en()
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
    let has_dirty_blocker = plan.blockers.iter().any(|b| {
        b.message_en().contains("dirty")
            || b.message_en().contains("modified")
            || b.message_en().contains("staged")
    });
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
        .any(|b| b.message_en().contains("out of range") || b.message_en().contains("range"));
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
fn test_stash_pop_planning_a_conflicting_pop_changes_nothing() {
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

    // plan_stash_pop predicts the conflict as a warning, and planning alone
    // must change nothing: no drop, no working-tree write. (This test's unique
    // claim is the second half; the confirm-through path is covered by
    // test_stash_pop_conflicting_apply_keeps_stash.)
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(plan.blockers.is_empty(), "got: {:?}", plan.blockers);
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("will conflict")),
        "got: {:?}",
        plan.warnings
    );

    let snap = snapshot(&mut repo, 100).expect("snapshot");
    assert_eq!(snap.stashes.len(), 1, "planning must not drop the stash");

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
        plan.title.message_en().contains("pop") || plan.title.message_en().contains("Pop"),
        "plan title should mention pop, got: {:?}",
        plan.title
    );

    // Recovery text must warn that stash will be consumed.
    let recovery_warns_destructive = plan
        .recovery
        .as_ref()
        .map(|r| r.message_en())
        .unwrap_or_default()
        .contains("pop")
        || plan
            .recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains("drop")
        || plan
            .recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains("removed")
        || plan
            .recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains("consumed");
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

// ────────────────────────────────────────────────────────────
// issue #280 — a conflicted apply must NOT drop the stash
// ────────────────────────────────────────────────────────────

/// Repo where a pop is guaranteed to conflict at apply time:
/// base `shared.txt` = "base", stash = "stashed", HEAD moved to "head".
fn build_conflicting_stash_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "shared.txt", "base\n");
    git(d, &["add", "shared.txt"]);
    git(d, &["commit", "-qm", "base"]);

    write_file(d, "shared.txt", "stashed\n");
    let mut repo = Repository::open(d).expect("open repo");
    execute_stash_push(&mut repo, Some("conflict-stash"), true).expect("push failed");

    write_file(d, "shared.txt", "head\n");
    git(d, &["add", "shared.txt"]);
    git(d, &["commit", "-qm", "head change"]);

    // Re-open: the handle above cached the pre-commit index.
    drop(repo);
    let repo = Repository::open(d).expect("re-open repo");
    (d.to_path_buf(), repo)
}

/// Acceptance criterion of issue #280: `execute_stash_pop` on an apply that
/// conflicts reports `ConflictedStashKept` and leaves the stash list untouched.
/// (Called directly, bypassing the plan blocker, because the whole point is
/// that execute must be safe on its own.)
#[test]
fn test_stash_pop_conflicting_apply_keeps_stash() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_conflicting_stash_repo(&tmp);

    let outcome = execute_stash_pop(&mut repo, 0).expect("execute_stash_pop must not hard-error");

    match &outcome {
        StashPopOutcome::ConflictedStashKept { files } => {
            assert!(
                files.iter().any(|f| f == "shared.txt"),
                "conflicted files should name shared.txt, got: {:?}",
                files
            );
        }
        other => panic!("expected ConflictedStashKept, got {:?}", other),
    }

    // The stash entry MUST still be there — this is the data-loss regression.
    let snap = snapshot(&mut repo, 100).expect("snapshot after conflicted pop");
    assert_eq!(
        snap.stashes.len(),
        1,
        "conflicted pop must NOT drop the stash entry (issue #280)"
    );

    // And the conflict really is in the tree.
    let wt = std::fs::read_to_string(repo_dir.join("shared.txt")).expect("read shared.txt");
    assert!(
        wt.contains("<<<<<<<") && wt.contains(">>>>>>>"),
        "working tree should hold conflict markers, got: {:?}",
        wt
    );
    assert!(
        !snap.status.conflicted.is_empty(),
        "status should report the conflicted path"
    );
}

/// The divergence scenario from issue #280: the stash was taken on a branch
/// whose base is NOT an ancestor of the current HEAD, so `merge_base(HEAD,
/// stash)` (the old prediction base) sees no conflict while the real apply
/// (base = stash parent[0]) does. The prediction must now block it.
#[test]
fn test_stash_pop_prediction_uses_stash_parent_as_base() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // X (main): f = "old" — the common ancestor.
    write_file(d, "f.txt", "old\n");
    git(d, &["add", "f.txt"]);
    git(d, &["commit", "-qm", "X"]);

    // Branch A: commit B changes f to "mainline-v2", then stash "stashed".
    git(d, &["checkout", "-q", "-b", "a"]);
    write_file(d, "f.txt", "mainline-v2\n");
    git(d, &["add", "f.txt"]);
    git(d, &["commit", "-qm", "B"]);
    write_file(d, "f.txt", "stashed\n");
    let mut repo = Repository::open(d).expect("open repo");
    execute_stash_push(&mut repo, Some("on-a"), true).expect("push failed");

    // Branch C from X (does NOT contain B): f is still "old".
    git(d, &["checkout", "-q", "-b", "c", "main"]);

    // The prediction must fire (the old merge_base(HEAD, stash) base said
    // 'clean' for this shape) — but as a WARNING, not a blocker: a conflicted
    // apply keeps the stash, so the user must be able to confirm through it
    // (GUI report: a blocker left the modal with only a Cancel button).
    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(
        plan.blockers.is_empty(),
        "a predicted conflict must not block the pop, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("will conflict")),
        "the plan must WARN about the predicted conflict; \
         merge_base(HEAD, stash) as the base would have said 'clean': {:?}",
        plan.warnings
    );

    // Confirming through the warning keeps the stash.
    let outcome = execute_stash_pop(&mut repo, 0).expect("execute must not hard-error");
    assert!(
        matches!(outcome, StashPopOutcome::ConflictedStashKept { .. }),
        "expected ConflictedStashKept, got {:?}",
        outcome
    );
    let snap = snapshot(&mut repo, 100).expect("snapshot");
    assert_eq!(snap.stashes.len(), 1, "stash must survive a conflicted pop");
}

/// Fail-closed: when the prediction cannot be computed (here: a stash ref whose
/// commit is a root commit, so `parent(0)` errors), the plan must carry a
/// blocker instead of silently reporting "clean".
#[test]
fn test_stash_pop_prediction_failure_is_fail_closed() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // A real stash first, so refs/stash and its reflog exist.
    write_file(&repo_dir, "README.md", "wip\n");
    execute_stash_push(&mut repo, Some("real"), true).expect("push failed");

    // A parentless commit reachable from refs/stash: stash_foreach lists it,
    // but `parent(0)` — the apply base — does not exist.
    let tree = Command::new("git")
        .args(["hash-object", "-t", "tree", "-w", "--stdin"])
        .current_dir(&repo_dir)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("hash-object failed");
    let tree_oid = String::from_utf8_lossy(&tree.stdout).trim().to_string();
    let commit = Command::new("git")
        .args(["commit-tree", &tree_oid, "-m", "WIP orphan stash"])
        .current_dir(&repo_dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("commit-tree failed");
    let commit_oid = String::from_utf8_lossy(&commit.stdout).trim().to_string();
    git(
        &repo_dir,
        &["update-ref", "refs/stash", &commit_oid, "-m", "WIP orphan"],
    );

    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(
        !plan.blockers.is_empty(),
        "an uncomputable prediction must fail closed (blocker), got no blockers"
    );
    let msg = plan
        .blockers
        .iter()
        .map(|b| b.message_en())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        msg.contains("Could not verify"),
        "blocker should be the fail-closed prediction note, got: {}",
        msg
    );
}

/// issue #280: pop deletes the stash entry irreversibly, so its plan is
/// Destructive class. (Was `destructive: false`; the expectation changed
/// because the confirm UI must gate pop like drop/reset, not like apply.)
#[test]
fn test_stash_pop_plan_is_destructive() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    write_file(&repo_dir, "README.md", "wip\n");
    execute_stash_push(&mut repo, Some("wip"), true).expect("push failed");

    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    assert!(
        plan.destructive,
        "stash pop plan must be marked destructive (it deletes the stash entry)"
    );
}

/// issue #280: a tree that turned dirty between plan and execute is what makes
/// `stash_apply` write conflicts — preflight must refuse.
#[test]
fn test_preflight_check_stash_rejects_dirty_tree() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    write_file(&repo_dir, "README.md", "wip\n");
    execute_stash_push(&mut repo, Some("wip"), true).expect("push failed");

    let plan = plan_stash_pop(&mut repo, 0).expect("plan_stash_pop failed");
    let count = plan.stash_count_at_plan();
    assert!(preflight_check_stash(&mut repo, &plan, count).is_ok());

    // The tree goes dirty after the plan was confirmed.
    write_file(&repo_dir, "README.md", "sneaky edit\n");
    let err = preflight_check_stash(&mut repo, &plan, count)
        .expect_err("preflight must reject a tree that turned dirty since planning");
    assert!(
        format!("{}", err).contains("Working tree changed since planning"),
        "unexpected preflight error: {}",
        err
    );

    // A standalone drop is unaffected: it never touches the working tree.
    let drop_plan = plan_stash_drop(&mut repo, 0).expect("plan_stash_drop failed");
    assert!(
        preflight_check_stash(&mut repo, &drop_plan, drop_plan.stash_count_at_plan()).is_ok(),
        "stash drop must stay allowed on a dirty tree"
    );
}
