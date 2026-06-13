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

/// Process-global serial guard for tests that mutate the `KAGI_LOG_DIR`
/// environment variable (which `ResolutionBuffer` autosave reads).  `std::env`
/// is process-global, so concurrent set/remove across parallel test threads
/// races (the known flaky `abort_restores_pre_op_state_and_retains_buffer`).
/// Every test that touches `KAGI_LOG_DIR` holds this lock for its duration.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use kagi::git::{
    continue_blockers, detect_conflict_session, execute_conflict_abort, execute_conflict_skip,
    plan_conflict_abort, plan_conflict_continue, plan_conflict_skip, ConflictKind, ConflictOp,
    LineOrigin, ResolutionBuffer, ResolutionChoice,
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
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

// ────────────────────────────────────────────────────────────
// W32-CONFLICT-EDITOR: hunk-level model over a real multi-hunk conflict
// ────────────────────────────────────────────────────────────

/// Build a repo whose `file.txt` conflicts in TWO separate places (top and
/// bottom), with an unchanged middle, so the materialization has two hunks
/// separated by passthrough context.
fn two_hunk_conflict_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(
        dir,
        "file.txt",
        "top base\nmid 1\nmid 2\nmid 3\nbottom base\n",
    );
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_file(
        dir,
        "file.txt",
        "top FEATURE\nmid 1\nmid 2\nmid 3\nbottom FEATURE\n",
    );
    git(dir, &["commit", "-qam", "feature edits top+bottom"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(
        dir,
        "file.txt",
        "top MAIN\nmid 1\nmid 2\nmid 3\nbottom MAIN\n",
    );
    git(dir, &["commit", "-qam", "main edits top+bottom"]);

    git_allow_fail(dir, &["merge", "feature"]);
    tmp
}

#[test]
fn hunk_model_splits_real_multi_hunk_conflict_and_assembles_marker_free() {
    use kagi::git::resolution::HunkChoice;

    let tmp = two_hunk_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let path = Path::new("file.txt");

    // Materialize zdiff3 markers and decompose into hunks.
    let markers = buffer
        .materialized_markers(&repo, path)
        .expect("zdiff3 materialization");
    assert!(buffer.ensure_hunks(path, &markers));
    assert_eq!(buffer.hunk_count(path), 2, "two separate conflict hunks");

    // Resolve hunk 0 → current, hunk 1 → incoming.
    assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
    assert!(buffer.apply_hunk_choice(path, 1, HunkChoice::AcceptIncoming));
    assert!(buffer.hunks_all_resolved(path));

    let text = buffer.resolved_text(path).expect("resolved text");
    // Current branch is `main` (we merged feature into main).
    assert!(text.contains("top MAIN"), "hunk 0 accepted current: {:?}", text);
    assert!(text.contains("bottom FEATURE"), "hunk 1 accepted incoming: {:?}", text);
    // Passthrough context preserved.
    assert!(text.contains("mid 2"));
    // Fully resolved → no markers.
    assert!(
        !kagi::git::text_has_conflict_marker(&text),
        "assembled Result must be marker-free: {:?}",
        text
    );

    // Provenance over the assembled lines.
    let prov = buffer.provenance(path).expect("provenance");
    use kagi::git::LineOrigin::*;
    assert!(prov.contains(&Current));
    assert!(prov.contains(&Incoming));
    assert!(prov.contains(&Context));
}

#[test]
fn hunk_reset_keeps_marker_residue_and_blocks_continue() {
    use kagi::git::resolution::HunkChoice;

    let tmp = two_hunk_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let path = Path::new("file.txt");
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    buffer.ensure_hunks(path, &markers);

    buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent);
    buffer.apply_hunk_choice(path, 1, HunkChoice::AcceptIncoming);
    assert!(buffer.files_with_marker_residue().is_empty());

    // Reset hunk 1 → it re-emits markers → residue → continue gate trips.
    assert!(buffer.reset_hunk(path, 1));
    assert!(!buffer.hunks_all_resolved(path));
    assert_eq!(buffer.files_with_marker_residue(), vec![path.to_path_buf()]);

    let session = detect_conflict_session(&repo).unwrap();
    let plan = plan_conflict_continue(&repo, &session, &buffer).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "marker residue from a reset hunk must block continue"
    );
}

