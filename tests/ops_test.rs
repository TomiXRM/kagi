//! Integration tests for checkout operation pipeline (T013),
//! create-branch operation pipeline (T014), and
//! stash push / apply operation pipeline (T015).
//!
//! All write operations are confined to `TempDir` repositories created within
//! each test.  This project's own repository and any other existing repository
//! are **never** touched.

use std::path::Path;
use std::process::Command;

use git2::{BranchType, Repository};
use tempfile::TempDir;

use kagi_git::{
    ops::{
        execute_checkout, execute_checkout_commit, execute_cherry_pick, execute_create_branch,
        execute_stash_apply, execute_stash_push, plan_checkout, plan_checkout_commit,
        plan_cherry_pick, plan_create_branch, plan_create_branch_with_checkout, plan_stash_apply,
        plan_stash_push, preflight_check, preflight_check_stash,
    },
    snapshot, CommitId, Head,
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
        plan.predicted.head, "branch: feature/one",
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
// Test 2: dirty repo — non-overlapping changes carry over (no blocker);
//         only changes that collide with the checkout block.
// ────────────────────────────────────────────────────────────

// README.md is identical on `main` and `feature/one` (feature only adds
// feat.txt), so a local edit to it does NOT overlap the checkout. The user
// should be able to switch directly: no blocker, a "carried over" warning, and
// a safe checkout that preserves the edit and moves HEAD.
#[test]
fn test_plan_dirty_unstaged_non_overlapping_no_blocker_carries_over() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Modify README.md without staging (not part of the main→feature/one diff).
    write_file(&repo_dir, "README.md", "modified content\n");

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "non-overlapping dirty change should NOT block, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("carried over")),
        "should warn that changes carry over, got: {:?}",
        plan.warnings
    );

    // And the safe checkout actually succeeds, preserving the edit + moving HEAD.
    preflight_check(&repo, &plan).expect("preflight failed");
    execute_checkout(&repo, "feature/one").expect("execute failed");
    assert_eq!(
        std::fs::read_to_string(repo_dir.join("README.md")).unwrap(),
        "modified content\n",
        "local edit should carry over to feature/one"
    );
    let repo2 = Repository::open(&repo_dir).expect("re-open");
    assert_eq!(
        repo2.head().unwrap().shorthand().unwrap_or(""),
        "feature/one",
        "HEAD should be on feature/one"
    );
}

// A newly-staged file that does not exist on the target branch cannot collide
// with the checkout either → no blocker.
#[test]
fn test_plan_dirty_staged_non_overlapping_no_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    // Stage a new file (absent from both trees → not in the checkout diff).
    write_file(&repo_dir, "staged.txt", "staged\n");
    git(&repo_dir, &["add", "staged.txt"]);

    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "non-overlapping staged change should NOT block, got: {:?}",
        plan.blockers
    );
}

// When a locally-modified tracked file IS part of the checkout diff, a safe
// checkout would be refused — so the plan must block and point at stash.
#[test]
fn test_plan_dirty_overlapping_has_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, _repo) = build_two_branch_repo(&tmp);

    // Make README.md differ between main and feature/one.
    git(&repo_dir, &["checkout", "-q", "feature/one"]);
    write_file(&repo_dir, "README.md", "feature version\n");
    git(&repo_dir, &["commit", "-aqm", "feature edits README"]);
    git(&repo_dir, &["checkout", "-q", "main"]);

    // Now locally modify README.md on main → overlaps the main→feature diff.
    write_file(&repo_dir, "README.md", "my local edit\n");

    // Re-open after the external commit so git2's ODB sees the new feature tip.
    let repo = Repository::open(&repo_dir).expect("reopen repo");
    let plan = plan_checkout(&repo, "feature/one").expect("plan_checkout failed");

    assert!(
        !plan.blockers.is_empty(),
        "overlapping dirty change must block"
    );
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().to_lowercase().contains("stash")),
        "blocker should point at stash, got: {:?}",
        plan.blockers
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
        .any(|b| b.message_en().contains("not exist") || b.message_en().contains("does not exist"));
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
        .any(|b| b.message_en().contains("already") || b.message_en().contains("current HEAD"));
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
        plan.recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains("main"),
        "recovery text should mention 'main' (the original branch), got: {:?}",
        plan.recovery
    );
    assert!(
        plan.recovery
            .as_ref()
            .map(|r| r.message_en())
            .unwrap_or_default()
            .contains("reflog"),
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

fn branch_commit_id(repo: &Repository, branch: &str) -> CommitId {
    let branch = repo
        .find_branch(branch, BranchType::Local)
        .expect("find branch");
    CommitId(branch.get().target().expect("branch target").to_string())
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).expect("read file")
}

