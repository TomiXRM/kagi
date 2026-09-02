//! Integration tests for path-handling correctness in the git layer
//! (issues #292 and #293).
//!
//! #292 — diff pathspecs were interpreted as globs and a no-match fell back to
//!         delta 0 (a *different* file's content).
//! #293 — non-UTF-8 file names collapsed to an empty `PathBuf` and vanished
//!         from `working_tree_status` (and thus every overwrite guard).
//!
//! All writes are confined to `TempDir` repositories.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{unstaged_file_diff, DiffLineKind};

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

fn init_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let dir = tmp.path().to_path_buf();
    git(&dir, &["init", "-b", "main", "."]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    let repo = Repository::open(&dir).expect("open repo");
    (dir, repo)
}

/// Whether any Added line in the diff contains `needle`.
fn added_contains(diff: &kagi_git::FileDiff, needle: &str) -> bool {
    diff.hunks.iter().any(|h| {
        h.lines
            .iter()
            .any(|l| matches!(l.kind, DiffLineKind::Added) && l.content.contains(needle))
    })
}

// ────────────────────────────────────────────────────────────
// #292 — glob metacharacters in a name must be matched literally
// ────────────────────────────────────────────────────────────

/// Faithful #292 repro: `a[b].txt` is a glob that matches `ab.txt`. When the
/// glob file itself has NO working-tree delta (staged-only) but its neighbour
/// `ab.txt` is changed, requesting `a[b].txt`'s diff must NOT show `ab.txt`'s
/// content. The original bug (`.unwrap_or(0)` + no `disable_pathspec_match`)
/// returned `ab.txt` — a different file's content in the diff pane.
#[test]
fn glob_char_name_does_not_show_neighbor_content() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = init_repo(&tmp);

    std::fs::write(dir.join("ab.txt"), "orig\n").unwrap();
    std::fs::write(dir.join("a[b].txt"), "orig\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-m", "init"]);

    // Only the neighbour changes in the working tree; a[b].txt has no WT delta.
    std::fs::write(dir.join("ab.txt"), "AB-NEIGHBOR-CONTENT\n").unwrap();

    let diff = unstaged_file_diff(&repo, Path::new("a[b].txt")).expect("diff");

    assert!(
        !added_contains(&diff, "AB-NEIGHBOR-CONTENT"),
        "a[b].txt diff must NOT contain ab.txt's content (glob shadow / delta[0] fallback); \
         new_path={:?} hunks={:?}",
        diff.new_path,
        diff.hunks,
    );
    assert!(
        diff.hunks.is_empty(),
        "an unchanged glob-named file should have an empty diff, got hunks={:?}",
        diff.hunks,
    );
}

/// When a glob-named file IS changed it must show its own content. Guards the
/// combined fix so `disable_pathspec_match` + literal delta match still returns
/// the right file rather than over-filtering it out.
#[test]
fn glob_char_name_shows_its_own_content() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = init_repo(&tmp);

    std::fs::write(dir.join("ab.txt"), "orig\n").unwrap();
    std::fs::write(dir.join("a[b].txt"), "orig\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-m", "init"]);

    std::fs::write(dir.join("ab.txt"), "AB-NEIGHBOR-CONTENT\n").unwrap();
    std::fs::write(dir.join("a[b].txt"), "BRACKET-OWN-CONTENT\n").unwrap();

    let diff = unstaged_file_diff(&repo, Path::new("a[b].txt")).expect("diff");

    assert!(
        added_contains(&diff, "BRACKET-OWN-CONTENT"),
        "a[b].txt diff must contain its own content, got new_path={:?} hunks={:?}",
        diff.new_path,
        diff.hunks,
    );
    assert!(
        !added_contains(&diff, "AB-NEIGHBOR-CONTENT"),
        "a[b].txt diff must NOT contain ab.txt's content",
    );
}

