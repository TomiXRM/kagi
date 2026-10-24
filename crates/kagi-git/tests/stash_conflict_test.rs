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

#[path = "../../../tests/support/backend_ops.rs"]
mod backend_ops;
use backend_ops::{execute_conflict_continue, execute_stash_conflict_abort};
use std::path::Path;

#[path = "../../../tests/support/git_fixture.rs"]
mod git_fixture;
use git_fixture::{git, git_output, git_succeeds, init_repo, write_file};

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    detect_conflict_session, ConflictKind, ConflictOp, ContinueOutcome, ResolutionBuffer,
    ResolutionChoice,
};

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
    init_repo(dir, "main");

    write_file(dir, "file.txt", "line one\nbase\nline three\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    // Local modification, stashed away (tracked change only).
    write_file(dir, "file.txt", "line one\nSTASH change\nline three\n");
    git(dir, &["stash", "push", "-q", "-m", "wip"]);

    // HEAD now changes the same line differently.
    write_file(dir, "file.txt", "line one\nHEAD change\nline three\n");
    git(dir, &["commit", "-qam", "head change"]);

    // Pop the stash → conflict (exit 1); git keeps the stash entry, state stays Clean.
    git_succeeds(dir, &["stash", "pop"]);

    tmp
}

// ────────────────────────────────────────────────────────────
// 1. Detection
// ────────────────────────────────────────────────────────────

/// Reverting the `classify_op` fallthrough to `_ => None` makes
/// `detect_conflict_session` return `None` here → this `.expect` panics.
#[test]
fn detects_stash_conflict_session() {
    if !crate::test_support::run_isolated() {
        return;
    }
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
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = stash_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let expected = kagi_git::Backend::open(dir)
        .unwrap()
        .conflict_snapshot()
        .unwrap()
        .unwrap()
        .observation
        .revision;

    let head_before = git_output(dir, &["rev-parse", "HEAD"]);

    // Pick the stashed side as the resolution.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();

    let result = execute_conflict_continue(&repo, dir, &session, &buffer, &expected)
        .expect("stash-conflict continue");

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
    if !crate::test_support::run_isolated() {
        return;
    }
    let tmp = stash_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    let head = git_output(dir, &["rev-parse", "HEAD"]);
    let stale = git_output(dir, &["rev-parse", "HEAD^"]);
    assert_ne!(stale, head);
    git(dir, &["update-ref", "ORIG_HEAD", &stale]);
    let plan = kagi_git::plan_conflict_abort(&repo, &session).unwrap();
    assert_eq!(plan.predicted.head, format!("restored to {}", &head[..8]));
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

/// #1745: dropping non-UTF-8 paths from checkout used to remove their index
/// entries while leaving conflict markers in the worktree.
#[cfg(unix)]
#[test]
fn stash_abort_restores_non_utf8_index_and_worktree_bytes() {
    use std::os::unix::ffi::OsStrExt;

    // Probe before isolated dispatch: successful child stderr is swallowed by
    // the harness, so an unsupported filesystem must be reported by the parent.
    let path = Path::new(std::ffi::OsStr::from_bytes(b"conflict-\xff.txt"));
    let probe = TempDir::new().unwrap();
    if let Err(error) = std::fs::write(probe.path().join(path), b"probe\n") {
        if error.raw_os_error() == Some(libc::EILSEQ) {
            eprintln!(
                "skip: filesystem rejects non-UTF-8 filenames (EILSEQ); stash abort regression requires a supporting filesystem"
            );
            return;
        }
        panic!("cannot probe non-UTF-8 filename support: {error}");
    }
    drop(probe);
    if !crate::test_support::run_isolated() {
        return;
    }
    assert_stash_abort_restores_bytes(path);
}

#[cfg(unix)]
#[test]
fn stash_abort_restores_unicode_index_and_worktree_bytes() {
    if !crate::test_support::run_isolated() {
        return;
    }
    assert_stash_abort_restores_bytes(Path::new("conflict-雪.txt"));
}

#[cfg(unix)]
fn assert_stash_abort_restores_bytes(path: &Path) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir, "main");
    if let Err(error) = std::fs::write(dir.join(path), b"base\n") {
        if path.to_str().is_none() && error.raw_os_error() == Some(libc::EILSEQ) {
            eprintln!(
                "skip: filesystem rejects non-UTF-8 filenames (EILSEQ); stash abort regression requires a supporting filesystem"
            );
            return;
        }
        panic!("cannot create non-UTF-8 stash fixture: {error}");
    }
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);
    std::fs::write(dir.join(path), b"stash\n").unwrap();
    git(dir, &["stash", "push", "-q"]);
    std::fs::write(dir.join(path), b"before pop\n").unwrap();
    git(dir, &["commit", "-qam", "head change"]);

    let repo = Repository::open(dir).unwrap();
    let before_entry = repo.index().unwrap().get_path(path, 0).unwrap();
    let before_index_bytes = repo.find_blob(before_entry.id).unwrap().content().to_vec();
    let before_worktree_bytes = std::fs::read(dir.join(path)).unwrap();
    let before_head = git_output(dir, &["rev-parse", "HEAD"]);
    let before_stash = git_output(dir, &["rev-parse", "refs/stash"]);
    assert!(!git_succeeds(dir, &["stash", "pop"]));

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.op, ConflictOp::StashConflict);
    assert_eq!(session.files[0].path, path);
    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    execute_stash_conflict_abort(&repo, &session, &buffer).unwrap();

    let index = repo.index().unwrap();
    assert!(!index.has_conflicts());
    let restored = index
        .get_path(path, 0)
        .expect("raw path restored at stage 0");
    assert_eq!(
        (restored.id, restored.mode),
        (before_entry.id, before_entry.mode)
    );
    assert_eq!(
        repo.find_blob(restored.id).unwrap().content(),
        before_index_bytes
    );
    assert_eq!(
        std::fs::read(dir.join(path)).unwrap(),
        before_worktree_bytes
    );
    assert_eq!(git_output(dir, &["rev-parse", "HEAD"]), before_head);
    assert_eq!(git_output(dir, &["rev-parse", "refs/stash"]), before_stash);
}

#[path = "../../../tests/support/isolated.rs"]
mod test_support;