fn build_readme_conflict_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository, CommitId) {
    let d = tmp.path();

    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    write_file(d, "README.md", "base\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "base"]);

    git(d, &["checkout", "-qb", "target"]);
    write_file(d, "README.md", "target\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "target readme"]);
    let repo = Repository::open(d).expect("open repo");
    let target = head_commit_id(&repo);

    git(d, &["checkout", "-q", "main"]);
    let repo = Repository::open(d).expect("reopen repo");
    (d.to_path_buf(), repo, target)
}

// ────────────────────────────────────────────────────────────
// T-CM-041: detached checkout commit tests
// ────────────────────────────────────────────────────────────

#[test]
fn test_checkout_commit_plan_warns_and_execute_detaches_head() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);
    let target = branch_commit_id(&repo, "feature/one");

    let plan = plan_checkout_commit(&repo, &target).expect("plan_checkout_commit failed");
    assert!(
        plan.blockers.is_empty(),
        "checkout commit should have no blockers, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.predicted.head.starts_with("detached: "),
        "predicted head should show detached state, got: {}",
        plan.predicted.head
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.message_en().contains("detached HEAD"))
            && plan
                .warnings
                .iter()
                .any(|w| w.message_en().contains("Create branch")),
        "plan should warn about detached HEAD and branch creation, got: {:?}",
        plan.warnings
    );

    preflight_check(&repo, &plan).expect("preflight failed");
    execute_checkout_commit(&repo, &target).expect("execute_checkout_commit failed");

    let mut repo2 = Repository::open(&repo_dir).expect("re-open repo");
    let snap = snapshot(&mut repo2, 100).expect("snapshot after checkout commit");
    assert!(
        matches!(&snap.head, Head::Detached { target: actual } if actual == &target.0),
        "HEAD should be detached at target after checkout commit, got: {:?}",
        snap.head
    );
    assert!(
        repo_dir.join("feat.txt").exists(),
        "target tree should be checked out"
    );
}

#[test]
fn test_checkout_commit_dirty_safe_checkout_fails_without_moving_head() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, target) = build_readme_conflict_repo(&tmp);

    write_file(&repo_dir, "README.md", "local dirty\n");
    let plan = plan_checkout_commit(&repo, &target).expect("plan_checkout_commit failed");
    // W15-ASYNCOPS / BUG-2: README.md is locally modified AND differs in the
    // target commit (overlap). The in-memory dry-run promotes the dirty warning
    // to a blocker so the plan matches what safe-mode `execute` does (refuse).
    assert!(
        !plan.blockers.is_empty(),
        "overlapping dirty worktree should block, got blockers: {:?}",
        plan.blockers
    );
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("README.md")),
        "blocker should name the conflicting file, got: {:?}",
        plan.blockers
    );

    // Execution still refuses and preserves the local edit (data-safety), even
    // when driven past the plan's blocker.
    let result = execute_checkout_commit(&repo, &target);
    assert!(
        result.is_err(),
        "safe checkout should refuse dirty overwrite"
    );

    let mut repo2 = Repository::open(&repo_dir).expect("re-open repo");
    let snap = snapshot(&mut repo2, 100).expect("snapshot after failed checkout commit");
    assert!(
        matches!(&snap.head, Head::Attached { branch, .. } if branch == "main"),
        "HEAD should remain on main after failed checkout, got: {:?}",
        snap.head
    );
    assert_eq!(read_file(&repo_dir, "README.md"), "local dirty\n");
}

