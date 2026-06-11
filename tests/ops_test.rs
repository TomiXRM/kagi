//! Integration tests for checkout operation pipeline (T013),
//! create-branch operation pipeline (T014), and
//! stash push / apply operation pipeline (T015).
//!
//! All write operations are confined to `TempDir` repositories created within
//! each test.  This project's own repository and any other existing repository
//! are **never** touched.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    CommitId, Head,
    ops::{
        execute_checkout, execute_create_branch, plan_checkout, plan_create_branch, preflight_check,
        plan_stash_push, execute_stash_push,
        plan_stash_apply, execute_stash_apply,
        preflight_check_stash,
    },
    snapshot,
};

// ────────────────────────────────────────────────────────────
// Helpers (copied from snapshot_test pattern)
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
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

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Build a minimal repo with two branches: `main` and `feature/one`.
/// HEAD is on `main`.  The repo is initially clean.
///
/// Returns `(TempDir, repo_dir_path, opened Repository)`.
fn build_two_branch_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();

    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);

    // Create feature/one from main.
    git(d, &["checkout", "-qb", "feature/one"]);
    write_file(d, "feat.txt", "feature work\n");
    git(d, &["add", "feat.txt"]);
    git(d, &["commit", "-qm", "feature/one work"]);

    // Return to main.
    git(d, &["checkout", "-q", "main"]);

    let repo = Repository::open(d).expect("failed to open repo");
    (d.to_path_buf(), repo)
}

// ────────────────────────────────────────────────────────────
// Test 1: clean repo — plan has no blockers, execute moves HEAD
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_clean_repo_no_blockers() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "clean repo should have no blockers, got: {:?}",
        plan.blockers
    );
    assert_eq!(
        plan.predicted.head,
        "branch: feature/one",
        "predicted HEAD should be 'branch: feature/one'"
    );
}