// ────────────────────────────────────────────────────────────
// T-043 / 044: strengthened continue gate (structured blockers)
// ────────────────────────────────────────────────────────────

#[test]
fn continue_gate_reports_specific_blocker_codes() {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();

    // Empty buffer → unresolved-files blocker code is present.
    let empty = ResolutionBuffer::new(tmp.path());
    let blockers = continue_blockers(&repo, &session, &empty);
    assert!(
        blockers.iter().any(|b| b.code() == "unresolved-files"),
        "expected unresolved-files code, got {:?}",
        blockers.iter().map(|b| b.code()).collect::<Vec<_>>()
    );

    // Clean resolution → no blockers, Continue allowed.
    let mut clean = ResolutionBuffer::from_repo(&repo).unwrap();
    clean
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Current)
        .unwrap();
    assert!(
        continue_blockers(&repo, &session, &clean).is_empty(),
        "clean resolution should clear every blocker"
    );
}

#[test]
fn continue_gate_flags_unresolved_binary_conflict() {
    let tmp = binary_merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert_eq!(session.files[0].kind, ConflictKind::Binary);

    // Binary file unresolved → both unresolved-files AND binary-unresolved.
    let empty = ResolutionBuffer::new(tmp.path());
    let codes: Vec<&str> = continue_blockers(&repo, &session, &empty)
        .iter()
        .map(|b| b.code())
        .collect();
    assert!(codes.contains(&"binary-unresolved"), "got {:?}", codes);
}

// ────────────────────────────────────────────────────────────
// T-042: sequencer skip (rebase / cherry-pick / revert only)
// ────────────────────────────────────────────────────────────

/// Build a repo mid cherry-pick conflict and return (TempDir, side_sha).
fn cherry_pick_conflict_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_file(dir, "file.txt", "base\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    write_file(dir, "file.txt", "SIDE\n");
    git(dir, &["commit", "-qam", "side change"]);
    let side_sha = git_output(dir, &["rev-parse", "HEAD"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "file.txt", "MAIN\n");
    git(dir, &["commit", "-qam", "main change"]);

    git_allow_fail(dir, &["cherry-pick", &side_sha]);
    tmp
}

/// Build a binary merge conflict repo (both sides change a binary blob).
fn binary_merge_conflict_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    init_repo(dir);

    write_binary(dir, "blob.bin", &[0u8, 1, 2, 3, 0, 4, 5]);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_binary(dir, "blob.bin", &[0u8, 9, 9, 9, 0, 9, 9]);
    git(dir, &["commit", "-qam", "feature blob"]);

    git(dir, &["checkout", "-q", "main"]);
    write_binary(dir, "blob.bin", &[0u8, 7, 7, 7, 0, 7, 7]);
    git(dir, &["commit", "-qam", "main blob"]);

    git_allow_fail(dir, &["merge", "feature"]);
    tmp
}

#[test]
fn skip_is_rejected_for_merge() {
    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert!(matches!(session.op, ConflictOp::Merge { .. }));
    assert!(
        plan_conflict_skip(&repo, &session).is_err(),
        "merge has no skip — plan must error"
    );
}

#[test]
fn skip_cherry_pick_drops_current_step() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let log_tmp = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log_tmp.path());

    let tmp = cherry_pick_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert!(matches!(session.op, ConflictOp::CherryPick { .. }));

    // Plan is always available (no blockers).
    let plan = plan_conflict_skip(&repo, &session).expect("skip plan");
    assert!(plan.blockers.is_empty());

    // Partial resolution so the buffer is preserved.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("file.txt"), ResolutionChoice::Incoming)
        .unwrap();

    let head_before = git_output(dir, &["rev-parse", "HEAD"]);
    let outcome = execute_conflict_skip(&repo, &session, &buffer).expect("skip exec");

    // HEAD unchanged (the conflicting pick was dropped, not committed).
    assert_eq!(outcome.head.as_deref(), Some(head_before.as_str()));

    // No longer mid cherry-pick, working tree restored to HEAD ("MAIN").
    assert!(!dir.join(".git").join("CHERRY_PICK_HEAD").exists());
    let repo2 = Repository::open(dir).unwrap();
    assert!(detect_conflict_session(&repo2).is_none());
    let content = std::fs::read_to_string(dir.join("file.txt")).unwrap();
    assert!(content.contains("MAIN"), "step changes dropped, got {:?}", content);
    assert!(!content.contains("<<<<<<<"), "no markers should remain");

    // Buffer preserved.
    assert!(outcome.buffer_preserved_at.is_some());

    std::env::remove_var("KAGI_LOG_DIR");
}

