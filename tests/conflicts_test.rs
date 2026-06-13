//! Integration tests for the conflict-resolution backend (W26-CONFLICT-CORE,
//! T-CONFLICT-001 / 005 / 008 / 010).
//!
//! Real merge / cherry-pick conflicts are produced in `TempDir` repositories via
//! the `git` CLI; no existing user repository is touched.  These tests cover:
//!
//! - session detection (op kind + file kinds incl. modify/delete + binary),
//! - the resolution buffer (choices, undo, provenance),
//! - autosave round-trip (under a redirected `KAGI_LOG_DIR`),
//! - the marker-residue gate blocking continue,
//! - abort restoring the pre-op state with the buffer retained.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    detect_conflict_session, execute_conflict_abort, plan_conflict_abort,
    plan_conflict_continue, ConflictKind, ConflictOp, LineOrigin, ResolutionBuffer,
    ResolutionChoice,
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

/// Run a git command allowed to fail (e.g. `merge` that conflicts exits 1).
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

fn write_binary(dir: &Path, name: &str, content: &[u8]) {
    std::fs::write(dir.join(name), content).expect("write_binary failed");
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

// ────────────────────────────────────────────────────────────
// Fixtures
// ────────────────────────────────────────────────────────────

/// Build a repo with a content merge conflict on `file.txt` and return the
/// TempDir.  After this, `git merge feature` has left the repo in the merge
/// conflict state.
fn merge_conflict_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(dir, "file.txt", "line one\nshared\nline three\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    // feature branch changes the middle line one way.
    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_file(dir, "file.txt", "line one\nFEATURE change\nline three\n");
    git(dir, &["commit", "-qam", "feature change"]);

    // main changes the same line a different way.
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "file.txt", "line one\nMAIN change\nline three\n");
    git(dir, &["commit", "-qam", "main change"]);

    // Merge feature → conflict.
    git_allow_fail(dir, &["merge", "feature"]);

    tmp
}

// ────────────────────────────────────────────────────────────
// T-CONFLICT-001: detection
// ────────────────────────────────────────────────────────────

#[test]
fn detects_merge_session_with_content_conflict() {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();

    let session = detect_conflict_session(&repo).expect("expected a merge conflict session");
    match &session.op {
        ConflictOp::Merge { incoming, .. } => {
            assert!(incoming.is_some(), "MERGE_HEAD sha should be readable");
        }
        other => panic!("expected Merge op, got {:?}", other),
    }
    assert_eq!(session.total_count(), 1);
    assert_eq!(session.unresolved_count(), 1);
    assert_eq!(session.files[0].path.to_string_lossy(), "file.txt");
    assert_eq!(session.files[0].kind, ConflictKind::Content);
}

#[test]
fn no_session_on_clean_repo() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);
    write_file(dir, "a.txt", "hi\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "init"]);

    let repo = Repository::open(dir).unwrap();
    assert!(detect_conflict_session(&repo).is_none());
}

#[test]
fn detects_cherry_pick_session() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(dir, "file.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    // A side branch with a conflicting change to cherry-pick.
    git(dir, &["checkout", "-q", "-b", "side"]);
    write_file(dir, "file.txt", "SIDE\n");
    git(dir, &["commit", "-qam", "side change"]);
    let side_sha = git_output(dir, &["rev-parse", "HEAD"]);

    // main diverges on the same line.
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "file.txt", "MAIN\n");
    git(dir, &["commit", "-qam", "main change"]);

    git_allow_fail(dir, &["cherry-pick", &side_sha]);

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("expected cherry-pick session");
    match &session.op {
        ConflictOp::CherryPick { source, source_summary } => {
            assert!(source.is_some(), "CHERRY_PICK_HEAD sha should be readable");
            assert_eq!(source_summary.as_deref(), Some("side change"));
        }
        other => panic!("expected CherryPick op, got {:?}", other),
    }
    assert_eq!(session.files[0].kind, ConflictKind::Content);
}