/// A directory pathspec prefix-matches its members but equals none of them.
/// The delta-match fallback must return an EMPTY diff for the requested path,
/// never delta 0 (a member file's content). Catches a restored `.unwrap_or(0)`.
#[test]
fn no_exact_delta_match_returns_empty_not_delta_zero() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = init_repo(&tmp);

    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/a.txt"), "a0\n").unwrap();
    std::fs::write(dir.join("sub/b.txt"), "b0\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-m", "init"]);

    std::fs::write(dir.join("sub/a.txt"), "MEMBER-A-CONTENT\n").unwrap();
    std::fs::write(dir.join("sub/b.txt"), "MEMBER-B-CONTENT\n").unwrap();

    // Request the directory itself — no delta has path exactly "sub".
    let diff = unstaged_file_diff(&repo, Path::new("sub")).expect("diff");

    assert!(
        !added_contains(&diff, "MEMBER-A-CONTENT") && !added_contains(&diff, "MEMBER-B-CONTENT"),
        "diff for a path with no exact delta must not return a member file's content (delta[0])",
    );
    assert!(
        diff.hunks.is_empty(),
        "no-match diff should be empty, got hunks={:?}",
        diff.hunks,
    );
}

/// A name starting with `#` is silently dropped by libgit2's fnmatch parser
/// unless the pathspec is matched literally, making a changed file read as
/// "no change". Catches a missing `disable_pathspec_match`.
#[test]
fn hash_prefixed_name_diff_is_not_empty() {
    let tmp = TempDir::new().unwrap();
    let (dir, repo) = init_repo(&tmp);

    std::fs::write(dir.join("#note.md"), "orig\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-m", "init"]);

    std::fs::write(dir.join("#note.md"), "orig\nHASH-CHANGED\n").unwrap();

    let diff = unstaged_file_diff(&repo, Path::new("#note.md")).expect("diff");

    assert!(
        added_contains(&diff, "HASH-CHANGED"),
        "#note.md is changed; its diff must not be empty, got hunks={:?}",
        diff.hunks,
    );
}

// ────────────────────────────────────────────────────────────
// #293 — non-UTF-8 names must be listed byte-faithfully, never dropped
// ────────────────────────────────────────────────────────────

/// A file with non-UTF-8 bytes in its name must appear in `working_tree_status`
/// with its exact bytes — never as an empty PathBuf, and never dropped. Catches
/// a restored `unwrap_or_default()` / lossy `entry_path`.
///
/// Runtime-skips where the filesystem rejects invalid-UTF-8 names: macOS/APFS
/// returns EILSEQ from `std::fs::write`, so the fixture can't be created on the
/// darwin dev host and the test self-skips there; on Linux (CI) it runs. This
/// keeps the body compiled on every unix so it can't silently rot. The fix
/// itself is `#[cfg(unix)]`.
#[cfg(unix)]
#[test]
fn non_utf8_name_appears_in_status_with_exact_bytes() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let tmp = TempDir::new().unwrap();
    let (dir, repo) = init_repo(&tmp);

    // Make an initial commit so HEAD exists.
    std::fs::write(dir.join("README.md"), "hi\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-m", "init"]);

    // b"caf\xe9.txt" is invalid UTF-8 (0xe9 is not a valid sequence start).
    let raw = b"caf\xe9.txt";
    let name = OsStr::from_bytes(raw);
    if std::fs::write(dir.join(name), "content\n").is_err() {
        eprintln!(
            "skip: filesystem rejects non-UTF-8 filenames (e.g. APFS); test needs Linux/ext4"
        );
        return;
    }

    let status = kagi_git::working_tree_status(&repo).expect("status");
    let expected = std::path::PathBuf::from(name);

    // Must show up as untracked with the EXACT bytes, not an empty path.
    assert!(
        status.untracked.iter().any(|p| p == &expected),
        "non-UTF-8 file must be listed untracked with exact bytes; untracked={:?}",
        status.untracked,
    );
    assert!(
        !status.untracked.iter().any(|p| p.as_os_str().is_empty()),
        "no empty PathBuf may appear in status",
    );

    // Stage it → must appear staged with the exact bytes, still no empty path.
    git(&dir, &["add", "-A"]);
    let status2 = kagi_git::working_tree_status(&repo).expect("status2");
    assert!(
        status2.staged.iter().any(|f| f.path == expected),
        "non-UTF-8 file must be listed staged with exact bytes; staged={:?}",
        status2.staged,
    );
    assert!(
        !status2.staged.iter().any(|f| f.path.as_os_str().is_empty())
            && !status2
                .unstaged
                .iter()
                .any(|f| f.path.as_os_str().is_empty()),
        "no empty PathBuf may appear in staged/unstaged status",
    );
}