#[test]
fn test_execute_clean_repo_moves_head() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty(), "should have no blockers");

    preflight_check(&repo, &plan).expect("preflight_check failed");
    execute_checkout(&repo, "feature/one").expect("execute_checkout failed");

    // Re-open and verify HEAD moved.
    let repo2 = Repository::open(&repo_dir).expect("re-open repo");
    let mut repo2 = repo2;
    let snap = snapshot(&mut repo2, 100).expect("snapshot after checkout");
    assert!(
        matches!(&snap.head, Head::Attached { branch, .. } if branch == "feature/one"),
        "HEAD should be on feature/one after checkout, got: {:?}",
        snap.head
    );

    // Verify feat.txt was checked out.
    assert!(
        repo_dir.join("feat.txt").exists(),
        "feat.txt should exist after checkout to feature/one"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: dirty repo (unstaged modified) — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_dirty_unstaged_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Modify README.md without staging.
    write_file(&repo_dir, "README.md", "modified content\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty repo (unstaged) should have at least one blocker"
    );
    let has_stash_mention = plan.blockers.iter().any(|b| b.contains("stash"));
    assert!(
        has_stash_mention,
        "blocker should mention 'stash', got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_plan_dirty_staged_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Stage a new file.
    write_file(&repo_dir, "staged.txt", "staged\n");
    git(&repo_dir, &["add", "staged.txt"]);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty repo (staged) should have at least one blocker"
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: untracked only — no blocker, has warning, execute succeeds
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_untracked_only_no_blocker_warning() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Add an untracked file (not staged).
    write_file(&repo_dir, "untracked.txt", "not tracked\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "untracked-only should have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        !plan.warnings.is_empty(),
        "untracked-only should have a warning about untracked files"
    );
}

#[test]
fn test_execute_untracked_only_file_remains() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Add an untracked file.
    write_file(&repo_dir, "untracked.txt", "not tracked\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty());

    preflight_check(&repo, &plan).expect("preflight failed");
    execute_checkout(&repo, "feature/one").expect("execute failed");

    // Untracked file should still be there.
    assert!(
        repo_dir.join("untracked.txt").exists(),
        "untracked.txt should survive the checkout"
    );

    // HEAD should have moved.
    let repo2 = Repository::open(&repo_dir).expect("re-open");
    let head_ref = repo2.head().expect("head");
    let branch = head_ref.shorthand().unwrap_or("");
    assert_eq!(branch, "feature/one", "HEAD should be on feature/one");
}

// ────────────────────────────────────────────────────────────
// Test 4: nonexistent branch — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_nonexistent_branch_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "nonexistent-branch").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "nonexistent branch should have a blocker"
    );
    let has_not_exist = plan
        .blockers
        .iter()
        .any(|b| b.contains("not exist") || b.contains("does not exist"));
    assert!(
        has_not_exist,
        "blocker should mention branch not existing, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: already on HEAD branch — plan has blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_already_head_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    // HEAD is on main; try to plan checkout of main.
    let plan = plan_checkout(&repo, "main").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "checking out the current HEAD branch should produce a blocker"
    );
    let has_already = plan
        .blockers
        .iter()
        .any(|b| b.contains("already") || b.contains("current HEAD"));
    assert!(
        has_already,
        "blocker should mention 'already' or 'current HEAD', got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: preflight detects HEAD change, returns error
// ────────────────────────────────────────────────────────────

#[test]
fn test_preflight_aborts_when_head_changed() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Plan while on main.
    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");
    assert!(plan.blockers.is_empty());

    // Simulate HEAD changing between plan and execute: add a commit on main.
    write_file(&repo_dir, "extra.txt", "extra\n");
    git(&repo_dir, &["add", "extra.txt"]);
    git(&repo_dir, &["commit", "-qm", "extra commit"]);

    // preflight_check must detect the HEAD target changed.
    let result = preflight_check(&repo, &plan);
    assert!(
        result.is_err(),
        "preflight_check should return Err when HEAD has changed"
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: execute_checkout on nonexistent branch returns error
// ────────────────────────────────────────────────────────────

#[test]
fn test_execute_nonexistent_branch_returns_error() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let result = execute_checkout(&repo, "does-not-exist");
    assert!(
        result.is_err(),
        "execute_checkout on nonexistent branch should return Err"
    );
}

// ────────────────────────────────────────────────────────────
// Test 8: plan includes recovery text with original branch name
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_recovery_mentions_original_branch() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.recovery.contains("main"),
        "recovery text should mention 'main' (the original branch), got: {:?}",
        plan.recovery
    );
    assert!(
        plan.recovery.contains("reflog"),
        "recovery text should mention 'reflog', got: {:?}",
        plan.recovery
    );
}

// ────────────────────────────────────────────────────────────
// T014: create-branch tests
// ────────────────────────────────────────────────────────────

/// Helper: resolve HEAD commit id from a repo.
fn head_commit_id(repo: &Repository) -> CommitId {
    let oid = repo
        .head()
        .expect("repo.head()")
        .target()
        .expect("head target oid");
    CommitId(oid.to_string())
}

// ── T014-1: normal case — branch created, HEAD unchanged, WT unchanged ──

#[test]
fn test_create_branch_normal_creates_branch() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "new-feature", &at)
        .expect("plan_create_branch failed");

    assert!(
        plan.blockers.is_empty(),
        "no blockers expected for valid name + existing commit, got: {:?}",
        plan.blockers
    );

    // Execute.
    execute_create_branch(&repo, "new-feature", &at)
        .expect("execute_create_branch failed");

    // Branch must exist.
    let branch_exists = repo
        .find_branch("new-feature", git2::BranchType::Local)
        .is_ok();
    assert!(branch_exists, "branch 'new-feature' should exist after creation");

    // HEAD must still be on main.
    let head_ref = repo.head().expect("repo.head()");
    assert_eq!(
        head_ref.shorthand().unwrap_or(""),
        "main",
        "HEAD should still be 'main' after create-branch"
    );

    // Working tree file must be intact.
    assert!(
        repo_dir.join("README.md").exists(),
        "README.md should still exist after create-branch"
    );
}