#[test]
fn classifies_modify_delete_conflict() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(dir, "doomed.txt", "original\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    // feature deletes the file.
    git(dir, &["checkout", "-q", "-b", "feature"]);
    git(dir, &["rm", "-q", "doomed.txt"]);
    git(dir, &["commit", "-qm", "delete doomed"]);

    // main modifies it.
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "doomed.txt", "MODIFIED\n");
    git(dir, &["commit", "-qam", "modify doomed"]);

    git_allow_fail(dir, &["merge", "feature"]);

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("expected modify/delete session");
    let file = session
        .files
        .iter()
        .find(|f| f.path.to_string_lossy() == "doomed.txt")
        .expect("doomed.txt should be a conflict");
    assert_eq!(file.kind, ConflictKind::ModifyDelete);
}

#[test]
fn classifies_binary_conflict() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    // A blob with NUL bytes → binary.
    write_binary(dir, "img.bin", &[0u8, 1, 2, 3, 0, 9, 8]);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base binary"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_binary(dir, "img.bin", &[0u8, 1, 2, 3, 0, 99, 88]);
    git(dir, &["commit", "-qam", "feature binary"]);

    git(dir, &["checkout", "-q", "main"]);
    write_binary(dir, "img.bin", &[0u8, 1, 2, 3, 0, 77, 66]);
    git(dir, &["commit", "-qam", "main binary"]);

    git_allow_fail(dir, &["merge", "feature"]);

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("expected binary conflict session");
    let file = session
        .files
        .iter()
        .find(|f| f.path.to_string_lossy() == "img.bin")
        .expect("img.bin should be a conflict");
    assert_eq!(file.kind, ConflictKind::Binary);
}

// ────────────────────────────────────────────────────────────
// T-CONFLICT-005: resolution buffer
// ────────────────────────────────────────────────────────────

#[test]
fn buffer_choices_undo_and_provenance() {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();

    let mut buffer = ResolutionBuffer::from_repo(&repo).expect("build buffer");
    let path = Path::new("file.txt");

    // Side texts were materialized from the index stages.
    let (cur, inc) = buffer.sides(path).expect("sides");
    assert!(cur.as_deref().unwrap().contains("MAIN change"));
    assert!(inc.as_deref().unwrap().contains("FEATURE change"));

    // Choose current → resolved text equals the current side, provenance Current.
    buffer.apply_choice(path, ResolutionChoice::Current).unwrap();
    assert!(buffer.has_resolution(path));
    assert!(buffer.resolved_text(path).unwrap().contains("MAIN change"));
    assert!(buffer
        .provenance(path)
        .unwrap()
        .iter()
        .all(|o| *o == LineOrigin::Current));

    // Both, current-first → both changes present in order.
    buffer
        .apply_choice(path, ResolutionChoice::BothCurrentFirst)
        .unwrap();
    let both = buffer.resolved_text(path).unwrap();
    let main_idx = both.find("MAIN change").unwrap();
    let feat_idx = both.find("FEATURE change").unwrap();
    assert!(main_idx < feat_idx, "current side should come first");

    // Undo returns to the Current-only resolution.
    assert!(buffer.undo(path));
    assert!(buffer.resolved_text(path).unwrap().contains("MAIN change"));
    assert!(!buffer.resolved_text(path).unwrap().contains("FEATURE change"));
}

#[test]
fn buffer_autosave_round_trip() {
    // Redirect autosave to a temp dir via KAGI_LOG_DIR.
    let log_tmp = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log_tmp.path());

    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();

    let mut buffer = ResolutionBuffer::from_repo(&repo).expect("build buffer");
    let path = Path::new("file.txt");
    buffer
        .apply_choice(path, ResolutionChoice::BothIncomingFirst)
        .unwrap();

    let saved_at = buffer.autosave().expect("autosave");
    assert!(saved_at.exists(), "autosave file should exist");

    let loaded = ResolutionBuffer::load(tmp.path()).expect("load buffer");
    assert!(loaded.has_resolution(path));
    assert_eq!(
        loaded.resolved_text(path).unwrap(),
        buffer.resolved_text(path).unwrap()
    );
    assert_eq!(loaded.provenance(path).unwrap(), buffer.provenance(path).unwrap());

    // Cleanup the global env var so other test binaries are unaffected.
    ResolutionBuffer::clear(tmp.path()).unwrap();
    std::env::remove_var("KAGI_LOG_DIR");
}

// ────────────────────────────────────────────────────────────
// T-CONFLICT-008: continue gate + abort
// ────────────────────────────────────────────────────────────

