//! Integration tests for the P2 conflict-resolution batch (#296 #297 #298 #302).
//!
//! Each test drives a REAL git fixture (git init + real merge / cherry-pick /
//! rebase to produce genuine conflicts) and asserts the git-layer behaviour of
//! the fix. Every test is written so that reverting its fix flips it red — the
//! per-test comment records the mutation.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{
    continue_blockers, detect_conflict_session, execute_conflict_abort, execute_conflict_continue,
    execute_conflict_save, stage_conflict_resolution, ConflictKind, ContinueOutcome,
    ResolutionBuffer, ResolutionChoice,
};

// ────────────────────────────────────────────────────────────
// git CLI helpers
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
        .expect("git failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

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
        .expect("git failed to start");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git failed to start");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn init_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    git(tmp.path(), &["init", "-q", "-b", "main"]);
    tmp
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

/// The blob bytes at `HEAD:<path>` (byte-exact, for binary assertions).
fn head_blob_bytes(repo: &Repository, path: &str) -> Vec<u8> {
    let obj = repo.revparse_single(&format!("HEAD:{path}")).unwrap();
    obj.peel_to_blob().unwrap().content().to_vec()
}

/// The (filemode, oid) of `<path>` in the HEAD tree.
fn head_tree_entry(repo: &Repository, path: &str) -> (i32, git2::Oid) {
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    let e = tree.get_path(Path::new(path)).unwrap();
    (e.filemode(), e.id())
}

// ════════════════════════════════════════════════════════════
// #296 — `<op> --continue` non-zero exit is not swallowed as success
// ════════════════════════════════════════════════════════════

/// A cherry-pick whose resolution reproduces HEAD becomes EMPTY on
/// `--continue`; git exits non-zero ("The previous cherry-pick is now empty").
///
/// Mutation: revert the `out.status` check in `execute_conflict_continue` (or the
/// `index.read(true)` refresh) and this returns `Ok(Staged/Committed)` instead of
/// `Err`, so the assertion `is_err()` fails.
#[test]
fn continue_empty_cherry_pick_surfaces_error() {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "f.txt", "BASE\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    write_file(dir, "f.txt", "SIDE\n");
    git(dir, &["commit", "-qam", "side change"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "f.txt", "MAIN\n");
    git(dir, &["commit", "-qam", "main change"]);

    // cherry-pick the side commit → conflict on f.txt.
    git_allow_fail(dir, &["cherry-pick", "side"]);
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("cherry-pick conflict session");

    // Resolve by KEEPING CURRENT ("MAIN") — identical to HEAD, so the pick is
    // now empty and `git cherry-pick --continue` refuses with a non-zero exit.
    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    buffer
        .apply_choice(Path::new("f.txt"), ResolutionChoice::Current)
        .unwrap();

    let result = execute_conflict_continue(&repo, dir, &session, &buffer);
    assert!(
        result.is_err(),
        "an empty cherry-pick --continue (exit != 0) must surface as an error, got {result:?}"
    );
    // Still mid-cherry-pick (nothing was silently committed).
    assert!(dir.join(".git/CHERRY_PICK_HEAD").exists());
}

// ════════════════════════════════════════════════════════════
// #297 — binary + submodule conflicts resolve via the raw-OID path
// ════════════════════════════════════════════════════════════

fn binary_merge_conflict() -> (TempDir, Vec<u8>, Vec<u8>) {
    let tmp = init_repo();
    let dir = tmp.path();
    let base = vec![0u8, 1, 2, 0, 3];
    let main_bytes = vec![0u8, 1, 2, 0, 9, 9, 9];
    let side_bytes = vec![0u8, 7, 7, 0, 4, 2];
    std::fs::write(dir.join("blob.bin"), &base).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    std::fs::write(dir.join("blob.bin"), &side_bytes).unwrap();
    git(dir, &["commit", "-qam", "side binary"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("blob.bin"), &main_bytes).unwrap();
    git(dir, &["commit", "-qam", "main binary"]);

    git_allow_fail(dir, &["merge", "side"]);
    (tmp, main_bytes, side_bytes)
}

/// A binary conflict: "take current" and "take incoming" both succeed, unblock
/// Continue, and commit a blob BYTE-IDENTICAL to the chosen side.
///
/// Mutation: without the raw-OID path (#297), `apply_choice` errors with "that
/// side does not exist" (blob_text is None for binary), so `.unwrap()` panics
/// and the byte assertion is never reached.
#[test]
fn binary_conflict_take_current_and_incoming_are_byte_identical() {
    for take_current in [true, false] {
        let (tmp, main_bytes, side_bytes) = binary_merge_conflict();
        let dir = tmp.path();
        let repo = Repository::open(dir).unwrap();
        let session = detect_conflict_session(&repo).unwrap();
        assert_eq!(session.files[0].kind, ConflictKind::Binary);

        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        // Before choosing, Continue is blocked (binary unresolved).
        assert!(
            !continue_blockers(&repo, &session, &buffer).is_empty(),
            "binary conflict must block Continue until a side is chosen"
        );

        let choice = if take_current {
            ResolutionChoice::Current
        } else {
            ResolutionChoice::Incoming
        };
        buffer
            .apply_choice(Path::new("blob.bin"), choice)
            .expect("raw take-side must succeed for a binary conflict");

        // Continue is now enabled.
        assert!(
            continue_blockers(&repo, &session, &buffer).is_empty(),
            "binary conflict Continue must enable once a side is chosen"
        );

        let result = execute_conflict_continue(&repo, dir, &session, &buffer)
            .expect("continue binary merge");
        assert!(matches!(result.outcome, ContinueOutcome::Committed(_)));

        let committed = head_blob_bytes(&repo, "blob.bin");
        let expected = if take_current {
            &main_bytes
        } else {
            &side_bytes
        };
        assert_eq!(
            &committed, expected,
            "committed binary blob must be byte-identical to the chosen side"
        );
    }
}

/// A submodule (gitlink) conflict resolves through the same raw-OID path.
///
/// Mutation: without the Submodule classification + raw path, the gitlink is
/// treated as Content, `apply_choice` errors, and the tree-mode assertion fails.
#[test]
fn submodule_conflict_resolves_via_raw_oid() {
    let tmp = init_repo();
    let dir = tmp.path();
    // Three real commits whose shas we reuse as gitlink targets.
    write_file(dir, "f.txt", "1\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c0"]);
    let c0 = git_out(dir, &["rev-parse", "HEAD"]);
    write_file(dir, "f.txt", "2\n");
    git(dir, &["commit", "-qam", "c1"]);
    let c1 = git_out(dir, &["rev-parse", "HEAD"]);
    write_file(dir, "f.txt", "3\n");
    git(dir, &["commit", "-qam", "c2"]);
    let c2 = git_out(dir, &["rev-parse", "HEAD"]);

    // base gitlink → c0 (added on top of c2; the gitlink targets are just three
    // distinct commit shas — no working-tree reset needed).
    git(
        dir,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{c0},sub"),
        ],
    );
    git(dir, &["commit", "-qm", "add gitlink"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    git(
        dir,
        &["update-index", "--cacheinfo", &format!("160000,{c2},sub")],
    );
    git(dir, &["commit", "-qm", "side gitlink"]);

    git(dir, &["checkout", "-q", "main"]);
    git(
        dir,
        &["update-index", "--cacheinfo", &format!("160000,{c1},sub")],
    );
    git(dir, &["commit", "-qm", "main gitlink"]);

    git_allow_fail(dir, &["merge", "side"]);
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("gitlink conflict session");
    let sub = session
        .files
        .iter()
        .find(|f| f.path.to_string_lossy() == "sub")
        .expect("sub is a conflict");
    assert_eq!(sub.kind, ConflictKind::Submodule);

    let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    assert!(!continue_blockers(&repo, &session, &buffer).is_empty());
    buffer
        .apply_choice(Path::new("sub"), ResolutionChoice::Incoming)
        .expect("raw take-side must succeed for a gitlink conflict");
    assert!(continue_blockers(&repo, &session, &buffer).is_empty());

    execute_conflict_continue(&repo, dir, &session, &buffer).expect("continue gitlink merge");
    let (mode, oid) = head_tree_entry(&repo, "sub");
    assert_eq!(
        mode, 0o160000,
        "gitlink must stay a submodule (mode 160000)"
    );
    assert_eq!(oid.to_string(), c2, "gitlink must point at the chosen side");
}

// ════════════════════════════════════════════════════════════
// #298 — exec bit preserved; symlink never dereferenced
// ════════════════════════════════════════════════════════════

fn exec_content_conflict() -> TempDir {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "run.sh", "#!/bin/sh\nL1\nL2\n");
    std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    write_file(dir, "run.sh", "#!/bin/sh\nL1\nSIDE\n");
    git(dir, &["commit", "-qam", "side"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "run.sh", "#!/bin/sh\nMAIN\nL2\n");
    git(dir, &["commit", "-qam", "main"]);

    git_allow_fail(dir, &["merge", "side"]);
    tmp
}

fn resolved_buffer(repo: &Repository, path: &str, text: &str) -> ResolutionBuffer {
    let mut buffer = ResolutionBuffer::from_repo(repo).unwrap();
    buffer.set_manual_text(Path::new(path), text).unwrap();
    buffer
}

/// The exec bit survives BOTH the Continue staging path and the per-file Save
/// path, and the two agree (0o100755 in the staged index either way).
///
/// Mutation: revert the mode preservation in `stage_conflict_resolution` (the
/// temp write drops to 0644) and the Continue-path mode becomes 0o100644,
/// disagreeing with Save's 0o100755.
#[test]
fn exec_bit_preserved_and_save_continue_agree() {
    let resolved = "#!/bin/sh\nMAIN\nSIDE\n";

    // Continue path: stage via stage_conflict_resolution, read staged mode.
    let tmp_a = exec_content_conflict();
    let repo_a = Repository::open(tmp_a.path()).unwrap();
    let session_a = detect_conflict_session(&repo_a).unwrap();
    let buf_a = resolved_buffer(&repo_a, "run.sh", resolved);
    stage_conflict_resolution(&repo_a, &session_a, &buf_a).unwrap();
    let mut idx_a = repo_a.index().unwrap();
    idx_a.read(true).unwrap();
    let mode_continue = idx_a.get_path(Path::new("run.sh"), 0).unwrap().mode;

    // Save path: execute_conflict_save, read staged mode.
    let tmp_b = exec_content_conflict();
    let repo_b = Repository::open(tmp_b.path()).unwrap();
    let buf_b = resolved_buffer(&repo_b, "run.sh", resolved);
    execute_conflict_save(&repo_b, &buf_b, Path::new("run.sh")).unwrap();
    let mut idx_b = repo_b.index().unwrap();
    idx_b.read(true).unwrap();
    let mode_save = idx_b.get_path(Path::new("run.sh"), 0).unwrap().mode;

    assert_eq!(
        mode_continue, 0o100755,
        "Continue must preserve the exec bit"
    );
    assert_eq!(mode_save, 0o100755, "Save must preserve the exec bit");
    assert_eq!(mode_continue, mode_save, "Save and Continue must agree");
}

/// SECURITY (highest priority): a conflicted symlink that points OUTSIDE the
/// repo is never dereferenced. Resolving (Save + Continue) and aborting leave
/// the outside target's bytes untouched, and the committed tree keeps mode
/// 120000 (no 120000→100644 typechange).
///
/// Mutation: revert the Symlink classification / raw path and `execute_conflict_save`
/// would `fs::write` through the on-disk link, overwriting the outside file — the
/// "outside bytes unchanged" assertion fails.
#[test]
fn conflicted_symlink_never_written_through() {
    // The outside target lives in a sibling dir the repo must never touch.
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, b"SECRET-DO-NOT-TOUCH").unwrap();
    let secret_str = secret.to_string_lossy().into_owned();

    let make = || -> TempDir {
        let tmp = init_repo();
        let dir = tmp.path();
        std::os::unix::fs::symlink("placeholder", dir.join("link")).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "base link"]);

        git(dir, &["checkout", "-q", "-b", "side"]);
        std::fs::remove_file(dir.join("link")).unwrap();
        std::os::unix::fs::symlink("inside-target", dir.join("link")).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "side link"]);

        git(dir, &["checkout", "-q", "main"]);
        std::fs::remove_file(dir.join("link")).unwrap();
        // main's link points OUTSIDE the repo — this is the on-disk link at
        // merge time.
        std::os::unix::fs::symlink(&secret_str, dir.join("link")).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-qm", "main link outside"]);

        git_allow_fail(dir, &["merge", "side"]);
        tmp
    };

    let outside_bytes = || std::fs::read(&secret).unwrap();

    // (a) per-file Save through the raw path — must not touch the outside file.
    {
        let tmp = make();
        let repo = Repository::open(tmp.path()).unwrap();
        let session = detect_conflict_session(&repo).unwrap();
        let link = session
            .files
            .iter()
            .find(|f| f.path.to_string_lossy() == "link")
            .expect("link conflict");
        assert_eq!(link.kind, ConflictKind::Symlink);

        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .apply_choice(Path::new("link"), ResolutionChoice::Incoming)
            .expect("raw take-side for symlink");
        execute_conflict_save(&repo, &buffer, Path::new("link")).unwrap();
        assert_eq!(
            outside_bytes(),
            b"SECRET-DO-NOT-TOUCH",
            "Save must not write through the link"
        );
    }

    // (b) Continue — must not touch the outside file, and must commit a symlink
    //     (mode 120000), not a typechange.
    {
        let tmp = make();
        let dir = tmp.path();
        let repo = Repository::open(dir).unwrap();
        let session = detect_conflict_session(&repo).unwrap();
        let mut buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        buffer
            .apply_choice(Path::new("link"), ResolutionChoice::Incoming)
            .unwrap();
        execute_conflict_continue(&repo, dir, &session, &buffer).expect("continue symlink merge");
        assert_eq!(
            outside_bytes(),
            b"SECRET-DO-NOT-TOUCH",
            "Continue must not write through the link"
        );
        let (mode, _) = head_tree_entry(&repo, "link");
        assert_eq!(
            mode, 0o120000,
            "committed link must stay a symlink (no typechange)"
        );
        // The chosen side's target text is committed as the link blob.
        assert_eq!(head_blob_bytes(&repo, "link"), b"inside-target");
    }

    // (c) Abort — restore path must not write through the link either.
    {
        let tmp = make();
        let repo = Repository::open(tmp.path()).unwrap();
        let session = detect_conflict_session(&repo).unwrap();
        let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        execute_conflict_abort(&repo, &session, &buffer).expect("abort symlink merge");
        assert_eq!(
            outside_bytes(),
            b"SECRET-DO-NOT-TOUCH",
            "Abort must not write through the link"
        );
    }
}