#[test]
fn test_create_branch_head_and_wt_unchanged() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);

    // Capture HEAD oid before.
    let head_oid_before = repo
        .head()
        .unwrap()
        .target()
        .map(|o| o.to_string())
        .unwrap_or_default();

    execute_create_branch(&repo, "stable-branch", &at)
        .expect("execute_create_branch failed");

    // HEAD oid must be unchanged.
    let head_oid_after = repo
        .head()
        .unwrap()
        .target()
        .map(|o| o.to_string())
        .unwrap_or_default();
    assert_eq!(head_oid_before, head_oid_after, "HEAD OID should not change");

    // Branch must point to same commit.
    let branch_ref = repo
        .find_branch("stable-branch", git2::BranchType::Local)
        .expect("branch should exist");
    let branch_oid = branch_ref
        .get()
        .target()
        .map(|o| o.to_string())
        .unwrap_or_default();
    assert_eq!(
        head_oid_before, branch_oid,
        "new branch should point to the same commit as HEAD"
    );

    // Existing files must be intact.
    assert!(repo_dir.join("README.md").exists());
}

// ── T014-2: same-name blocker ────────────────────────────────

#[test]
fn test_create_branch_same_name_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);

    // 'main' already exists.
    let plan = plan_create_branch(&repo, "main", &at)
        .expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "creating a branch with an existing name should have a blocker"
    );
    let has_already_exists = plan
        .blockers
        .iter()
        .any(|b| b.contains("already exists") || b.contains("already exist"));
    assert!(
        has_already_exists,
        "blocker should mention 'already exists', got: {:?}",
        plan.blockers
    );
}

// ── T014-3: invalid name — 3 variants ────────────────────────

#[test]
fn test_create_branch_invalid_name_with_space() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "has space", &at)
        .expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "branch name with space should produce a blocker"
    );
    let has_invalid = plan
        .blockers
        .iter()
        .any(|b| b.contains("not a valid") || b.contains("invalid"));
    assert!(
        has_invalid,
        "blocker should mention invalid name, got: {:?}",
        plan.blockers
    );
}

#[test]
fn test_create_branch_invalid_name_double_dot() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "feat..broken", &at)
        .expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "branch name with '..' should produce a blocker"
    );
}

#[test]
fn test_create_branch_invalid_name_leading_dash() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "-bad-name", &at)
        .expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "branch name with leading '-' should produce a blocker"
    );
}

// ── T014-4: empty name blocker ───────────────────────────────

#[test]
fn test_create_branch_empty_name_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "", &at)
        .expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "empty branch name should produce a blocker"
    );
}

// ── T014-5: force=false prevents overwriting existing branch ─

#[test]
fn test_execute_create_branch_does_not_overwrite_existing() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    // 'feature/one' already exists at a different commit from HEAD (main).
    // find its tip commit.
    let feature_branch = repo
        .find_branch("feature/one", git2::BranchType::Local)
        .expect("feature/one should exist");
    let feature_oid = feature_branch
        .get()
        .target()
        .expect("feature/one target");
    let feature_commit_id_str = feature_oid.to_string();

    // We are on main; HEAD is at a different commit.
    let main_at = head_commit_id(&repo);
    assert_ne!(
        main_at.0, feature_commit_id_str,
        "HEAD (main) and feature/one should be at different commits"
    );

    // Calling execute_create_branch with force=false must fail, not overwrite.
    let result = execute_create_branch(&repo, "feature/one", &main_at);
    assert!(
        result.is_err(),
        "execute_create_branch with an existing branch name must return Err (force=false)"
    );

    // feature/one must still point to its original commit.
    let still_feature = repo
        .find_branch("feature/one", git2::BranchType::Local)
        .expect("feature/one should still exist");
    let still_oid = still_feature
        .get()
        .target()
        .map(|o| o.to_string())
        .unwrap_or_default();
    assert_eq!(
        still_oid, feature_commit_id_str,
        "feature/one must not be moved after failed create_branch (force=false)"
    );
}