#[test]
fn continue_blocked_until_resolved_and_marker_free() {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    // 1. Empty buffer → unresolved blocker.
    let empty = ResolutionBuffer::new(tmp.path());
    let plan = plan_conflict_continue(&repo, &session, &empty).unwrap();
    assert!(
        plan.blockers.iter().any(|b| b.contains("unresolved")),
        "expected unresolved blocker, got {:?}",
        plan.blockers
    );

    // 2. Resolution that still contains conflict markers → marker blocker.
    let mut markered = ResolutionBuffer::from_repo(&repo).unwrap();
    markered
        .set_manual_text(
            Path::new("file.txt"),
            "<<<<<<< HEAD\nMAIN\n=======\nFEATURE\n>>>>>>> feature\n",
        )
        .unwrap();
    let plan = plan_conflict_continue(&repo, &session, &markered).unwrap();
    assert!(
        plan.blockers.iter().any(|b| b.contains("marker")),
        "expected marker residue blocker, got {:?}",
        plan.blockers
    );

    // 3. Clean resolution → no blockers.
    let mut clean = ResolutionBuffer::from_repo(&repo).unwrap();
    clean
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
        .unwrap();
    let plan = plan_conflict_continue(&repo, &session, &clean).unwrap();
    assert!(
        plan.blockers.is_empty(),
        "clean resolution should have no blockers, got {:?}",
        plan.blockers
    );
}

#[test]
fn abort_restores_pre_op_state_and_retains_buffer() {
    let log_tmp = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log_tmp.path());

    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();

    // The pre-merge HEAD ("main change") is recorded in ORIG_HEAD.
    let orig_head = git_output(dir, &["rev-parse", "ORIG_HEAD"]);
    assert!(!orig_head.is_empty());

    let session = detect_conflict_session(&repo).unwrap();

    // Partially resolve so the buffer has content worth preserving.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();

    // Plan is always available (no blockers).
    let plan = plan_conflict_abort(&repo, &session).unwrap();
    assert!(plan.blockers.is_empty());

    // Execute abort.
    let outcome = execute_conflict_abort(&repo, &session, &buffer).expect("abort");

    // HEAD restored to the pre-op commit.
    assert_eq!(outcome.restored_to.as_deref(), Some(orig_head.as_str()));
    let head_now = git_output(dir, &["rev-parse", "HEAD"]);
    assert_eq!(head_now, orig_head, "HEAD should be back at ORIG_HEAD");

    // No longer mid-merge: MERGE_HEAD cleared, no conflict session.
    assert!(!dir.join(".git").join("MERGE_HEAD").exists());
    let repo2 = Repository::open(dir).unwrap();
    assert!(detect_conflict_session(&repo2).is_none());

    // Working tree restored to the pre-op content ("MAIN change").
    let restored = std::fs::read_to_string(dir.join("file.txt")).unwrap();
    assert!(restored.contains("MAIN change"));
    assert!(!restored.contains("<<<<<<<"), "no conflict markers should remain");

    // The buffer was preserved to the autosave dir.
    let preserved = outcome.buffer_preserved_at.expect("buffer preserved path");
    assert!(preserved.exists(), "preserved buffer file should exist");
    let reloaded = ResolutionBuffer::load(dir).expect("reload preserved buffer");
    assert!(reloaded.has_resolution(Path::new("file.txt")));

    ResolutionBuffer::clear(dir).unwrap();
    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn execute_continue_merge_creates_merge_commit() {
    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::BothCurrentFirst)
        .unwrap();

    let outcome = kagi::git::execute_conflict_continue(&repo, &session, &buffer)
        .expect("continue merge");
    match outcome {
        kagi::git::ContinueOutcome::Committed(id) => {
            // The new commit is a merge (two parents).
            let parents = git_output(dir, &["rev-list", "--parents", "-n", "1", &id.0]);
            let count = parents.split_whitespace().count();
            assert_eq!(count, 3, "merge commit should have 2 parents (3 hashes)");
        }
        other => panic!("expected Committed, got {:?}", other),
    }

    // Repo is no longer mid-merge.
    assert!(detect_conflict_session(&repo).is_none());
    let content = std::fs::read_to_string(dir.join("file.txt")).unwrap();
    assert!(content.contains("MAIN change") && content.contains("FEATURE change"));
}