// ────────────────────────────────────────────────────────────
// ADR-0068: Save / Continue / Commit responsibility split
// (T-CONFLICT-FLOW-030/031/032, T-CONFLICT-UX-013/014)
// ────────────────────────────────────────────────────────────

/// Save resolution writes the working tree and STAGES the file: the index
/// unmerged entries (stage 1/2/3) collapse to stage 0 (T-CONFLICT-UX-014).
#[test]
fn save_resolution_stages_file_to_stage_zero() {
    use kagi::git::execute_conflict_save;

    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let path = Path::new("file.txt");

    // Index has the conflict (unmerged) before Save.
    let index = repo.index().unwrap();
    assert!(index.has_conflicts(), "index should be unmerged before Save");
    drop(index);

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.apply_choice(path, ResolutionChoice::Current).unwrap();

    let outcome = execute_conflict_save(&repo, &buffer, path).expect("save");
    assert_eq!(outcome.path, path.to_path_buf());

    // After Save the index has no conflicts and the path is at stage 0.
    let repo2 = Repository::open(dir).unwrap();
    let index2 = repo2.index().unwrap();
    assert!(!index2.has_conflicts(), "Save must collapse stages → stage 0");
    let entry = index2.get_path(path, 0);
    assert!(entry.is_some(), "path must be present at stage 0 after Save");

    // The working tree holds the resolved (current) text, marker-free.
    let wt = std::fs::read_to_string(dir.join("file.txt")).unwrap();
    assert!(wt.contains("MAIN change"), "working tree has resolved text: {:?}", wt);
    assert!(!kagi::git::text_has_conflict_marker(&wt));
}

/// Save refuses (blocks) when the resolved text still has conflict markers.
#[test]
fn save_resolution_blocks_on_marker_residue() {
    use kagi::git::execute_conflict_save;

    let tmp = merge_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .set_manual_text(path, "<<<<<<< x\nMAIN\n=======\nFEATURE\n>>>>>>> y\n")
        .unwrap();

    assert!(
        execute_conflict_save(&repo, &buffer, path).is_err(),
        "Save must block while conflict markers remain"
    );
    // The index must still be unmerged (nothing staged on a blocked save).
    let index = repo.index().unwrap();
    assert!(index.has_conflicts(), "blocked Save must not stage the file");
}

/// merge Continue does NOT create a commit — it routes to the commit message
/// panel (T-CONFLICT-FLOW-030).  HEAD is unchanged after routing.
#[test]
fn merge_continue_routes_to_commit_panel_without_committing() {
    use kagi::git::{plan_conflict_continue_route, ContinueRoute};

    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let path = Path::new("file.txt");

    let head_before = git_output(dir, &["rev-parse", "HEAD"]);

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.apply_choice(path, ResolutionChoice::Current).unwrap();

    let route = plan_conflict_continue_route(&repo, &session, &buffer, "main")
        .expect("route");
    match route {
        ContinueRoute::MergeCommitPanel { message } => {
            assert!(
                message.to_lowercase().contains("merge"),
                "merge message prefilled: {:?}",
                message
            );
        }
        other => panic!("merge must route to the commit panel, got {:?}", other),
    }

    // No commit was created by routing.
    let head_after = git_output(dir, &["rev-parse", "HEAD"]);
    assert_eq!(head_before, head_after, "routing must NOT create a commit");
    // Still mid-merge (MERGE_HEAD present).
    assert!(dir.join(".git").join("MERGE_HEAD").exists());
}