#[test]
fn test_checkout_commit_preflight_aborts_when_head_changed() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);
    let target = branch_commit_id(&repo, "feature/one");
    let plan = plan_checkout_commit(&repo, &target).expect("plan_checkout_commit failed");

    write_file(&repo_dir, "extra.txt", "extra\n");
    git(&repo_dir, &["add", "extra.txt"]);
    git(&repo_dir, &["commit", "-qm", "extra commit"]);

    let result = preflight_check(&repo, &plan);
    assert!(
        result.is_err(),
        "preflight_check should return Err when HEAD changed before checkout commit"
    );
}

// ── T014-1: normal case — branch created, HEAD unchanged, WT unchanged ──

#[test]
fn test_create_branch_normal_creates_branch() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch(&repo, "new-feature", &at).expect("plan_create_branch failed");

    assert!(
        plan.blockers.is_empty(),
        "no blockers expected for valid name + existing commit, got: {:?}",
        plan.blockers
    );

    // Execute.
    execute_create_branch(&repo, "new-feature", &at).expect("execute_create_branch failed");

    // Branch must exist.
    let branch_exists = repo
        .find_branch("new-feature", git2::BranchType::Local)
        .is_ok();
    assert!(
        branch_exists,
        "branch 'new-feature' should exist after creation"
    );

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

    execute_create_branch(&repo, "stable-branch", &at).expect("execute_create_branch failed");

    // HEAD oid must be unchanged.
    let head_oid_after = repo
        .head()
        .unwrap()
        .target()
        .map(|o| o.to_string())
        .unwrap_or_default();
    assert_eq!(
        head_oid_before, head_oid_after,
        "HEAD OID should not change"
    );

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
    let plan = plan_create_branch(&repo, "main", &at).expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "creating a branch with an existing name should have a blocker"
    );
    let has_already_exists = plan.blockers.iter().any(|b| {
        b.message_en().contains("already exists") || b.message_en().contains("already exist")
    });
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
    let plan = plan_create_branch(&repo, "has space", &at).expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "branch name with space should produce a blocker"
    );
    let has_invalid = plan
        .blockers
        .iter()
        .any(|b| b.message_en().contains("not a valid") || b.message_en().contains("invalid"));
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
    let plan = plan_create_branch(&repo, "feat..broken", &at).expect("plan_create_branch failed");

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
    let plan = plan_create_branch(&repo, "-bad-name", &at).expect("plan_create_branch failed");

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
    let plan = plan_create_branch(&repo, "", &at).expect("plan_create_branch failed");

    assert!(
        !plan.blockers.is_empty(),
        "empty branch name should produce a blocker"
    );
}

