//! Integration tests for the discard operation pipeline (W17-DISCARD, ADR-0046).
//!
//! Verifies backup-then-discard semantics:
//! - discard of a modification restores the working tree from the index
//! - discard of an unstaged deletion restores the file from the index
//! - staged (index) content is left unchanged by discard
//! - the backup blob is readable from the ODB by the logged SHA, and equals the
//!   pre-discard working-tree content
//! - conflicted / untracked targets produce blockers (no working-tree change)
//!
//! All write operations are confined to `TempDir` repositories — never user repos.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{execute_discard, plan_discard, working_tree_status};

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

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(out.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).expect("read_file failed")
}

/// Build a minimal repo with `tracked.txt` committed. HEAD on `main`, clean.
fn build_repo(tmp: &TempDir) -> std::path::PathBuf {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);
    write_file(d, "tracked.txt", "committed\n");
    git(d, &["add", "tracked.txt"]);
    git(d, &["commit", "-qm", "initial commit"]);
    d.to_path_buf()
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-1: discard a modification → WT restored from index
// ────────────────────────────────────────────────────────────

#[test]
fn discard_modification_restores_from_index() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    // Unstaged modification.
    write_file(&d, "tracked.txt", "DIRTY EDIT\n");
    assert_eq!(read_file(&d, "tracked.txt"), "DIRTY EDIT\n");

    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.destructive, "discard plan must be destructive");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(outcome.backups[0].path, "tracked.txt");

    // WT restored to committed/index content.
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");

    // No longer unstaged.
    let status = working_tree_status(&repo).unwrap();
    assert!(
        !status
            .unstaged
            .iter()
            .any(|f| f.path == Path::new("tracked.txt")),
        "tracked.txt should have left the unstaged set"
    );

    // Backup blob holds the PRE-discard working-tree content.
    let blob = git_out(&d, &["cat-file", "-p", &outcome.backups[0].blob]);
    assert_eq!(blob, "DIRTY EDIT\n");
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-2: discard an unstaged deletion → file restored from index
// ────────────────────────────────────────────────────────────

#[test]
fn discard_unstaged_deletion_restores_file() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    // Delete the tracked file in the working tree (unstaged deletion).
    std::fs::remove_file(d.join("tracked.txt")).unwrap();
    assert!(!d.join("tracked.txt").exists());

    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(outcome.backups.len(), 1);

    // File restored from index.
    assert!(d.join("tracked.txt").exists(), "file should be restored");
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");

    let status = working_tree_status(&repo).unwrap();
    assert!(
        !status
            .unstaged
            .iter()
            .any(|f| f.path == Path::new("tracked.txt")),
        "tracked.txt should have left the unstaged set"
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-3: staged content is unchanged by discard
// ────────────────────────────────────────────────────────────

#[test]
fn discard_leaves_staged_content_unchanged() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    // Stage one version, then make a *further* unstaged edit on top.
    write_file(&d, "tracked.txt", "STAGED VERSION\n");
    git(&d, &["add", "tracked.txt"]);
    write_file(&d, "tracked.txt", "WORKTREE EDIT\n");

    // Sanity: file is both staged and unstaged now.
    let before = working_tree_status(&repo).unwrap();
    assert!(before
        .staged
        .iter()
        .any(|f| f.path == Path::new("tracked.txt")));
    assert!(before
        .unstaged
        .iter()
        .any(|f| f.path == Path::new("tracked.txt")));

    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");

    // WT now matches the STAGED (index) content, not HEAD.
    assert_eq!(read_file(&d, "tracked.txt"), "STAGED VERSION\n");

    // The staged change is still present (index untouched).
    let after = working_tree_status(&repo).unwrap();
    assert!(
        after
            .staged
            .iter()
            .any(|f| f.path == Path::new("tracked.txt")),
        "staged change must survive discard"
    );
    assert!(
        !after
            .unstaged
            .iter()
            .any(|f| f.path == Path::new("tracked.txt")),
        "unstaged change must be gone"
    );

    // The staged blob in the index must equal STAGED VERSION.
    let staged_blob = git_out(&d, &["show", ":tracked.txt"]);
    assert_eq!(staged_blob, "STAGED VERSION\n");

    // Backup captured the pre-discard WT content.
    let blob = git_out(&d, &["cat-file", "-p", &outcome.backups[0].blob]);
    assert_eq!(blob, "WORKTREE EDIT\n");
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-4: untracked target → file deleted, content backed up (ADR-0083)
// ────────────────────────────────────────────────────────────

#[test]
fn discard_untracked_deletes_file_and_backs_it_up() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    write_file(&d, "newfile.txt", "untracked body\n");

    let paths = vec!["newfile.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    // No longer a blocker — untracked discard is allowed (warns instead).
    assert!(
        plan.blockers.is_empty(),
        "untracked discard must not be blocked: {:?}",
        plan.blockers
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.to_lowercase().contains("deleted")),
        "plan should warn the file will be deleted: {:?}",
        plan.warnings
    );

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");

    // The untracked file is gone from disk …
    assert!(
        !d.join("newfile.txt").exists(),
        "untracked file must be deleted"
    );
    // … and no longer reported as untracked.
    let status = working_tree_status(&repo).expect("status");
    assert!(
        !status
            .untracked
            .iter()
            .any(|p| p.to_string_lossy() == "newfile.txt"),
        "file must leave the untracked set"
    );

    // The content is recoverable from the ODB via the backup blob SHA.
    let backup = outcome
        .backups
        .iter()
        .find(|b| b.path == "newfile.txt")
        .expect("a backup for the deleted file");
    let restored = git_out(&d, &["cat-file", "-p", &backup.blob]);
    assert_eq!(
        restored, "untracked body\n",
        "backup blob must hold the deleted file's content"
    );
}

