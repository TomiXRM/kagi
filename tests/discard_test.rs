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

use kagi_domain::plan_note::{DiscardNote, PlanNote};
use kagi_git::{execute_discard, plan_discard, working_tree_status};

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
            .any(|w| w.message_en().to_lowercase().contains("deleted")),
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
            .any(|b| b.message_en().contains("conflict") || b.message_en().contains("Conflict")),
        "blocker should mention conflict: {:?}",
        plan.blockers
    );

    // The blocked plan must also be refused at EXECUTE time (defence in depth):
    // execute_discard has its own blocker gate + preflight, and the module doc
    // promises "no working-tree change" for a blocked discard.
    let before = read_file(&d, "conflict.txt");
    assert!(
        before.contains("<<<<<<<"),
        "precondition: conflict markers present: {}",
        before
    );

    let err = execute_discard(&repo, &plan, &paths)
        .expect_err("execute_discard must refuse a plan with blockers");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("blocker"),
        "error should name the blocker gate: {}",
        msg
    );

    // No working-tree change: the conflict markers are still there.
    let after = read_file(&d, "conflict.txt");
    assert_eq!(
        before, after,
        "a refused discard must not touch the working tree"
    );
    assert!(
        after.contains("<<<<<<<") && after.contains(">>>>>>>"),
        "conflict markers must survive the refused discard: {}",
        after
    );
    // And the file is still conflicted in the index.
    let status_after = working_tree_status(&repo).unwrap();
    assert!(
        status_after
            .conflicted
            .iter()
            .any(|p| p == Path::new("conflict.txt")),
        "conflict.txt must still be conflicted"
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
    assert!(
        plan.blockers
            .iter()
            .any(|b| matches!(b, PlanNote::Discard(DiscardNote::NothingSelected))),
        "expected DiscardNote::NothingSelected, got: {:?}",
        plan.blockers
    );
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

// ────────────────────────────────────────────────────────────
// TC-DISCARD-8: verify step — a target that cannot be restored errors
// ────────────────────────────────────────────────────────────

/// A CRLF blob in the index plus `text eol=lf` makes the file *permanently*
/// unstaged: checkout smudges the CRLF blob to LF, and status cleans that LF
/// back to LF, which still differs from the CRLF blob. So step 2 "succeeds"
/// while the target never leaves the unstaged set — exactly what step 3
/// (verify) exists to catch.
#[test]
fn discard_verify_catches_unrestorable_target() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // Commit a blob that keeps its CRLF bytes.
    write_file(&d, "crlf.txt", "one\r\ntwo\r\n");
    git(&d, &["config", "core.autocrlf", "false"]);
    git(&d, &["add", "crlf.txt"]);
    git(&d, &["commit", "-qm", "add crlf.txt"]);

    // Now declare it LF-in-worktree: the committed CRLF blob can never round-trip.
    write_file(&d, ".gitattributes", "crlf.txt text eol=lf\n");
    git(&d, &["add", ".gitattributes"]);
    git(&d, &["commit", "-qm", "add gitattributes"]);

    // Rewrite the same bytes so the file's mtime is newer than the index: git's
    // stat cache would otherwise be free to call it clean (identical size and
    // an older mtime), which would make this test racy.
    write_file(&d, "crlf.txt", "one\r\ntwo\r\n");

    let repo = Repository::open(&d).unwrap();
    let status = working_tree_status(&repo).unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .any(|f| f.path == Path::new("crlf.txt")),
        "precondition: crlf.txt must be unstaged, got {:?}",
        status.unstaged
    );

    // Issue #281: batch the unrestorable target with a normal one. The working
    // tree IS mutated (tracked.txt is stomped) before verify fails, so the
    // backup blob SHAs must survive the failure.
    write_file(&d, "tracked.txt", "PRECIOUS USER EDIT\n");

    let paths = vec!["tracked.txt".to_string(), "crlf.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths)
        .expect("#281: a post-mutation failure must still return the backups");
    assert!(
        outcome.is_partial(),
        "verify failure must be reported as a PARTIAL outcome, got {:?}",
        outcome
    );
    let msg = outcome.error.clone().unwrap();
    assert!(
        msg.contains("discard verify failed"),
        "expected the verify-step error, got: {}",
        msg
    );
    assert!(
        msg.contains("crlf.txt"),
        "error should name the target: {}",
        msg
    );
    assert_eq!(
        outcome.unverified,
        vec!["crlf.txt".to_string()],
        "per-path status: only crlf.txt was not discarded"
    );

    // The working tree WAS mutated: tracked.txt lost the user's edit …
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");
    // … and the ONLY route back to it is the backup blob, which must be in the
    // outcome (and therefore in the oplog) despite the failure.
    let backup = outcome
        .backups
        .iter()
        .find(|b| b.path == "tracked.txt")
        .expect("#281: tracked.txt backup must survive the failure");
    assert_eq!(
        git_out(&d, &["cat-file", "-p", &backup.blob]),
        "PRECIOUS USER EDIT\n"
    );

    // The oplog line carries both the recovery handle and the partial status.
    let summary = outcome.oplog_summary();
    assert!(summary.contains(&backup.blob), "summary: {}", summary);
    assert!(summary.contains("PARTIAL:"), "summary: {}", summary);
    assert!(
        summary.contains("not discarded: crlf.txt"),
        "summary: {}",
        summary
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-8b (issue #281): untracked removal that fails at target N still
// returns the backups for 1..N.
// ────────────────────────────────────────────────────────────

/// Forced by making the second target's parent directory non-writable, so
/// `remove_file` fails with PermissionDenied. Unix-only, and skipped when the
/// test runs as root (root ignores the mode bits, so the failure can't be
/// provoked); there is no portable Windows equivalent.
#[cfg(unix)]
#[test]
fn discard_partial_untracked_removal_keeps_backups() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    std::fs::create_dir_all(d.join("locked")).unwrap();
    write_file(&d, "first.txt", "first untracked\n");
    write_file(&d, "locked/second.txt", "second untracked\n");

    // Probe: can a 0o500 directory actually stop us? (No, if we are root.)
    std::fs::create_dir_all(d.join("probe")).unwrap();
    write_file(&d, "probe/p.txt", "p\n");
    std::fs::set_permissions(d.join("probe"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let is_root = std::fs::remove_file(d.join("probe/p.txt")).is_ok();
    std::fs::set_permissions(d.join("probe"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::remove_dir_all(d.join("probe")).unwrap();
    if is_root {
        eprintln!("skipped: running as root, directory permissions cannot block remove_file");
        return;
    }

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["first.txt".to_string(), "locked/second.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let outcome = execute_discard(&repo, &plan, &paths);
    // Restore before asserting so TempDir cleanup always works.
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o700)).unwrap();

    let outcome = outcome.expect("#281: a partial deletion must still return the backups");
    assert!(outcome.is_partial(), "outcome: {:?}", outcome);
    assert_eq!(
        outcome.backups.len(),
        2,
        "both step-1 backups survive: {:?}",
        outcome
    );
    assert_eq!(outcome.unverified, vec!["locked/second.txt".to_string()]);
    // 1..N-1 really were deleted — the backup is the only copy left.
    assert!(!d.join("first.txt").exists());
    let first = outcome
        .backups
        .iter()
        .find(|b| b.path == "first.txt")
        .unwrap();
    assert_eq!(
        git_out(&d, &["cat-file", "-p", &first.blob]),
        "first untracked\n"
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-9 (issue #282): a repo-relative target is resolved against the
// WORKDIR, never the process CWD — a same-named file in a subdirectory must
// not be touched.
// ────────────────────────────────────────────────────────────

#[test]
fn discard_relative_path_targets_the_workdir_root_not_a_shadow_file() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    std::fs::create_dir_all(d.join("src")).unwrap();
    write_file(&d, "a.txt", "root committed\n");
    write_file(&d, "src/a.txt", "sub committed\n");
    git(&d, &["add", "a.txt", "src/a.txt"]);
    git(&d, &["commit", "-qm", "add both a.txt"]);

    write_file(&d, "a.txt", "root DIRTY\n");
    write_file(&d, "src/a.txt", "sub DIRTY\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec!["a.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert!(!outcome.is_partial(), "outcome: {:?}", outcome);
    assert_eq!(outcome.backups.len(), 1);
    assert_eq!(
        outcome.backups[0].path, "a.txt",
        "the ROOT a.txt was chosen"
    );

    assert_eq!(
        read_file(&d, "a.txt"),
        "root committed\n",
        "target reverted"
    );
    assert_eq!(
        read_file(&d, "src/a.txt"),
        "sub DIRTY\n",
        "#282: the same-named file in a subdirectory must be untouched"
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-10 (issue #282): an absolute target behaves exactly like the
// repo-relative form.
// ────────────────────────────────────────────────────────────

#[test]
fn discard_absolute_path_matches_relative_form() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    write_file(&d, "tracked.txt", "DIRTY EDIT\n");

    let repo = Repository::open(&d).unwrap();
    let paths = vec![d.join("tracked.txt").to_string_lossy().into_owned()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(outcome.backups[0].path, "tracked.txt");
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");
}

/// git hash-object of `bytes` (does NOT write), for "does this blob exist" checks.
fn hash_object(dir: &Path, bytes: &[u8]) -> String {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Whether object `sha` exists in `dir`'s ODB (`git cat-file -e`).
fn object_exists(dir: &Path, sha: &str) -> bool {
    Command::new("git")
        .args(["cat-file", "-e", sha])
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .unwrap()
        .success()
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-324a (#324): discarding an untracked symlink must NOT read
// through the link — no repo-external bytes reach the ODB, the outside file
// is untouched, and the link itself is removed from disk.
// ────────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn discard_untracked_symlink_does_not_ingest_target_bytes() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);
    let repo = Repository::open(&d).unwrap();

    // A file OUTSIDE the repo with known, secret bytes.
    let outside = TempDir::new().unwrap();
    let secret_path = outside.path().join("secret.txt");
    let secret_bytes = b"OUTSIDE-REPO-SECRET-DO-NOT-INGEST\n";
    std::fs::write(&secret_path, secret_bytes).unwrap();

    // An untracked symlink inside the repo pointing at the outside file.
    let link = d.join("link");
    std::os::unix::fs::symlink(&secret_path, &link).unwrap();
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());

    let paths = vec!["link".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    let outcome = execute_discard(&repo, &plan, &paths).expect("execute");

    // (a) the outside file is untouched.
    assert_eq!(
        std::fs::read(&secret_path).unwrap(),
        secret_bytes,
        "the symlink target outside the repo must not be modified"
    );

    // (b) NO blob equal to the outside bytes was written to the ODB. The blob
    // SHA the outside bytes WOULD hash to must not exist — its presence is
    // exactly the #324 leak (fs::read followed the link into the ODB).
    let leaked_sha = hash_object(&d, secret_bytes);
    assert!(
        !object_exists(&d, &leaked_sha),
        "a blob equal to the outside bytes ({leaked_sha}) was written to the ODB \
         — the backup read followed the symlink (#324 leak)"
    );

    // The backup blob stores the LINK TARGET PATH, not the dereferenced content.
    let backup = outcome
        .backups
        .iter()
        .find(|b| b.path == "link")
        .expect("a backup for the discarded link");
    let stored = git_out(&d, &["cat-file", "-p", &backup.blob]);
    assert_eq!(
        stored,
        secret_path.to_string_lossy(),
        "backup must store the link target path, not the target's content"
    );

    // (c) the link is removed from disk.
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the symlink must be removed from the working tree"
    );
}

// ────────────────────────────────────────────────────────────
// TC-DISCARD-324b (#324): a dirty submodule is a plan-time blocker, so it
// never reaches the backup read (which would EISDIR-abort the whole batch).
// Discarding the OTHER dirty files completes.
// ────────────────────────────────────────────────────────────

#[test]
fn discard_dirty_submodule_is_blocked_and_other_files_complete() {
    let tmp = TempDir::new().unwrap();
    let d = build_repo(&tmp);

    // Build a separate repo to embed as a submodule.
    let subsrc = TempDir::new().unwrap();
    let s = subsrc.path();
    git(s, &["init", "-q", "-b", "main", "."]);
    git(s, &["config", "user.name", "Test"]);
    git(s, &["config", "user.email", "test@example.com"]);
    git(s, &["config", "commit.gpgsign", "false"]);
    write_file(s, "inner.txt", "inner v1\n");
    git(s, &["add", "inner.txt"]);
    git(s, &["commit", "-qm", "sub initial"]);

    // Add it as a submodule at path `sub` (file protocol needs opt-in).
    let url = s.to_string_lossy().to_string();
    git(
        &d,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            &url,
            "sub",
        ],
    );
    git(&d, &["commit", "-qm", "add submodule"]);

    // Make the submodule dirty (new commit inside → gitlink moves) → ` M sub`.
    let subwd = d.join("sub");
    write_file(&subwd, "inner.txt", "inner v2\n");
    git(&subwd, &["commit", "-aqm", "sub change"]);

    // And an ordinary dirty tracked file alongside it.
    write_file(&d, "tracked.txt", "DIRTY\n");

    let repo = Repository::open(&d).unwrap();

    // The submodule must be a plan-time BLOCKER (this is the #324 fix; reverting
    // it removes the blocker and execute would EISDIR-abort on fs::read(sub)).
    let plan_with_sub =
        plan_discard(&repo, &["sub".to_string(), "tracked.txt".to_string()]).expect("plan");
    assert!(
        plan_with_sub.blockers.iter().any(|b| matches!(
            b,
            PlanNote::Discard(DiscardNote::TargetSubmodule { path }) if path == "sub"
        )),
        "dirty submodule must be a plan-time blocker: {:?}",
        plan_with_sub.blockers
    );

    // Discard-all of the OTHER files (submodule excluded) completes.
    let paths = vec!["tracked.txt".to_string()];
    let plan = plan_discard(&repo, &paths).expect("plan");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    execute_discard(&repo, &plan, &paths).expect("execute");
    assert_eq!(read_file(&d, "tracked.txt"), "committed\n");

    // The submodule change is untouched (still dirty), never mangled.
    let status = working_tree_status(&repo).expect("status");
    assert!(
        status.unstaged.iter().any(|f| f.path == Path::new("sub")),
        "submodule must remain dirty (not discarded, not aborted): {:?}",
        status.unstaged
    );
}