#[test]
fn test_create_branch_with_checkout_predicts_new_head() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo) = build_two_branch_repo(&tmp);

    let at = head_commit_id(&repo);
    let plan = plan_create_branch_with_checkout(&repo, "checkout-me", &at, true)
        .expect("plan_create_branch_with_checkout failed");

    assert!(
        plan.blockers.is_empty(),
        "unexpected blockers: {:?}",
        plan.blockers
    );
    assert_eq!(plan.predicted.head, "branch: checkout-me");
    assert!(plan.title.message_en().contains("and checkout"));
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
    let feature_oid = feature_branch.get().target().expect("feature/one target");
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

    let plan =
        plan_stash_push(&mut repo, Some("test stash"), true).expect("plan_stash_push failed");

    assert!(
        plan.blockers.is_empty(),
        "dirty repo should have no blockers for stash push, got: {:?}",
        plan.blockers
    );

    // Execute.
    let mut repo2 = Repository::open(&repo_dir).expect("re-open");
    execute_stash_push(&mut repo2, Some("test stash"), true).expect("execute_stash_push failed");

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

    let plan = plan_stash_push(&mut repo, None, true).expect("plan_stash_push failed");

    assert!(
        !plan.blockers.is_empty(),
        "clean repo should have a blocker for stash push (nothing to stash)"
    );
    let has_nothing = plan
        .blockers
        .iter()
        .any(|b| b.message_en().contains("Nothing to stash") || b.message_en().contains("clean"));
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

    let plan = plan_stash_push(&mut repo, None, true).expect("plan_stash_push failed");

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
    execute_stash_push(&mut repo, Some("wip"), true).expect("execute_stash_push failed");

    // Verify clean + 1 stash.
    {
        let snap = snapshot(&mut repo, 100).expect("snapshot");
        assert!(!snap.status.is_dirty(), "should be clean after push");
        assert_eq!(snap.stashes.len(), 1, "stash count should be 1");
    }

    // Plan apply at index 0.
    let plan = plan_stash_apply(&mut repo, 0).expect("plan_stash_apply failed");

    assert!(
        plan.blockers.is_empty(),
        "clean repo with stash should have no blockers for apply, got: {:?}",
        plan.blockers
    );

    // Execute apply.
    execute_stash_apply(&mut repo, 0).expect("execute_stash_apply failed");

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
    execute_stash_push(&mut repo, None, true).expect("execute_stash_push failed");

    // Now dirty the working tree again (unstaged change).
    write_file(&repo_dir, "README.md", "new dirty\n");

    // Plan apply: should be blocked because working tree is dirty.
    let plan = plan_stash_apply(&mut repo, 0).expect("plan_stash_apply failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty working tree should produce a blocker for stash apply"
    );
    let has_dirty_blocker = plan.blockers.iter().any(|b| {
        b.message_en().contains("dirty")
            || b.message_en().contains("staged")
            || b.message_en().contains("modified")
    });
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
    let plan = plan_stash_apply(&mut repo, 0).expect("plan_stash_apply failed");

    assert!(
        !plan.blockers.is_empty(),
        "index out of range should produce a blocker"
    );
    let has_range = plan
        .blockers
        .iter()
        .any(|b| b.message_en().contains("out of range") || b.message_en().contains("range"));
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
    execute_stash_push(&mut repo, Some("round-trip stash"), true)
        .expect("execute_stash_push failed");

    // File must be reverted to its committed content.
    let file_after_push =
        std::fs::read_to_string(repo_dir.join("README.md")).expect("read README.md after push");
    assert_ne!(
        file_after_push, original_content,
        "file content should differ from stashed content after push"
    );

    // Apply.
    execute_stash_apply(&mut repo, 0).expect("execute_stash_apply failed");

    // File must be restored to original_content.
    let file_after_apply =
        std::fs::read_to_string(repo_dir.join("README.md")).expect("read README.md after apply");
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
    execute_stash_push(&mut repo, Some("first"), true).expect("first push failed");

    // Plan apply at index 0.
    let plan = plan_stash_apply(&mut repo, 0).expect("plan_stash_apply failed");
    assert!(plan.blockers.is_empty(), "should have no blockers");

    let stash_count_at_plan = plan.stash_count_at_plan();

    // Simulate another stash being added between plan and execution.
    write_file(&repo_dir, "extra.txt", "extra content\n");
    execute_stash_push(&mut repo, Some("extra stash"), true).expect("second push failed");

    // preflight_check_stash must detect the count changed.
    let result = preflight_check_stash(&mut repo, &plan, stash_count_at_plan);
    assert!(
        result.is_err(),
        "preflight_check_stash must return Err when stash count changed since planning"
    );
}

// ────────────────────────────────────────────────────────────
// T016: cherry-pick tests
// ────────────────────────────────────────────────────────────

/// Build a repo with two branches diverged from `main`:
/// - `main`: initial commit + "main-only" file
/// - `feature/two`: initial commit + "feature-two" file
///
/// HEAD is on `main`.  Returns (repo_dir, repo, feature_commit_id).
fn build_cherry_pick_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository, CommitId) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // Initial commit on main.
    write_file(d, "base.txt", "base content\n");
    git(d, &["add", "base.txt"]);
    git(d, &["commit", "-qm", "initial commit"]);

    // Create feature/two from main and add a new file.
    git(d, &["checkout", "-qb", "feature/two"]);
    write_file(d, "feat_two.txt", "feature two content\n");
    git(d, &["add", "feat_two.txt"]);
    git(d, &["commit", "-qm", "add feat_two.txt"]);

    // Capture the feature commit id.
    let repo_tmp = Repository::open(d).expect("open repo");
    let feature_oid = repo_tmp
        .head()
        .expect("head")
        .target()
        .expect("head target");
    let feature_id = CommitId(feature_oid.to_string());

    // Return to main.
    git(d, &["checkout", "-q", "main"]);

    let repo = Repository::open(d).expect("re-open repo on main");
    (d.to_path_buf(), repo, feature_id)
}