// ADR-0083: discarding all untracked files in a new folder also removes the
// now-empty folder (the `-d` of `git clean -fd`).
#[test]
fn discard_untracked_prunes_now_empty_dirs() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    std::fs::create_dir_all(d.join("newdir/sub")).unwrap();
    write_file(&d, "newdir/inner.txt", "a\n");
    write_file(&d, "newdir/sub/deep.txt", "b\n");

    let paths = vec![
        "newdir/inner.txt".to_string(),
        "newdir/sub/deep.txt".to_string(),
    ];
    let plan = plan_discard(&repo, &paths).expect("plan");
    execute_discard(&repo, &plan, &paths).expect("execute");

    assert!(!d.join("newdir/sub/deep.txt").exists(), "file deleted");
    assert!(!d.join("newdir/inner.txt").exists(), "file deleted");
    assert!(
        !d.join("newdir/sub").exists(),
        "empty sub-directory must be pruned"
    );
    assert!(
        !d.join("newdir").exists(),
        "empty directory must be pruned (git clean -fd)"
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-5: conflicted target → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn discard_conflicted_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // Build a merge conflict on conflict.txt.
    write_file(&d, "conflict.txt", "base\n");
    git(&d, &["add", "conflict.txt"]);
    git(&d, &["commit", "-qm", "add conflict.txt"]);

    git(&d, &["checkout", "-qb", "branchA"]);
    write_file(&d, "conflict.txt", "from A\n");
    git(&d, &["commit", "-qam", "A edit"]);

    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["checkout", "-qb", "branchB"]);
    write_file(&d, "conflict.txt", "from B\n");
    git(&d, &["commit", "-qam", "B edit"]);

    // Merge A into B → conflict.
    let merge = Command::new("git")
        .args(["merge", "branchA"])
        .current_dir(&d)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", &d)
        .output()
        .expect("merge");
    assert!(!merge.status.success(), "merge should conflict");

    let repo = Repository::open(&d).unwrap();
    let status = working_tree_status(&repo).unwrap();
    assert!(
        status
            .conflicted
            .iter()
            .any(|p| p == Path::new("conflict.txt")),
        "conflict.txt should be conflicted"
    );

    let paths = vec!["conflict.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(
        !plan.blockers.is_empty(),
        "conflicted discard must be blocked"
    );
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.contains("conflict") || b.contains("Conflict")),
        "blocker should mention conflict: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-6: empty selection → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn discard_empty_selection_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    let paths: Vec<String> = Vec::new();
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(!plan.blockers.is_empty(), "empty selection must be blocked");
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-7: multi-file discard is one operation; oplog summary lists all
// ────────────────────────────────────────────────────────────

#[test]
fn discard_multi_file_one_outcome() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    write_file(&d, "second.txt", "two\n");
    git(&d, &["add", "second.txt"]);
    git(&d, &["commit", "-qm", "add second.txt"]);

    write_file(&d, "tracked.txt", "edit one\n");
    write_file(&d, "second.txt", "edit two\n");

    let paths = vec!["tracked.txt".to_string(), "second.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(
        outcome.backups.len(),
        2,
        "one backup per file, one operation"
    );

    let summary = outcome.oplog_summary();
    assert!(
        summary.contains("discarded 2 file(s)"),
        "summary: {}",
        summary
    );
    assert!(summary.contains("tracked.txt="), "summary: {}", summary);
    assert!(summary.contains("second.txt="), "summary: {}", summary);

    // Both restored.
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");
    assert_eq!(read_file(&d, "second.txt"), "two\n");
}