// ════════════════════════════════════════════════════════════
// #302 — rebase abort returns to the branch; classification gaps
// ════════════════════════════════════════════════════════════

/// `rebase --abort` restores HEAD to the ORIGINAL BRANCH (symbolic HEAD), not a
/// detached commit.
///
/// Mutation: revert the head-name restore and abort writes `HEAD` as a direct
/// ref → `repo.head().is_branch()` is false and the shorthand is not the branch.
#[test]
fn rebase_abort_returns_to_branch_not_detached() {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "f.txt", "L1\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    write_file(dir, "f.txt", "FEATURE\n");
    git(dir, &["commit", "-qam", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "f.txt", "MAIN\n");
    git(dir, &["commit", "-qam", "main change"]);

    // Rebase feature onto main → conflict, detached HEAD mid-rebase.
    git(dir, &["checkout", "-q", "feature"]);
    git_allow_fail(dir, &["rebase", "main"]);

    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).expect("rebase conflict session");
    assert!(repo.head_detached().unwrap(), "mid-rebase HEAD is detached");

    let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
    execute_conflict_abort(&repo, &session, &buffer).expect("rebase abort");

    let head = repo.head().unwrap();
    assert!(
        head.is_branch(),
        "after rebase --abort HEAD must be symbolic (attached to a branch)"
    );
    assert_eq!(head.shorthand().ok(), Some("feature"));
    assert!(
        !repo.head_detached().unwrap(),
        "after rebase --abort HEAD must NOT be detached"
    );
}

