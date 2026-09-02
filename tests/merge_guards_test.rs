//! Merge safety guards — issues #299 / #300 / #301.
//!
//! #299: unrelated histories block; an in-progress operation blocks; a revision
//!       expression (HEAD~3) is not accepted as a merge target.
//! #300: a true (non-FF) merge whose result would overwrite an untracked file
//!       blocks at plan time and refuses at execute time WITHOUT writing an
//!       orphaned merge commit; a clean true-merge still succeeds and leaves the
//!       object database dangling-free.
//! #301: a merge with many conflicts caps the file list shown in the plan note.
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

/// git that is allowed to fail (e.g. a conflicting `git merge`).
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
        .output()
        .expect("git command failed to start");
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write file");
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

fn rev_parse(dir: &Path, rev: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(dir)
        .output()
        .expect("rev-parse");
    assert!(out.status.success(), "git rev-parse {} failed", rev);
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Lines from `git fsck --full` naming a dangling or unreachable commit — the
/// signature of an orphaned merge commit (#300). Empty means the ODB is clean.
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

// ── #299: unrelated histories ────────────────────────────────────────────

#[test]
fn merge_of_unrelated_histories_is_blocked() {
    let tmp = init_repo();
    let dir = tmp.path();

    // An orphan branch has its own root → no common ancestor with main.
    git(dir, &["checkout", "-q", "--orphan", "stranger"]);
    git(dir, &["rm", "-q", "-f", "base.txt"]);
    write_file(dir, "stranger.txt", "hello\n");
    git(dir, &["add", "stranger.txt"]);
    git(dir, &["commit", "-qm", "stranger root"]);
    git(dir, &["checkout", "-q", "main"]);

    let backend = Backend::open(dir).expect("open backend");
    let (plan, _kind) = backend.plan_merge_branch("stranger").expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("no common history")),
        "unrelated histories must be a blocker: blockers={:?} warnings={:?}",
        plan.blockers,
        plan.warnings
    );
}

#[test]
fn merge_with_a_shared_ancestor_is_not_blocked_by_the_unrelated_guard() {
    // Mutation guard for the above: a normal diverged merge must still proceed.
    let tmp = init_repo();
    let dir = tmp.path();
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan");
    assert!(
        !plan
            .blockers
            .iter()
            .any(|b| b.message_en().contains("no common history")),
        "a shared-ancestor merge must not trip the unrelated-histories guard: {:?}",
        plan.blockers
    );
    assert_eq!(kind, MergeKind::MergeCommit);
}

// ── #299: operation already in progress ──────────────────────────────────

#[test]
fn merge_blocks_while_another_merge_is_in_progress_even_if_conflicts_are_staged() {
    let tmp = init_repo();
    let dir = tmp.path();

    // Two branches that conflict on the same file.
    write_file(dir, "c.txt", "base\n");
    git(dir, &["add", "c.txt"]);
    git(dir, &["commit", "-qm", "add c"]);
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "c.txt", "feature\n");
    git(dir, &["commit", "-qam", "feature c"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "c.txt", "main\n");
    git(dir, &["commit", "-qam", "main c"]);

    // Leave the repo mid-merge, then STAGE the resolution. `status.conflicted`
    // is now empty, but `.git/MERGE_HEAD` still exists → repo.state()==Merge.
    git_allow_fail(dir, &["merge", "feature"]);
    write_file(dir, "c.txt", "resolved\n");
    git(dir, &["add", "c.txt"]);
    assert_eq!(
        rev_parse(dir, "MERGE_HEAD").len(),
        40,
        "merge is in progress"
    );

    // A second, unrelated branch to try to merge on top of the in-progress one.
    let backend = Backend::open(dir).expect("open backend");
    let (plan, _kind) = backend.plan_merge_branch("feature").expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("already in progress")),
        "an in-progress merge must block a new merge: {:?}",
        plan.blockers
    );

    // Execute must refuse too.
    let err = backend
        .execute_merge_branch("feature")
        .expect_err("execute must refuse mid-merge");
    assert!(
        format!("{err}").contains("already in progress"),
        "execute error: {err}"
    );
}

// ── #299: revision expressions are not merge targets ─────────────────────

#[test]
fn a_revision_expression_is_not_accepted_as_a_merge_target() {
    let tmp = init_repo();
    let dir = tmp.path();
    write_file(dir, "a.txt", "a\n");
    git(dir, &["add", "a.txt"]);
    git(dir, &["commit", "-qm", "second"]);

    let backend = Backend::open(dir).expect("open backend");
    for spec in ["HEAD~1", "@{-1}", ":/second"] {
        let res = backend.plan_merge_branch(spec);
        assert!(
            res.is_err(),
            "revision expression {spec:?} must not resolve as a merge target"
        );
    }
}

