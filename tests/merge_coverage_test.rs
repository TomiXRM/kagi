//! Issue #304 — merge coverage gaps that survived the #299/#300/#301 guard work.
//!
//! `merge_guards_test.rs` already covers the reject/no-orphan paths (#300),
//! unrelated histories (#299), and the capped conflict-file note (#301);
//! `conflicts_test.rs` covers abort restoring cleanly-merged files (#278).
//! The two paths NOTHING exercised end-to-end were:
//!
//!   1. the SUCCESS fast-forward path of `execute_merge_branch` — that the ref
//!      advances AND the working tree is actually updated on disk (the #279
//!      "ref moved but WT stale" shape, on the merge side).
//!   2. a clean TRUE merge touching 100+ files — that it plans + executes,
//!      keeps `preview_files` bounded (#301), and lands every file on disk.
//!
//! All repos live in `TempDir`s (no network, no writes to real repos).

use std::path::Path;
use std::process::Command;

use kagi_git::ops::MergeKind;
use kagi_git::Backend;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git command failed to start");
    assert!(
        output.status.success(),
        "git {} exited with {:?}\nstderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
}

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("rev-parse");
    assert!(out.status.success(), "git rev-parse {} failed", rev);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn porcelain(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .env("HOME", dir)
        .output()
        .expect("git status");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Lines from `git fsck --full` naming a dangling / unreachable commit — the
/// signature of an orphaned merge commit. Empty means the ODB is clean.
fn dangling_commits(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["fsck", "--full", "--no-progress"])
        .current_dir(dir)
        .env("HOME", dir)
        .output()
        .expect("git fsck");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&out.stderr).lines())
        .filter(|l| (l.contains("dangling") || l.contains("unreachable")) && l.contains("commit"))
        .map(|l| l.to_string())
        .collect()
}

fn init_repo() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path();
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    write_file(dir, "base.txt", "base\n");
    git(dir, &["add", "base.txt"]);
    git(dir, &["commit", "-qm", "base"]);
    tmp
}

// ── #304.1: fast-forward EXECUTE updates the working tree ─────────────────

/// The success FF path of `execute_merge_branch` (not `execute_merge_into_branch`,
/// not the reject path). `feature` is strictly ahead of `main`; merging it into
/// `main` fast-forwards. This closes the #279-shaped "ref advanced but the
/// working tree is stale" gap on the merge side.
#[test]
fn merge_fast_forward_execute_updates_worktree() {
    let tmp = init_repo();
    let dir = tmp.path();

    // feature is ahead: it CHANGES base.txt and ADDS ff_new.txt (+ a nested one).
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "base.txt", "advanced\n");
    write_file(dir, "ff_new.txt", "brand new\n");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    write_file(dir, "sub/nested.txt", "nested new\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "feature advances"]);
    let feature_tip = rev_parse(dir, "feature");

    // Back to main (strictly behind feature).
    git(dir, &["checkout", "-q", "main"]);
    let main_before = rev_parse(dir, "main");
    assert_ne!(main_before, feature_tip);

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan merge");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert_eq!(
        kind,
        MergeKind::FastForward,
        "an ahead branch fast-forwards"
    );

    let merged = backend
        .execute_merge_branch("feature")
        .expect("execute FF merge");

    // The ref advanced to exactly the feature tip (and is the returned oid).
    assert_eq!(merged.0, feature_tip, "FF lands the ref on the target tip");
    assert_eq!(rev_parse(dir, "main"), feature_tip, "main advanced");
    assert_eq!(rev_parse(dir, "HEAD"), feature_tip, "HEAD advanced");
    assert_eq!(
        rev_parse(dir, "feature"),
        feature_tip,
        "the target branch does not move"
    );

    // The working tree was actually updated — the whole point of #279.
    assert_eq!(
        std::fs::read_to_string(dir.join("base.txt")).unwrap(),
        "advanced\n",
        "FF-changed file must have the NEW content on disk"
    );
    assert!(
        dir.join("ff_new.txt").exists(),
        "FF-added file must exist on disk"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("ff_new.txt")).unwrap(),
        "brand new\n"
    );
    assert!(
        dir.join("sub/nested.txt").exists(),
        "FF-added nested file must exist on disk"
    );

    // Nothing left staged / unstaged / untracked.
    assert_eq!(porcelain(dir), "", "worktree must be clean after FF merge");
    assert!(
        dangling_commits(dir).is_empty(),
        "FF leaves no dangling objects: {:?}",
        dangling_commits(dir)
    );
}

// ── #304.3: clean TRUE merge over 100+ files ─────────────────────────────

/// A real (two-parent) merge whose incoming side touches well over 100 files,
/// with NO conflicts (the two branches edit disjoint file sets). Asserts it
/// plans + executes, `preview_files` stays bounded (#301 cap = 200), and every
/// one of the touched files has the correct content on disk afterwards.
#[test]
fn merge_bulk_clean_execute_lands_all_files_and_bounds_preview() {
    const BASE: usize = 300;
    // feature edits f0..f259 (260 files); main edits f260..f299 (40 files).
    // Disjoint edits → a clean merge; the incoming diff (260 files) exceeds the
    // 200-file preview cap so we also assert the cap holds.
    const FEATURE_EDITS: usize = 260;

    let tmp = init_repo();
    let dir = tmp.path();

    for i in 0..BASE {
        write_file(dir, &format!("f{i}.txt"), "base\n");
    }
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "many base files"]);

    git(dir, &["checkout", "-qb", "feature"]);
    for i in 0..FEATURE_EDITS {
        write_file(dir, &format!("f{i}.txt"), "feature\n");
    }
    git(dir, &["commit", "-qam", "feature edits 0..260"]);
    let feature_before = rev_parse(dir, "feature");

    git(dir, &["checkout", "-q", "main"]);
    for i in FEATURE_EDITS..BASE {
        write_file(dir, &format!("f{i}.txt"), "main\n");
    }
    git(dir, &["commit", "-qam", "main edits 260..300"]);
    let main_before = rev_parse(dir, "main");

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan merge");
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert_eq!(
        kind,
        MergeKind::MergeCommit,
        "disjoint diverge → true merge"
    );
    // #301: the preview list is bounded even though 260 files change.
    assert!(
        plan.preview_files.len() <= 200,
        "preview_files must be capped at 200, got {}",
        plan.preview_files.len()
    );

    let merged = backend
        .execute_merge_branch("feature")
        .expect("execute merge");

    // Two-parent merge, current branch advanced, target untouched.
    assert_eq!(rev_parse(dir, "main"), merged.0);
    assert_ne!(merged.0, main_before);
    assert_eq!(
        rev_parse(dir, "feature"),
        feature_before,
        "target stays put"
    );
    let repo = git2::Repository::open(dir).expect("open repo");
    let commit = repo
        .find_commit(git2::Oid::from_str(&merged.0).unwrap())
        .expect("merge commit");
    assert_eq!(commit.parent_count(), 2, "a true merge has two parents");

    // Every touched file has the correct merged content ON DISK.
    for i in 0..FEATURE_EDITS {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("f{i}.txt"))).unwrap(),
            "feature\n",
            "feature-edited f{i}.txt must be on disk"
        );
    }
    for i in FEATURE_EDITS..BASE {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("f{i}.txt"))).unwrap(),
            "main\n",
            "main-edited f{i}.txt must be on disk"
        );
    }

    assert_eq!(porcelain(dir), "", "worktree clean after a 300-file merge");
    assert!(
        dangling_commits(dir).is_empty(),
        "bulk merge leaves no dangling objects: {:?}",
        dangling_commits(dir)
    );
}