/// The merge commit, created from the commit-panel button, has TWO parents
/// (HEAD + MERGE_HEAD) and cleans up the merge state (T-CONFLICT-FLOW-031).
#[test]
fn merge_commit_has_two_parents_and_cleans_state() {
    use kagi::git::{execute_conflict_save, execute_merge_commit};

    let tmp = merge_conflict_repo();
    let dir = tmp.path();
    let repo = Repository::open(dir).unwrap();
    let path = Path::new("file.txt");

    // Save the resolution (stages the file).
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.apply_choice(path, ResolutionChoice::Current).unwrap();
    execute_conflict_save(&repo, &buffer, path).expect("save");

    // Create the merge commit with the panel's edited message.
    let repo2 = Repository::open(dir).unwrap();
    let id = execute_merge_commit(&repo2, "Merge feature into main").expect("merge commit");

    // Two parents, custom message, state cleaned.
    let repo3 = Repository::open(dir).unwrap();
    let oid = git2::Oid::from_str(&id.0).unwrap();
    let commit = repo3.find_commit(oid).unwrap();
    assert_eq!(commit.parent_count(), 2, "merge commit must have two parents");
    assert_eq!(commit.message().unwrap().trim(), "Merge feature into main");
    assert!(!dir.join(".git").join("MERGE_HEAD").exists(), "MERGE_HEAD cleaned");
    assert!(detect_conflict_session(&repo3).is_none(), "no longer in conflict");
}

/// A sequencer (cherry-pick) Continue produces a `--continue` OperationPlan,
/// not a merge-commit-panel route (T-CONFLICT-FLOW-032).
#[test]
fn sequencer_continue_produces_a_plan() {
    use kagi::git::{plan_conflict_continue_route, ContinueRoute};

    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let log_tmp = TempDir::new().unwrap();
    std::env::set_var("KAGI_LOG_DIR", log_tmp.path());

    let tmp = cherry_pick_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    assert!(matches!(session.op, ConflictOp::CherryPick { .. }));
    let path = Path::new("file.txt");

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer.apply_choice(path, ResolutionChoice::Incoming).unwrap();

    let route = plan_conflict_continue_route(&repo, &session, &buffer, "main")
        .expect("route");
    match route {
        ContinueRoute::SequencerPlan(plan) => {
            assert!(plan.blockers.is_empty(), "resolved → no blockers");
            assert!(plan.title.contains("cherry-pick"), "plan titled for the op: {}", plan.title);
        }
        other => panic!("sequencer must produce a plan, got {:?}", other),
    }

    std::env::remove_var("KAGI_LOG_DIR");
}

/// Per-hunk accept is independent: each hunk's choice can differ, and changing
/// one hunk does not alter another (T-CONFLICT-UX-010/012).
#[test]
fn per_hunk_accept_is_independent() {
    use kagi::git::resolution::{HunkChoice, Region};

    let tmp = two_hunk_conflict_repo();
    let repo = Repository::open(tmp.path()).unwrap();
    let path = Path::new("file.txt");

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    let markers = buffer.materialized_markers(&repo, path).unwrap();
    buffer.ensure_hunks(path, &markers);
    assert_eq!(buffer.hunk_count(path), 2);

    // Hunk 0 → current, hunk 1 left unresolved.
    assert!(buffer.apply_hunk_choice(path, 0, HunkChoice::AcceptCurrent));
    {
        let model = buffer.hunk_model(path).unwrap();
        let hunks: Vec<_> = model
            .regions
            .iter()
            .filter_map(|r| match r {
                Region::Hunk(h) => Some(h),
                Region::Passthrough(_) => None,
            })
            .collect();
        assert_eq!(hunks[0].choice, HunkChoice::AcceptCurrent);
        assert_eq!(hunks[1].choice, HunkChoice::Unresolved, "hunk 1 untouched");
    }

    // Now hunk 1 → incoming; hunk 0 stays current (independent).
    assert!(buffer.apply_hunk_choice(path, 1, HunkChoice::AcceptIncoming));
    {
        let model = buffer.hunk_model(path).unwrap();
        let hunks: Vec<_> = model
            .regions
            .iter()
            .filter_map(|r| match r {
                Region::Hunk(h) => Some(h),
                Region::Passthrough(_) => None,
            })
            .collect();
        assert_eq!(hunks[0].choice, HunkChoice::AcceptCurrent, "hunk 0 unchanged");
        assert_eq!(hunks[1].choice, HunkChoice::AcceptIncoming);
    }
}
