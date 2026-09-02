//! #309 — stash-apply/pop conflict → Conflict Mode (backend, mutation-verified).
//!
//! A conflicted `git stash pop` leaves `repo.state() == Clean` (no MERGE_HEAD /
//! sequencer state) with unmerged entries only in the index. These tests cover
//! the three backend paths added for #309:
//!
//! 1. **detection** — `detect_conflict_session` returns `ConflictOp::StashConflict`.
//! 2. **complete**  — `execute_conflict_continue` stages the resolution (stage 0),
//!    clears conflicts, and creates NO commit (HEAD unchanged).
//! 3. **abort**     — `execute_stash_conflict_abort` restores HEAD for the
//!    conflicted paths, clears conflicts, and leaves the stash intact.
//!
//! Each is written to FAIL if its fix is reverted (see the per-test notes).

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    detect_conflict_session, execute_conflict_continue, execute_stash_conflict_abort, ConflictKind,
    ConflictOp, ContinueOutcome, ResolutionBuffer, ResolutionChoice,
};

// ────────────────────────────────────────────────────────────
// Git CLI helpers
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

/// Run a git command allowed to fail (`stash pop` that conflicts exits 1).
fn git_allow_fail(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
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
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

/// Build a repo where `git stash pop` conflicts with a subsequent HEAD change:
///
/// - `file.txt` = "base"          (committed)
/// - modify to "STASH change", `git stash`  → working tree clean, stash@{0}
/// - commit "HEAD change" on the same line   → HEAD diverges from the stash base
/// - `git stash pop`                          → conflict, repo stays Clean, stash kept
///
/// Returns the TempDir left in the conflicted-pop state.
fn stash_conflict_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(dir, "file.txt", "line one\nbase\nline three\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    // Local modification, stashed away (tracked change only).
    write_file(dir, "file.txt", "line one\nSTASH change\nline three\n");
    git(dir, &["stash", "push", "-q", "-m", "wip"]);

    // HEAD now changes the same line differently.
    write_file(dir, "file.txt", "line one\nHEAD change\nline three\n");
    git(dir, &["commit", "-qam", "head change"]);

    // Pop the stash → conflict; git keeps the stash entry, state stays Clean.
    git_allow_fail(dir, &["stash", "pop"]);

    tmp
}

// ────────────────────────────────────────────────────────────
// 1. Detection
// ────────────────────────────────────────────────────────────

/// Reverting the `classify_op` fallthrough to `_ => None` makes
/// `detect_conflict_session` return `None` here → this `.expect` panics.
#[test]
fn detects_stash_conflict_session() {
    let tmp = stash_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();

    // Premise: a conflicted stash pop leaves the repo Clean (no MERGE_HEAD).
    assert_eq!(
        repo.state(),
        git2::RepositoryState::Clean,
        "a conflicted stash pop must leave repo state Clean"
    );
    assert!(
        repo.index().unwrap().has_conflicts(),
        "the index must carry unmerged entries"
    );

    let session =
        detect_conflict_session(&repo).expect("expected a StashConflict session (state=Clean)");
    assert_eq!(
        session.op,
        ConflictOp::StashConflict,
        "clean-state + unmerged index must classify as StashConflict"
    );
    assert_eq!(session.op.slug(), "stash");
    assert!(!session.op.is_sequencer(), "stash conflict has no skip");
    assert_eq!(session.total_count(), 1);
    assert_eq!(session.files[0].path.to_string_lossy(), "file.txt");
    assert_eq!(session.files[0].kind, ConflictKind::Content);
}

// ────────────────────────────────────────────────────────────
// 2. Complete (continue = stage only, no commit)
// ────────────────────────────────────────────────────────────

/// Reverting the `StashConflict` early-return in `execute_conflict_continue`
/// makes it fall through to the sequencer loop and shell out
/// `git stash --continue` (not a command) → the call returns `Err` and this
/// `.expect` panics. With the fix: staged at stage 0, conflicts cleared, HEAD
/// unchanged, no commit.
#[test]
fn continue_stages_resolution_without_committing() {
    let tmp = stash_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    let head_before = git_output(dir, &["rev-parse", "HEAD"]);

    // Pick the stashed side as the resolution.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();

    let result =
        execute_conflict_continue(&repo, dir, &session, &buffer).expect("stash-conflict continue");

    // No commit was created — this is a stash apply, not a merge.
    assert!(
        matches!(result.outcome, ContinueOutcome::Staged),
        "stash-conflict continue must be Staged (no commit), got {:?}",
        result.outcome
    );
    let head_after = git_output(dir, &["rev-parse", "HEAD"]);
    assert_eq!(head_before, head_after, "HEAD must not move (no commit)");

    // Index is now conflict-free / commit-able, and the path is staged at
    // stage 0 (no remaining conflict session).
    let repo2 = Repository::open(dir).unwrap();
    assert!(
        !repo2.index().unwrap().has_conflicts(),
        "conflicts must be cleared after continue"
    );
    assert!(
        detect_conflict_session(&repo2).is_none(),
        "no conflict session should remain after continue"
    );
    // The stashed content is what got staged (`git diff --cached` shows it).
    let staged = git_output(dir, &["show", ":file.txt"]);
    assert!(
        staged.contains("STASH change"),
        "the chosen (stashed) side should be staged, got: {staged}"
    );
}

// ────────────────────────────────────────────────────────────
// 3. Abort (restore HEAD for conflicted paths; stash kept)
// ────────────────────────────────────────────────────────────

/// `execute_stash_conflict_abort` must restore the pre-apply (== HEAD) content
/// for the conflicted paths, clear conflicts, and leave the stash intact. If the
/// abort body did nothing, the file would still hold markers and the conflict
/// session would persist → the content / conflict assertions fail.
#[test]
fn abort_restores_head_and_keeps_stash() {
    let tmp = stash_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let stash_count_before = git_output(dir, &["stash", "list"]).lines().count();
    assert_eq!(stash_count_before, 1, "the conflicting stash must be kept");

    // A partial resolution to prove the buffer is preserved, not required.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();

    let outcome =
        execute_stash_conflict_abort(&repo, &session, &buffer).expect("stash-conflict abort");

    // Restored to HEAD (no ref move — HEAD is unchanged).
    assert_eq!(outcome.restored_to.as_deref(), Some(head.as_str()));
    assert_eq!(
        git_output(dir, &["rev-parse", "HEAD"]),
        head,
        "HEAD unmoved"
    );

    // Conflicted path restored to HEAD content, no markers.
    let restored = std::fs::read_to_string(dir.join("file.txt")).unwrap();
    assert!(
        restored.contains("HEAD change") && !restored.contains("<<<<<<<"),
        "file.txt should be back to HEAD content with no markers, got:\n{restored}"
    );

    // Conflicts cleared / no session remaining.
    let repo2 = Repository::open(dir).unwrap();
    assert!(
        !repo2.index().unwrap().has_conflicts(),
        "conflicts must be cleared after abort"
    );
    assert!(detect_conflict_session(&repo2).is_none());

    // The stash entry is left intact.
    let stash_count_after = git_output(dir, &["stash", "list"]).lines().count();
    assert_eq!(
        stash_count_after, stash_count_before,
        "abort must leave the stash entry intact"
    );
}