// ── T016-1: normal case — plan has no blockers, correct preview_files ───────

#[test]
fn test_cherry_pick_plan_normal_no_blockers() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);
    let _ = repo_dir;

    let plan = plan_cherry_pick(&repo, &feature_id).expect("plan_cherry_pick failed");

    assert!(
        plan.blockers.is_empty(),
        "clean repo + valid commit should have no blockers, got: {:?}",
        plan.blockers
    );
    // Plan must not have modified HEAD or WT.
    let repo2 = Repository::open(tmp.path()).expect("re-open");
    let head_branch = repo2
        .head()
        .expect("head")
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert_eq!(
        head_branch, "main",
        "plan_cherry_pick must not change HEAD branch"
    );

    // preview_files must be non-empty (feat_two.txt should appear).
    assert!(
        !plan.preview_files.is_empty(),
        "preview_files must be non-empty for a normal cherry-pick"
    );
    let has_feat_two = plan
        .preview_files
        .iter()
        .any(|f| f.path.to_string_lossy().contains("feat_two"));
    assert!(
        has_feat_two,
        "preview_files should include feat_two.txt, got: {:?}",
        plan.preview_files
            .iter()
            .map(|f| f.path.display().to_string())
            .collect::<Vec<_>>()
    );
}

// ── T016-2: normal execute — new commit on HEAD, message/author preserved ───

#[test]
fn test_cherry_pick_execute_normal() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);

    // Capture HEAD before.
    let head_before = repo
        .head()
        .expect("head")
        .target()
        .expect("target")
        .to_string();

    let plan = plan_cherry_pick(&repo, &feature_id).expect("plan_cherry_pick failed");
    assert!(plan.blockers.is_empty(), "expected no blockers");

    preflight_check(&repo, &plan).expect("preflight failed");

    let new_id = execute_cherry_pick(&repo, &feature_id).expect("execute_cherry_pick failed");

    // Head must have advanced.
    let repo2 = Repository::open(&repo_dir).expect("re-open");
    let head_after = repo2
        .head()
        .expect("head")
        .target()
        .expect("target")
        .to_string();
    assert_ne!(
        head_before, head_after,
        "HEAD must advance after cherry-pick"
    );
    assert_eq!(head_after, new_id.0, "HEAD must point to the new commit");

    // New commit must be on 'main'.
    let head_branch = repo2
        .head()
        .expect("head")
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert_eq!(head_branch, "main", "HEAD branch must still be main");

    // Parent of new commit must be old HEAD.
    let new_commit = repo2
        .find_commit(git2::Oid::from_str(&new_id.0).unwrap())
        .expect("find new commit");
    assert_eq!(
        new_commit.parent_count(),
        1,
        "new commit should have one parent"
    );
    let parent_oid = new_commit.parent(0).expect("parent").id().to_string();
    assert_eq!(
        parent_oid, head_before,
        "parent of new commit must be old HEAD"
    );

    // Message must match the cherry-picked commit.
    let original_commit = repo2
        .find_commit(git2::Oid::from_str(&feature_id.0).unwrap())
        .expect("find feature commit");
    let original_msg = original_commit.message().expect("original message");
    let new_msg = new_commit.message().expect("new commit message");
    assert_eq!(
        new_msg, original_msg,
        "cherry-pick must preserve commit message"
    );

    // Author must match.
    let orig_author = original_commit.author();
    let new_author = new_commit.author();
    assert_eq!(
        orig_author.name(),
        new_author.name(),
        "author name must be preserved"
    );
    assert_eq!(
        orig_author.email(),
        new_author.email(),
        "author email must be preserved"
    );

    // Working tree must reflect the cherry-picked file.
    assert!(
        repo_dir.join("feat_two.txt").exists(),
        "feat_two.txt must exist in working tree after cherry-pick"
    );

    // Status must be clean.
    let mut repo3 = Repository::open(&repo_dir).expect("re-open3");
    let snap = snapshot(&mut repo3, 100).expect("snapshot after cherry-pick");
    assert!(
        !snap.status.is_dirty(),
        "working tree must be clean after cherry-pick, got: staged={} unstaged={}",
        snap.status.staged.len(),
        snap.status.unstaged.len()
    );
}