// ── #300: no orphan commit on an untracked collision ─────────────────────

#[test]
fn a_true_merge_that_would_clobber_an_untracked_file_blocks_and_leaves_no_orphan() {
    let tmp = init_repo();
    let dir = tmp.path();

    // feature adds `new.txt`; main diverges elsewhere → a real (non-FF) merge.
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "new.txt", "from feature\n");
    git(dir, &["add", "new.txt"]);
    git(dir, &["commit", "-qm", "feature adds new.txt"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main diverges"]);

    // An UNTRACKED file on main at the exact path the merge wants to create.
    write_file(dir, "new.txt", "untracked local work\n");

    let backend = Backend::open(dir).expect("open backend");
    let (plan, _kind) = backend.plan_merge_branch("feature").expect("plan");
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("new.txt") && b.message_en().contains("overwritten")),
        "untracked collision must be a plan-time blocker naming the file: {:?}",
        plan.blockers
    );

    let main_before = rev_parse(dir, "main");

    // Execute must refuse and name the file, and MUST NOT write an orphan.
    let err = backend
        .execute_merge_branch("feature")
        .expect_err("execute must refuse the collision");
    let err = format!("{err}");
    assert!(err.contains("new.txt"), "error must name the file: {err}");

    assert_eq!(rev_parse(dir, "main"), main_before, "ref must not move");
    assert_eq!(
        std::fs::read_to_string(dir.join("new.txt")).unwrap(),
        "untracked local work\n",
        "the untracked file must be untouched"
    );
    assert!(
        dangling_commits(dir).is_empty(),
        "no orphaned merge commit may be left behind: {:?}",
        dangling_commits(dir)
    );
}

#[test]
fn a_clean_true_merge_still_succeeds_and_leaves_no_dangling_objects() {
    // Mutation guard for the dry-run: the happy path must be unaffected.
    let tmp = init_repo();
    let dir = tmp.path();
    git(dir, &["checkout", "-qb", "feature"]);
    write_file(dir, "feature.txt", "feature\n");
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "-qm", "feature"]);
    git(dir, &["checkout", "-q", "main"]);
    write_file(dir, "main.txt", "main\n");
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "-qm", "main"]);

    let backend = Backend::open(dir).expect("open backend");
    let main_before = rev_parse(dir, "main");
    let feature_before = rev_parse(dir, "feature");
    let merged = backend.execute_merge_branch("feature").expect("merge");

    assert_eq!(
        rev_parse(dir, "main"),
        merged.0,
        "ref moves last, to the merge"
    );
    assert_ne!(merged.0, main_before);
    assert_eq!(
        rev_parse(dir, "feature"),
        feature_before,
        "target branch stays put"
    );
    assert!(
        dangling_commits(dir).is_empty(),
        "a clean merge leaves the ODB dangling-free: {:?}",
        dangling_commits(dir)
    );
}

// ── #301: capped conflict-file list in the plan note ─────────────────────

#[test]
fn a_merge_with_many_conflicts_caps_the_file_list_in_the_note() {
    const N: usize = 120; // well past the 50-file cap
    let tmp = init_repo();
    let dir = tmp.path();

    for i in 0..N {
        write_file(dir, &format!("f{i}.txt"), "base\n");
    }
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", "many files"]);

    git(dir, &["checkout", "-qb", "feature"]);
    for i in 0..N {
        write_file(dir, &format!("f{i}.txt"), "feature\n");
    }
    git(dir, &["commit", "-qam", "feature edits"]);
    git(dir, &["checkout", "-q", "main"]);
    for i in 0..N {
        write_file(dir, &format!("f{i}.txt"), "main\n");
    }
    git(dir, &["commit", "-qam", "main edits"]);

    let backend = Backend::open(dir).expect("open backend");
    let (plan, kind) = backend.plan_merge_branch("feature").expect("plan");
    assert!(matches!(kind, MergeKind::Conflicts(_)));

    let note = plan
        .warnings
        .iter()
        .map(|w| w.message_en())
        .find(|m| m.contains("conflict(s)"))
        .expect("a WillConflict warning");

    // True count is reported…
    assert!(note.contains(&format!("{N} conflict(s)")), "note: {note}");
    // …but the list is capped and says how many more.
    assert!(
        note.contains("and 70 more"),
        "note must cap + count: {note}"
    );
    // The rendered line stays bounded regardless of N.
    assert!(
        note.len() < 2000,
        "note must be bounded: {} bytes",
        note.len()
    );

    // preview_files is bounded too (issue #301).
    assert!(
        plan.preview_files.len() <= 200,
        "preview_files must be capped: {}",
        plan.preview_files.len()
    );
}