// ────────────────────────────────────────────────────────────
// T015: stash push / apply tests
// ────────────────────────────────────────────────────────────

/// Helper: create a repo with a single commit (clean state).
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

// ── T015-1: stash push normal case — dirty repo → clean after push ────────

#[test]
fn test_stash_push_normal_cleans_working_tree() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Dirty the repo.
    write_file(&repo_dir, "README.md", "modified\n");

    let plan = plan_stash_push(&mut repo, Some("test stash"))
        .expect("plan_stash_push failed");

    assert!(
        plan.blockers.is_empty(),
        "dirty repo should have no blockers for stash push, got: {:?}",
        plan.blockers
    );

    // Execute.
    let mut repo2 = Repository::open(&repo_dir).expect("re-open");
    execute_stash_push(&mut repo2, Some("test stash"))
        .expect("execute_stash_push failed");

    // Working tree must be clean after push.
    let mut repo3 = Repository::open(&repo_dir).expect("re-open");
    let snap = snapshot(&mut repo3, 100).expect("snapshot");
    assert!(
        !snap.status.is_dirty(),
        "working tree must be clean after stash push"
    );
    // Stash count must be 1.
    assert_eq!(snap.stashes.len(), 1, "stash count must be 1 after push");
}

// ── T015-2: stash push blocker — clean repo ───────────────────────────────

#[test]
fn test_stash_push_blocker_on_clean_repo() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, mut repo) = build_clean_repo(&tmp);

    let plan = plan_stash_push(&mut repo, None)
        .expect("plan_stash_push failed");

    assert!(
        !plan.blockers.is_empty(),
        "clean repo should have a blocker for stash push (nothing to stash)"
    );
    let has_nothing = plan
        .blockers
        .iter()
        .any(|b| b.contains("Nothing to stash") || b.contains("clean"));
    assert!(
        has_nothing,
        "blocker should mention 'Nothing to stash' or 'clean', got: {:?}",
        plan.blockers
    );
}

// ── T015-3: stash push includes untracked — warning present ──────────────

#[test]
fn test_stash_push_untracked_warning() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Add only an untracked file.
    write_file(&repo_dir, "untracked.txt", "not tracked\n");

    let plan = plan_stash_push(&mut repo, None)
        .expect("plan_stash_push failed");

    assert!(
        plan.blockers.is_empty(),
        "untracked-only should have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        !plan.warnings.is_empty(),
        "untracked files should produce a warning about being included in stash"
    );
}

// ── T015-4: stash apply normal case — apply restores content, stash remains

#[test]
fn test_stash_apply_normal_restores_content_stash_remains() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Dirty the repo with a known change.
    write_file(&repo_dir, "README.md", "stashed content\n");

    // Push to stash so working tree is clean.
    execute_stash_push(&mut repo, Some("wip"))
        .expect("execute_stash_push failed");

    // Verify clean + 1 stash.
    {
        let snap = snapshot(&mut repo, 100).expect("snapshot");
        assert!(!snap.status.is_dirty(), "should be clean after push");
        assert_eq!(snap.stashes.len(), 1, "stash count should be 1");
    }

    // Plan apply at index 0.
    let plan = plan_stash_apply(&mut repo, 0)
        .expect("plan_stash_apply failed");

    assert!(
        plan.blockers.is_empty(),
        "clean repo with stash should have no blockers for apply, got: {:?}",
        plan.blockers
    );

    // Execute apply.
    execute_stash_apply(&mut repo, 0)
        .expect("execute_stash_apply failed");

    // Working tree must be dirty again (content restored).
    let snap_after = snapshot(&mut repo, 100).expect("snapshot after apply");
    assert!(
        snap_after.status.is_dirty(),
        "working tree must be dirty after stash apply (content restored)"
    );

    // Stash entry must STILL be present (apply, not pop).
    assert_eq!(
        snap_after.stashes.len(),
        1,
        "stash entry must remain after apply (not pop): stash count must be 1"
    );
}

// ── T015-5: stash apply blocker — dirty working tree ─────────────────────