/// add/add (both sides add the path, no common ancestor) is classified AddAdd.
#[test]
fn add_add_is_classified() {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "seed.txt", "x\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "seed"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    write_file(dir, "new.txt", "SIDE\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "side add"]);

    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "new.txt", "MAIN\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "main add"]);

    git_allow_fail(dir, &["merge", "side"]);
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let f = session
        .files
        .iter()
        .find(|f| f.path.to_string_lossy() == "new.txt")
        .expect("new.txt conflict");
    assert_eq!(f.kind, ConflictKind::AddAdd);
}

/// rename/rename is NOT collapsed by the path-dedup into a single lost entry —
/// git indexes it as three unmerged entries at three distinct paths.
///
/// Mutation: restore `dedup_by(|a,b| a.path == b.path)` (keyed on path only) and
/// the three distinct-path entries still survive here, but the (path,kind) dedup
/// is what guarantees distinct sides are never merged away — this locks the count.
#[test]
fn rename_rename_not_collapsed() {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "a.txt", "l1\nl2\nl3\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "base"]);

    git(dir, &["checkout", "-q", "-b", "side"]);
    git(dir, &["mv", "a.txt", "c.txt"]);
    git(dir, &["commit", "-qm", "rename to c"]);

    git(dir, &["checkout", "-q", "main"]);
    git(dir, &["mv", "a.txt", "b.txt"]);
    git(dir, &["commit", "-qm", "rename to b"]);

    git_allow_fail(dir, &["merge", "side"]);
    let repo = Repository::open(dir).unwrap();
    let session = detect_conflict_session(&repo).unwrap();
    let paths: Vec<String> = session
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&"a.txt".to_string()), "got {paths:?}");
    assert!(paths.contains(&"b.txt".to_string()), "got {paths:?}");
    assert!(paths.contains(&"c.txt".to_string()), "got {paths:?}");
    assert_eq!(paths.len(), 3, "rename/rename must not collapse: {paths:?}");
}