// ── T016-3: conflict prediction — plan blocked, WT untouched ────────────────

#[test]
fn test_cherry_pick_plan_conflict_blocker_wt_intact() {
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

    // Branch 'conflict-branch': modify file.txt to "line B\n"
    git(d, &["checkout", "-qb", "conflict-branch"]);
    write_file(d, "file.txt", "line B\n");
    git(d, &["add", "file.txt"]);
    git(d, &["commit", "-qm", "set line B"]);

    let repo_tmp = Repository::open(d).expect("open repo");
    let conflict_oid = repo_tmp.head().expect("head").target().expect("target");
    let conflict_id = CommitId(conflict_oid.to_string());

    // Return to main and also modify the same line.
    git(d, &["checkout", "-q", "main"]);
    write_file(d, "file.txt", "line C\n");
    git(d, &["add", "file.txt"]);
    git(d, &["commit", "-qm", "set line C"]);

    let repo = Repository::open(d).expect("open repo on main");

    // Capture WT content before plan.
    let wt_before = std::fs::read_to_string(d.join("file.txt")).expect("read file.txt before");

    let plan = plan_cherry_pick(&repo, &conflict_id).expect("plan_cherry_pick failed");

    // Must have a conflict blocker.
    assert!(
        !plan.blockers.is_empty(),
        "conflicting cherry-pick should produce blockers"
    );
    let has_conflict_blocker = plan
        .blockers
        .iter()
        .any(|b| b.message_en().contains("conflict") || b.message_en().contains("Conflict"));
    assert!(
        has_conflict_blocker,
        "blocker should mention conflict, got: {:?}",
        plan.blockers
    );

    // WT must be intact (plan must not touch working tree).
    let wt_after = std::fs::read_to_string(d.join("file.txt")).expect("read file.txt after");
    assert_eq!(
        wt_before, wt_after,
        "plan_cherry_pick must not modify working tree content"
    );

    // HEAD must be unchanged.
    let repo2 = Repository::open(d).expect("re-open");
    let head_branch = repo2
        .head()
        .expect("head")
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert_eq!(
        head_branch, "main",
        "HEAD branch must remain main after conflict plan"
    );
}

// ── T016-4: dirty working tree — plan blocked ──────────────────────────────

#[test]
fn test_cherry_pick_plan_dirty_wt_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);

    // Dirty the working tree.
    write_file(&repo_dir, "base.txt", "modified content\n");

    let plan = plan_cherry_pick(&repo, &feature_id).expect("plan_cherry_pick failed");

    assert!(
        !plan.blockers.is_empty(),
        "dirty working tree should produce blockers"
    );
    let has_dirty_blocker = plan.blockers.iter().any(|b| {
        b.message_en().contains("staged")
            || b.message_en().contains("modified")
            || b.message_en().contains("dirty")
            || b.message_en().contains("Working tree")
    });
    assert!(
        has_dirty_blocker,
        "blocker should mention dirty tree, got: {:?}",
        plan.blockers
    );

    // HEAD must be unchanged.
    let repo2 = Repository::open(tmp.path()).expect("re-open");
    let head_branch = repo2
        .head()
        .expect("head")
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert_eq!(head_branch, "main", "HEAD must remain main");
}

// ── T016-5: merge commit — plan blocked ─────────────────────────────────────

