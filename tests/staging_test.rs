//! Integration tests for staging backend — T024
//!
//! All write operations are confined to `TempDir` repositories created within
//! each test.  The project repository and any other existing repository are
//! **never** touched.
//!
//! # Test inventory (≥ 12 cases)
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_stage_modified_file` | stage a modified tracked file → INDEX side changes |
//! | 2 | `test_stage_new_untracked_file` | stage an untracked (new) file → INDEX_NEW |
//! | 3 | `test_stage_deleted_file` | stage a deleted file → INDEX_DELETED |
//! | 4 | `test_stage_does_not_change_workdir` | WT content is byte-for-byte identical after stage |
//! | 5 | `test_unstage_modified_file` | unstage a staged modification → back to WT_MODIFIED |
//! | 6 | `test_unstage_new_file_becomes_untracked` | unstage a new file → untracked |
//! | 7 | `test_unstage_does_not_change_workdir` | WT content is byte-for-byte identical after unstage |
//! | 8 | `test_unborn_repo_stage_and_initial_commit` | unborn HEAD: stage + execute_commit → first commit |
//! | 9 | `test_unstaged_file_diff_returns_wt_change` | unstaged_file_diff returns WT modification |
//! | 10 | `test_staged_file_diff_returns_index_change` | staged_file_diff returns index change |
//! | 11 | `test_partial_stage_diffs_are_independent` | after stage + extra WT edit, staged/unstaged diffs differ |
//! | 12 | `test_plan_commit_blocker_empty_message` | plan_commit blocks on empty message |
//! | 13 | `test_plan_commit_blocker_nothing_staged` | plan_commit blocks on empty staged |
//! | 14 | `test_plan_commit_warning_unstaged_remains` | plan_commit warns when unstaged remain |
//! | 15 | `test_execute_commit_creates_commit_and_clears_staged` | execute_commit creates commit, unstaged remain |

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    stage_file, unstage_file,
    unstaged_file_diff, staged_file_diff,
    plan_commit, execute_commit,
    DiffLineKind,
    working_tree_status,
};

// ────────────────────────────────────────────────────────────
// Helpers
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

/// Read `dir/name` as a String.
fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).expect("read_file failed")
}

/// Build a minimal repo with one commit on `main`.  HEAD is clean.
///
/// Returns `(repo_dir, Repository)`.
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

/// Build a fresh repo with **no commits** (unborn HEAD on `main`).
fn build_unborn_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    let repo = Repository::open(d).expect("failed to open repo");
    (d.to_path_buf(), repo)
}

// ────────────────────────────────────────────────────────────
// Test 1: stage a modified tracked file
// ────────────────────────────────────────────────────────────

#[test]
fn test_stage_modified_file() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Modify the tracked file.
    write_file(&dir, "README.md", "# modified\n");

    // Verify it is currently unstaged (WT_MODIFIED).
    let status_before = working_tree_status(&repo).unwrap();
    assert!(
        !status_before.unstaged.is_empty(),
        "README.md should appear as unstaged before stage_file"
    );
    assert!(
        status_before.staged.is_empty(),
        "README.md should not be staged yet"
    );

    // Stage.
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    // Now it should be in staged, not in unstaged.
    let status_after = working_tree_status(&repo).unwrap();
    assert!(
        !status_after.staged.is_empty(),
        "README.md should appear as staged after stage_file"
    );
    assert!(
        status_after.unstaged.is_empty(),
        "README.md should not be unstaged after stage_file (clean WT)"
    );
}

// ────────────────────────────────────────────────────────────
// Test 2: stage an untracked (new) file
// ────────────────────────────────────────────────────────────

#[test]
fn test_stage_new_untracked_file() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    write_file(&dir, "new_file.txt", "hello\n");

    let status_before = working_tree_status(&repo).unwrap();
    assert!(
        status_before.untracked.iter().any(|p| p.to_string_lossy().contains("new_file")),
        "new_file.txt should be untracked before stage"
    );

    stage_file(&repo, Path::new("new_file.txt")).expect("stage_file failed");

    let status_after = working_tree_status(&repo).unwrap();
    let staged_paths: Vec<_> = status_after.staged.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
    assert!(
        staged_paths.iter().any(|p| p.contains("new_file")),
        "new_file.txt should be staged after stage_file, got: {:?}",
        staged_paths
    );
    // Should no longer be untracked.
    assert!(
        !status_after.untracked.iter().any(|p| p.to_string_lossy().contains("new_file")),
        "new_file.txt should not be untracked after stage_file"
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: stage a deleted file
// ────────────────────────────────────────────────────────────

#[test]
fn test_stage_deleted_file() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Delete the tracked file from the working tree.
    std::fs::remove_file(dir.join("README.md")).expect("remove_file failed");

    let status_before = working_tree_status(&repo).unwrap();
    assert!(
        status_before.unstaged.iter().any(|f| {
            use kagi::git::ChangeKind;
            f.path.to_string_lossy().contains("README") && matches!(f.change, ChangeKind::Deleted)
        }),
        "README.md should appear as unstaged Deleted before stage"
    );

    stage_file(&repo, Path::new("README.md")).expect("stage_file for deletion failed");

    let status_after = working_tree_status(&repo).unwrap();
    let staged_paths: Vec<_> = status_after.staged.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
    assert!(
        staged_paths.iter().any(|p| p.contains("README")),
        "README.md deletion should be staged, got: {:?}",
        staged_paths
    );
    // Should no longer be unstaged.
    assert!(
        !status_after.unstaged.iter().any(|f| f.path.to_string_lossy().contains("README")),
        "README.md should not appear as unstaged after staging the deletion"
    );
}

// ────────────────────────────────────────────────────────────
// Test 4: stage does not change working tree
// ────────────────────────────────────────────────────────────

#[test]
fn test_stage_does_not_change_workdir() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    let content_before = "modified content for stage test\n";
    write_file(&dir, "README.md", content_before);

    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    // Working tree content must be byte-for-byte identical.
    let content_after = read_file(&dir, "README.md");
    assert_eq!(
        content_before, content_after,
        "stage_file must not change the working tree file content"
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: unstage a staged modification
// ────────────────────────────────────────────────────────────

#[test]
fn test_unstage_modified_file() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Modify and stage.
    write_file(&dir, "README.md", "staged modification\n");
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    let status_before = working_tree_status(&repo).unwrap();
    assert!(
        !status_before.staged.is_empty(),
        "README.md should be staged before unstage"
    );

    // Unstage.
    unstage_file(&repo, Path::new("README.md")).expect("unstage_file failed");

    let status_after = working_tree_status(&repo).unwrap();
    assert!(
        status_after.staged.is_empty(),
        "README.md should not be staged after unstage_file, got: {:?}",
        status_after.staged
    );
    // Should be unstaged (WT_MODIFIED) again.
    assert!(
        !status_after.unstaged.is_empty(),
        "README.md should appear as unstaged (WT_MODIFIED) after unstage"
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: unstage a new file → becomes untracked
// ────────────────────────────────────────────────────────────

#[test]
fn test_unstage_new_file_becomes_untracked() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Create and stage a new file.
    write_file(&dir, "brand_new.txt", "brand new\n");
    stage_file(&repo, Path::new("brand_new.txt")).expect("stage_file failed");

    let status_staged = working_tree_status(&repo).unwrap();
    assert!(
        status_staged.staged.iter().any(|f| f.path.to_string_lossy().contains("brand_new")),
        "brand_new.txt should be staged"
    );

    // Unstage → should become untracked (INDEX_NEW removed).
    unstage_file(&repo, Path::new("brand_new.txt")).expect("unstage_file failed");

    let status_after = working_tree_status(&repo).unwrap();
    // Not in staged.
    assert!(
        !status_after.staged.iter().any(|f| f.path.to_string_lossy().contains("brand_new")),
        "brand_new.txt should not be staged after unstage"
    );
    // In untracked.
    assert!(
        status_after.untracked.iter().any(|p| p.to_string_lossy().contains("brand_new")),
        "brand_new.txt should be untracked after unstage of new file, got: {:?}",
        status_after.untracked
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: unstage does not change working tree
// ────────────────────────────────────────────────────────────

#[test]
fn test_unstage_does_not_change_workdir() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    let content = "unstage working tree invariant\n";
    write_file(&dir, "README.md", content);
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    unstage_file(&repo, Path::new("README.md")).expect("unstage_file failed");

    // WT content must be byte-for-byte identical.
    let content_after = read_file(&dir, "README.md");
    assert_eq!(
        content, content_after,
        "unstage_file must not change the working tree file content"
    );
}

// ────────────────────────────────────────────────────────────
// Test 8: unborn HEAD — stage + initial commit
// ────────────────────────────────────────────────────────────

#[test]
fn test_unborn_repo_stage_and_initial_commit() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_unborn_repo(&tmp);

    // Create a file and stage it on unborn HEAD.
    write_file(&dir, "first.txt", "first file\n");
    stage_file(&repo, Path::new("first.txt")).expect("stage_file on unborn repo failed");

    let status = working_tree_status(&repo).unwrap();
    assert!(
        status.staged.iter().any(|f| f.path.to_string_lossy().contains("first")),
        "first.txt should be staged on unborn repo"
    );

    // Execute initial commit (no parents).
    let commit_id = execute_commit(&repo, "initial commit").expect("execute_commit on unborn failed");

    // HEAD should now exist and be attached.
    let repo2 = Repository::open(&dir).expect("re-open");
    let head_oid = repo2.head().expect("head").target().expect("head target");
    assert_eq!(
        head_oid.to_string(),
        commit_id.0,
        "HEAD should point to the new initial commit"
    );

    // Commit should have no parents.
    let commit = repo2.find_commit(head_oid).expect("find commit");
    assert_eq!(commit.parent_count(), 0, "initial commit should have no parents");

    // first.txt should exist in WT and tree.
    assert!(
        dir.join("first.txt").exists(),
        "first.txt should exist in working tree after commit"
    );
}

// ────────────────────────────────────────────────────────────
// Test 9: unstaged_file_diff returns WT changes
// ────────────────────────────────────────────────────────────

#[test]
fn test_unstaged_file_diff_returns_wt_change() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Modify README.md in WT but don't stage.
    write_file(&dir, "README.md", "# modified\nnew line\n");

    let diff = unstaged_file_diff(&repo, Path::new("README.md"))
        .expect("unstaged_file_diff failed");

    assert!(
        !diff.hunks.is_empty(),
        "unstaged diff should have at least one hunk for WT modification"
    );

    // Should contain Added lines (new content).
    let has_added = diff.hunks.iter().any(|h| {
        h.lines.iter().any(|l| matches!(l.kind, DiffLineKind::Added))
    });
    assert!(has_added, "unstaged diff should have Added lines for WT modification");
}

// ────────────────────────────────────────────────────────────
// Test 10: staged_file_diff returns index changes
// ────────────────────────────────────────────────────────────

#[test]
fn test_staged_file_diff_returns_index_change() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Modify and stage README.md.
    write_file(&dir, "README.md", "# staged modification\nextra line\n");
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    let diff = staged_file_diff(&repo, Path::new("README.md"))
        .expect("staged_file_diff failed");

    assert!(
        !diff.hunks.is_empty(),
        "staged diff should have at least one hunk"
    );

    // Should contain both Added and Removed lines (modification).
    let has_added = diff.hunks.iter().any(|h| {
        h.lines.iter().any(|l| matches!(l.kind, DiffLineKind::Added))
    });
    assert!(has_added, "staged diff should have Added lines");
}

// ────────────────────────────────────────────────────────────
// Test 11: partial stage — staged and unstaged diffs are independent
// ────────────────────────────────────────────────────────────

#[test]
fn test_partial_stage_diffs_are_independent() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Step 1: Modify README.md and stage it (captures "staged version").
    write_file(&dir, "README.md", "# staged content\n");
    stage_file(&repo, Path::new("README.md")).expect("stage_file (partial) failed");

    // Step 2: Further modify README.md in WT (but don't stage this change).
    write_file(&dir, "README.md", "# staged content\nFurther WT modification\n");

    // Now index = "# staged content\n"
    //     WT    = "# staged content\nFurther WT modification\n"

    let staged_diff = staged_file_diff(&repo, Path::new("README.md"))
        .expect("staged_file_diff failed");
    let unstaged_diff = unstaged_file_diff(&repo, Path::new("README.md"))
        .expect("unstaged_file_diff failed");

    // Staged diff: HEAD (original) → index (staged version).
    // Must contain hunk(s) showing the staged change.
    assert!(
        !staged_diff.hunks.is_empty(),
        "staged diff should show the staged change"
    );

    // Unstaged diff: index (staged version) → WT (further modified).
    // Must contain hunk(s) showing only the extra WT change.
    assert!(
        !unstaged_diff.hunks.is_empty(),
        "unstaged diff should show the additional WT change"
    );

    // The two diffs must differ.
    // Collect all Added line content from each diff.
    let staged_added: Vec<String> = staged_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| matches!(l.kind, DiffLineKind::Added))
        .map(|l| l.content.clone())
        .collect();

    let unstaged_added: Vec<String> = unstaged_diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| matches!(l.kind, DiffLineKind::Added))
        .map(|l| l.content.clone())
        .collect();

    // The unstaged diff should show "Further WT modification".
    let unstaged_has_further = unstaged_added
        .iter()
        .any(|l| l.contains("Further WT modification"));
    assert!(
        unstaged_has_further,
        "unstaged diff should show 'Further WT modification', got: {:?}",
        unstaged_added
    );

    // The staged diff should NOT show "Further WT modification" (not in index).
    let staged_has_further = staged_added
        .iter()
        .any(|l| l.contains("Further WT modification"));
    assert!(
        !staged_has_further,
        "staged diff must NOT show 'Further WT modification' (not staged yet), got: {:?}",
        staged_added
    );
}

// ────────────────────────────────────────────────────────────
// Test 12: plan_commit blocks on empty message
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_commit_blocker_empty_message() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Stage something so only the message blocker fires.
    write_file(&dir, "README.md", "something staged\n");
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    let plan = plan_commit(&repo, "").expect("plan_commit failed");

    assert!(
        !plan.blockers.is_empty(),
        "empty message should produce a blocker"
    );
    let has_empty_msg_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("empty") || b.contains("message"));
    assert!(
        has_empty_msg_blocker,
        "blocker should mention empty message, got: {:?}",
        plan.blockers
    );

    // Whitespace-only should also block.
    let plan2 = plan_commit(&repo, "   ").expect("plan_commit (whitespace) failed");
    assert!(
        !plan2.blockers.is_empty(),
        "whitespace-only message should also produce a blocker"
    );
}

// ────────────────────────────────────────────────────────────
// Test 13: plan_commit blocks when nothing staged
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_commit_blocker_nothing_staged() {
    let tmp = TempDir::new().unwrap();
    let (_dir, repo) = build_clean_repo(&tmp);

    // Repo is clean — nothing staged.
    let plan = plan_commit(&repo, "some message").expect("plan_commit failed");

    assert!(
        !plan.blockers.is_empty(),
        "nothing staged should produce a blocker"
    );
    let has_staged_blocker = plan
        .blockers
        .iter()
        .any(|b| b.contains("staged") || b.contains("Nothing"));
    assert!(
        has_staged_blocker,
        "blocker should mention nothing staged, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 14: plan_commit warns when unstaged changes remain
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_commit_warning_unstaged_remains() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Stage one file.
    write_file(&dir, "staged_change.txt", "staged\n");
    stage_file(&repo, Path::new("staged_change.txt")).expect("stage_file failed");

    // Leave another file unstaged.
    write_file(&dir, "README.md", "unstaged modification\n");

    let plan = plan_commit(&repo, "my commit message").expect("plan_commit failed");

    // No blockers (we have staged files and a message).
    assert!(
        plan.blockers.is_empty(),
        "should have no blockers when message + staged exist, got: {:?}",
        plan.blockers
    );

    // Should have a warning about the unstaged file.
    assert!(
        !plan.warnings.is_empty(),
        "should have a warning about the unstaged/untracked files that won't be committed"
    );
    let has_not_included_warning = plan
        .warnings
        .iter()
        .any(|w| w.contains("NOT") || w.contains("not") || w.contains("remain"));
    assert!(
        has_not_included_warning,
        "warning should mention files not included in commit, got: {:?}",
        plan.warnings
    );
}

// ────────────────────────────────────────────────────────────
// Test 15: execute_commit creates commit, staged clears, unstaged remains
// ────────────────────────────────────────────────────────────

#[test]
fn test_execute_commit_creates_commit_and_clears_staged() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = build_clean_repo(&tmp);

    // Capture HEAD before.
    let head_before = repo.head().expect("head").target().expect("head target").to_string();

    // Stage a change.
    let staged_content = "committed content\n";
    write_file(&dir, "README.md", staged_content);
    stage_file(&repo, Path::new("README.md")).expect("stage_file failed");

    // Also create an unstaged / untracked file.
    write_file(&dir, "not_staged.txt", "not staged\n");
    let unstaged_content = "unstaged modification\n";
    write_file(&dir, "README.md", &format!("{}{}", staged_content, unstaged_content));
    // Stage original content, then further modify WT so unstaged change remains.
    // Reset: re-stage only the first version.
    write_file(&dir, "README.md", staged_content);
    stage_file(&repo, Path::new("README.md")).expect("re-stage failed");
    // Add unstaged change after staging.
    write_file(&dir, "README.md", &format!("{}{}", staged_content, unstaged_content));

    // Execute commit.
    let new_id = execute_commit(&repo, "test commit message").expect("execute_commit failed");

    // HEAD must have advanced.
    let repo2 = Repository::open(&dir).expect("re-open");
    let head_after = repo2.head().expect("head").target().expect("head target").to_string();
    assert_ne!(head_before, head_after, "HEAD must advance after commit");
    assert_eq!(head_after, new_id.0, "HEAD must point to the new commit");

    // New commit must have one parent.
    let new_commit = repo2
        .find_commit(git2::Oid::from_str(&new_id.0).unwrap())
        .expect("find new commit");
    assert_eq!(new_commit.parent_count(), 1, "new commit should have one parent");
    assert_eq!(
        new_commit.parent(0).unwrap().id().to_string(),
        head_before,
        "parent of new commit must be old HEAD"
    );

    // Staged should now be empty (we committed it).
    let status_after = working_tree_status(&repo2).unwrap();
    assert!(
        status_after.staged.is_empty(),
        "staged must be empty after commit, got: {:?}",
        status_after.staged
    );

    // Unstaged change (the extra line we wrote after staging) must still exist.
    assert!(
        !status_after.unstaged.is_empty() || !status_after.untracked.is_empty(),
        "unstaged/untracked changes must remain after commit (WT not touched)"
    );

    // WT content of README.md must still have the post-stage modification.
    let wt_content = read_file(&dir, "README.md");
    assert!(
        wt_content.contains(unstaged_content.trim()),
        "WT content must still contain the unstaged modification after commit"
    );
}

// ── T-UI-002: batch stage / unstage ──
#[test]
fn test_stage_files_batch() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();
    for i in 1..=5 {
        write_file(dir, &format!("f{}.txt", i), "x\n");
    }
    let paths: Vec<std::path::PathBuf> = (1..=5).map(|i| std::path::PathBuf::from(format!("f{}.txt", i))).collect();
    let n = kagi::git::stage_files(&repo, &paths).unwrap();
    assert_eq!(n, 5);
    let st = working_tree_status(&repo).unwrap();
    assert_eq!(st.staged.len(), 5);
    assert!(st.untracked.is_empty());
}

#[test]
fn test_unstage_files_batch() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp);
    let dir = tmp.path();
    for i in 1..=4 {
        write_file(dir, &format!("g{}.txt", i), "y\n");
        git(dir, &["add", &format!("g{}.txt", i)]);
    }
    let paths: Vec<std::path::PathBuf> = (1..=4).map(|i| std::path::PathBuf::from(format!("g{}.txt", i))).collect();
    let n = kagi::git::unstage_files(&repo, &paths).unwrap();
    assert_eq!(n, 4);
    let st = working_tree_status(&repo).unwrap();
    assert!(st.staged.is_empty());
    assert_eq!(st.untracked.len(), 4);
}