#[test]
fn test_stash_apply_blocker_dirty_working_tree() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Push something to stash first so there's a stash entry.
    write_file(&repo_dir, "README.md", "to stash\n");
    execute_stash_push(&mut repo, None)
        .expect("execute_stash_push failed");

    // Now dirty the working tree again (unstaged change).
    write_file(&repo_dir, "README.md", "new dirty\n");

    // Plan apply: should be blocked because working tree is dirty.
    let plan = plan_stash_apply(&mut repo, 0)
        .expect("plan_stash_apply failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty working tree should produce a blocker for stash apply"
    );
    let has_dirty_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("dirty") || b.contains("staged") || b.contains("modified"));
    assert!(
        has_dirty_blocker,
        "blocker should mention dirty tree, got: {:?}",
        plan.blockers
    );
}

// ── T015-6: stash apply blocker — index out of range ─────────────────────

#[test]
fn test_stash_apply_blocker_index_out_of_range() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, mut repo) = build_clean_repo(&tmp);

    // No stash entries exist. Try to apply index 0.
    let plan = plan_stash_apply(&mut repo, 0)
        .expect("plan_stash_apply failed");

    assert!(
        !plan.blockers.is_empty(),
        "index out of range should produce a blocker"
    );
    let has_range = plan
        .blockers
        .iter()
        .any(|b| b.contains("out of range") || b.contains("range"));
    assert!(
        has_range,
        "blocker should mention index out of range, got: {:?}",
        plan.blockers
    );
}

// ── T015-7: round-trip — file content is fully restored ──────────────────

#[test]
fn test_stash_push_apply_round_trip_content_matches() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Write a specific content.
    let original_content = "round-trip content\n";
    write_file(&repo_dir, "README.md", original_content);

    // Push to stash.
    execute_stash_push(&mut repo, Some("round-trip stash"))
        .expect("execute_stash_push failed");

    // File must be reverted to its committed content.
    let file_after_push = std::fs::read_to_string(repo_dir.join("README.md"))
        .expect("read README.md after push");
    assert_ne!(
        file_after_push, original_content,
        "file content should differ from stashed content after push"
    );

    // Apply.
    execute_stash_apply(&mut repo, 0)
        .expect("execute_stash_apply failed");

    // File must be restored to original_content.
    let file_after_apply = std::fs::read_to_string(repo_dir.join("README.md"))
        .expect("read README.md after apply");
    assert_eq!(
        file_after_apply, original_content,
        "file content must match original after apply round-trip"
    );

    // Stash must still be there.
    let snap = snapshot(&mut repo, 100).expect("snapshot after round-trip");
    assert_eq!(
        snap.stashes.len(),
        1,
        "stash entry must persist after apply (stash count must be 1)"
    );
}

// ── T015-8: preflight_check_stash detects stash count change ─────────────

#[test]
fn test_preflight_check_stash_detects_count_change() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, mut repo) = build_clean_repo(&tmp);

    // Dirty and plan stash apply — but there's no stash yet, so plan has a blocker.
    // Instead, set up a stash, plan, then add another stash to change the count.

    // Create stash 1.
    write_file(&repo_dir, "README.md", "first stash content\n");
    execute_stash_push(&mut repo, Some("first"))
        .expect("first push failed");

    // Plan apply at index 0.
    let plan = plan_stash_apply(&mut repo, 0)
        .expect("plan_stash_apply failed");
    assert!(plan.blockers.is_empty(), "should have no blockers");

    let stash_count_at_plan = plan.stash_count_at_plan();

    // Simulate another stash being added between plan and execution.
    write_file(&repo_dir, "extra.txt", "extra content\n");
    execute_stash_push(&mut repo, Some("extra stash"))
        .expect("second push failed");

    // preflight_check_stash must detect the count changed.
    let result = preflight_check_stash(&mut repo, &plan, stash_count_at_plan);
    assert!(
        result.is_err(),
        "preflight_check_stash must return Err when stash count changed since planning"
    );
}