#[test]
fn test_cherry_pick_plan_merge_commit_blocker() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    // Initial commit.
    write_file(d, "base.txt", "base\n");
    git(d, &["add", "base.txt"]);
    git(d, &["commit", "-qm", "initial"]);

    // Create two branches diverging from main.
    git(d, &["checkout", "-qb", "side-a"]);
    write_file(d, "side_a.txt", "side a\n");
    git(d, &["add", "side_a.txt"]);
    git(d, &["commit", "-qm", "side a"]);

    git(d, &["checkout", "-q", "main"]);
    git(d, &["checkout", "-qb", "side-b"]);
    write_file(d, "side_b.txt", "side b\n");
    git(d, &["add", "side_b.txt"]);
    git(d, &["commit", "-qm", "side b"]);

    // Merge side-a into side-b to create a merge commit.
    git(
        d,
        &["merge", "-q", "--no-ff", "-m", "merge side-a", "side-a"],
    );

    // Capture merge commit id.
    let repo_tmp = Repository::open(d).expect("open repo");
    let merge_oid = repo_tmp.head().expect("head").target().expect("target");
    let merge_id = CommitId(merge_oid.to_string());

    // Return to main.
    git(d, &["checkout", "-q", "main"]);
    let repo = Repository::open(d).expect("open repo on main");

    let plan = plan_cherry_pick(&repo, &merge_id).expect("plan_cherry_pick failed");

    assert!(
        !plan.blockers.is_empty(),
        "merge commit should produce a blocker"
    );
    let has_merge_blocker = plan
        .blockers
        .iter()
        .any(|b| b.message_en().contains("merge") || b.message_en().contains("parent"));
    assert!(
        has_merge_blocker,
        "blocker should mention merge commit, got: {:?}",
        plan.blockers
    );

    // HEAD must be unchanged.
    let repo2 = Repository::open(d).expect("re-open");
    let head_branch = repo2
        .head()
        .expect("head")
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_default();
    assert_eq!(head_branch, "main", "HEAD must remain main");
}

// ── T016-6: HEAD-same commit — plan blocked ──────────────────────────────────

#[test]
fn test_cherry_pick_plan_head_same_blocker() {
    let tmp = TempDir::new().unwrap();
    let (_repo_dir, repo, _feature_id) = build_cherry_pick_repo(&tmp);

    // Get current HEAD commit id.
    let head_oid = repo.head().expect("head").target().expect("target");
    let head_id = CommitId(head_oid.to_string());

    let plan = plan_cherry_pick(&repo, &head_id).expect("plan_cherry_pick failed");

    assert!(
        !plan.blockers.is_empty(),
        "cherry-picking the current HEAD commit should produce a blocker"
    );
    let has_same_blocker = plan.blockers.iter().any(|b| {
        b.message_en().contains("current HEAD")
            || b.message_en().contains("same")
            || b.message_en().contains("HEAD commit")
    });
    assert!(
        has_same_blocker,
        "blocker should mention HEAD-same, got: {:?}",
        plan.blockers
    );
}

// ── T016-7: already-applied (empty result) — plan blocked ──────────────────

#[test]
fn test_cherry_pick_plan_already_applied_blocker() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);

    // Execute the cherry-pick first.
    let plan = plan_cherry_pick(&repo, &feature_id).expect("plan failed");
    assert!(
        plan.blockers.is_empty(),
        "first plan should have no blockers"
    );
    execute_cherry_pick(&repo, &feature_id).expect("execute failed");

    // Re-open the repo after the cherry-pick.
    let repo2 = Repository::open(&repo_dir).expect("re-open");

    // Now plan again — should be blocked (already applied).
    let plan2 = plan_cherry_pick(&repo2, &feature_id).expect("second plan failed");

    assert!(
        !plan2.blockers.is_empty(),
        "cherry-picking an already-applied commit should produce a blocker"
    );
    // Depending on timing, the blocker may be either:
    // - "no changes / already applied" (different commit hash) — preferred
    // - "HEAD same" (deterministic commit hash due to same tree/parent/author/timestamp)
    // Both are valid indicators that the commit cannot/should not be cherry-picked again.
    let has_applied_blocker = plan2.blockers.iter().any(|b| {
        b.message_en().contains("no changes")
                || b.message_en().contains("applied already")
                || b.message_en().contains("empty")
                || b.message_en().contains("current HEAD")  // HEAD-same check fires when hash is deterministic
                || b.message_en().contains("same")
    });
    assert!(
        has_applied_blocker,
        "blocker should indicate commit is already applied or HEAD-same, got: {:?}",
        plan2.blockers
    );
}

// ── T016-8: preview_files match actual changed files ────────────────────────

#[test]
fn test_cherry_pick_plan_preview_files_match() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);
    let _ = repo_dir;

    let plan = plan_cherry_pick(&repo, &feature_id).expect("plan_cherry_pick failed");

    assert!(plan.blockers.is_empty(), "expected no blockers");

    // preview_files should contain exactly feat_two.txt as Added.
    assert_eq!(
        plan.preview_files.len(),
        1,
        "should have exactly 1 preview file, got: {:?}",
        plan.preview_files
            .iter()
            .map(|f| f.path.display().to_string())
            .collect::<Vec<_>>()
    );
    let pf = &plan.preview_files[0];
    assert!(
        pf.path.to_string_lossy().contains("feat_two"),
        "preview file should be feat_two.txt, got: {}",
        pf.path.display()
    );
    use kagi_git::ChangeKind;
    assert!(
        matches!(pf.change, ChangeKind::Added),
        "change kind should be Added, got: {:?}",
        pf.change
    );
}

// ── T016-9: plan does not change repo state (status / HEAD / branch tips) ───

#[test]
fn test_cherry_pick_plan_does_not_change_repo() {
    let tmp = TempDir::new().unwrap();
    let (repo_dir, repo, feature_id) = build_cherry_pick_repo(&tmp);

    // Capture state before planning.
    let head_oid_before = repo
        .head()
        .expect("head")
        .target()
        .expect("target")
        .to_string();
    let wt_content_before = std::fs::read_to_string(repo_dir.join("base.txt")).expect("read");

    // Run the plan (may produce any result — we only care that repo is unchanged).
    let _ = plan_cherry_pick(&repo, &feature_id);

    // HEAD must not have changed.
    let repo2 = Repository::open(tmp.path()).expect("re-open");
    let head_oid_after = repo2
        .head()
        .expect("head")
        .target()
        .expect("target")
        .to_string();
    assert_eq!(
        head_oid_before, head_oid_after,
        "plan must not change HEAD OID"
    );

    // WT content must not have changed.
    let wt_content_after = std::fs::read_to_string(repo_dir.join("base.txt")).expect("read after");
    assert_eq!(
        wt_content_before, wt_content_after,
        "plan must not modify working tree"
    );

    // main branch tip must not have changed.
    let main_tip = repo2
        .find_branch("main", git2::BranchType::Local)
        .expect("find main")
        .get()
        .target()
        .expect("main tip")
        .to_string();
    assert_eq!(
        head_oid_before, main_tip,
        "main branch tip must not change after plan"
    );

    // Repo state must not be CHERRYPICK (no .git/CHERRY_PICK_HEAD).
    let cherry_pick_head = repo_dir.join(".git").join("CHERRY_PICK_HEAD");
    assert!(
        !cherry_pick_head.exists(),
        ".git/CHERRY_PICK_HEAD must not exist after plan (in-memory only)"
    );
}

// ── PM regression (found via T-HT-003): cherry-pick must update files that
// EXIST in the WT and are modified by the picked commit — not only create
// new files.  Same ref-ordering pitfall as the pull FF/merge paths.
#[test]
fn test_cherry_pick_updates_modified_existing_file() {
    use kagi_git::{execute_cherry_pick, CommitId};
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);

    std::fs::write(dir.join("shared.txt"), "v1\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    // Feature branch modifies the EXISTING file.
    git(dir, &["checkout", "-q", "-b", "feat"]);
    std::fs::write(dir.join("shared.txt"), "v2 from feat\n").unwrap();
    git(dir, &["commit", "-qam", "feat edit"]);
    let feat_sha = {
        let out = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Back to main with an unrelated extra commit (so cherry-pick isn't a no-op FF shape).
    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("other.txt"), "x\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "main work"]);

    let repo = git2::Repository::open(dir).unwrap();
    execute_cherry_pick(&repo, &CommitId(feat_sha)).expect("cherry-pick should succeed");

    // WT must contain the picked modification and be clean.
    assert_eq!(
        std::fs::read_to_string(dir.join("shared.txt")).unwrap(),
        "v2 from feat\n",
        "cherry-pick must materialise modifications to existing files"
    );
    let st = kagi_git::working_tree_status(&git2::Repository::open(dir).unwrap()).unwrap();
    assert!(!st.is_dirty(), "WT must be clean after cherry-pick");
}
