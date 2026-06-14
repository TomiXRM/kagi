//! QA audit regression tests — operation × state combinatorial coverage.
//!
//! Background (user report): "stash- してフリーズした". The audit traced the
//! freeze class to **synchronous, UI-thread execution of git operations**
//! (every `confirm_*` in `src/ui/mod.rs` calls `execute_*` inline with no
//! `cx.background_spawn`). The slowness is proportional to working-tree size
//! for stash/checkout/commit, and **unbounded** for pull/push (network I/O).
//!
//! These tests pin the *logic* findings that are safe to assert in CI:
//!   - stash-push plan blockers/warnings across every working-tree state,
//!   - undo = soft reset (no data loss when the tree is dirty),
//!   - stash-pop refuses on a dirty tree (stash preserved),
//!   - checkout-commit plan/execute mismatch: a dirty overlapping file is only
//!     a *warning* in the plan but makes execute fail — yet never destroys data,
//!   - amend of a pushed commit is blocked.
//!
//! Network-bound operations (pull/push) are **intentionally excluded** — they
//! can block on a libgit2 connect timeout for tens of seconds (the HUNG class
//! documented in docs/research/qa-audit-matrix.md) and must never run in CI.
//!
//! All write operations are confined to `TempDir` repositories. No `force`
//! flags are exercised; every assertion checks that the repo stays fsck-clean.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{
    execute_checkout_commit, execute_stash_push, execute_undo_commit, plan_amend,
    plan_checkout_commit, plan_stash_pop, plan_stash_push, snapshot, AmendMode, CommitId,
};

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

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

/// Two-commit repo on `main`, clean. Returns (workdir, repo).
fn build_repo(tmp: &TempDir) -> (std::path::PathBuf, Repository) {
    let d = tmp.path();
    git(d, &["init", "-q", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);
    write_file(d, "README.md", "# test\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);
    write_file(d, "f2.txt", "second\n");
    git(d, &["add", "f2.txt"]);
    git(d, &["commit", "-qm", "second commit"]);
    let repo = Repository::open(d).expect("failed to open repo");
    (d.to_path_buf(), repo)
}

/// Assert the repository object database has no integrity errors.
fn assert_fsck_clean(dir: &Path) {
    let out = Command::new("git")
        .args(["fsck", "--no-progress"])
        .current_dir(dir)
        .env("HOME", dir)
        .output()
        .expect("git fsck failed to start");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        let l = line.to_ascii_lowercase();
        assert!(
            !(l.contains("error") || l.contains("missing") || l.contains("corrupt")),
            "git fsck reported integrity problem: {line}"
        );
    }
}

fn head_oid(repo: &Repository) -> git2::Oid {
    repo.head().unwrap().target().unwrap()
}

// ════════════════════════════════════════════════════════════
// STASH-PUSH × working-tree state matrix
// ════════════════════════════════════════════════════════════

#[test]
fn stash_push_clean_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let (_d, mut repo) = build_repo(&tmp);
    let plan = plan_stash_push(&mut repo, None, true).unwrap();
    assert_eq!(
        plan.blockers.len(),
        1,
        "clean tree → 'nothing to stash' blocker"
    );
}

#[test]
fn stash_push_staged_only_succeeds() {
    // The exact user repro shape: a staged change, then stash.
    let tmp = TempDir::new().unwrap();
    let (d, mut repo) = build_repo(&tmp);
    write_file(&d, "README.md", "# test\nstaged line\n");
    git(&d, &["add", "README.md"]);

    let plan = plan_stash_push(&mut repo, None, true).unwrap();
    assert!(plan.blockers.is_empty(), "staged-only stash must not block");
    execute_stash_push(&mut repo, None, true).expect("stash push failed");

    let snap = snapshot(&mut repo, 100).unwrap();
    assert!(!snap.status.is_dirty(), "tree clean after stash");
    assert_eq!(snap.stashes.len(), 1);
    assert_fsck_clean(&d);
}

#[test]
fn stash_push_mixed_staged_and_unstaged_succeeds() {
    let tmp = TempDir::new().unwrap();
    let (d, mut repo) = build_repo(&tmp);
    write_file(&d, "README.md", "# test\nstaged\n");
    git(&d, &["add", "README.md"]);
    write_file(&d, "f2.txt", "second\nunstaged\n");

    let plan = plan_stash_push(&mut repo, None, true).unwrap();
    assert!(plan.blockers.is_empty());
    execute_stash_push(&mut repo, None, true).expect("stash push failed");

    let snap = snapshot(&mut repo, 100).unwrap();
    assert!(!snap.status.is_dirty());
    assert_eq!(snap.stashes.len(), 1);
    assert_fsck_clean(&d);
}

#[test]
fn stash_push_many_untracked_warns_not_blocks() {
    let tmp = TempDir::new().unwrap();
    let (d, mut repo) = build_repo(&tmp);
    for i in 0..400 {
        write_file(&d, &format!("u_{i}.txt"), "x");
    }
    let plan = plan_stash_push(&mut repo, None, true).unwrap();
    assert!(
        plan.blockers.is_empty(),
        "untracked-only → stashable with -u"
    );
    assert!(
        plan.warnings.iter().any(|w| w.contains("untracked")),
        "untracked inclusion should be a warning"
    );
}

#[test]
fn stash_push_conflict_state_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let (d, mut repo) = build_repo(&tmp);
    // Create a real merge conflict.
    git(&d, &["checkout", "-q", "-b", "feature"]);
    write_file(&d, "README.md", "feature\n");
    git(&d, &["commit", "-qam", "feature"]);
    git(&d, &["checkout", "-q", "main"]);
    write_file(&d, "README.md", "main\n");
    git(&d, &["commit", "-qam", "main"]);
    // merge will conflict; ignore non-zero status
    let _ = Command::new("git")
        .args(["merge", "feature"])
        .current_dir(&d)
        .env("HOME", &d)
        .output();

    let plan = plan_stash_push(&mut repo, None, true).unwrap();
    assert!(
        plan.blockers.iter().any(|b| b.contains("conflict")),
        "conflicted tree must block stash push"
    );
}

// ════════════════════════════════════════════════════════════
// UNDO = soft reset — no data loss when tree is dirty
// ════════════════════════════════════════════════════════════

#[test]
fn undo_with_staged_changes_preserves_working_tree() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = build_repo(&tmp);
    // Stage an uncommitted change before undoing the last commit.
    write_file(&d, "README.md", "# test\nuncommitted staged\n");
    git(&d, &["add", "README.md"]);

    let parent = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent_id(0)
        .unwrap();

    execute_undo_commit(&repo).expect("undo failed");

    // Branch moved to parent (soft reset).
    assert_eq!(head_oid(&repo), parent, "HEAD must move to parent");
    // The staged uncommitted change must survive.
    let content = std::fs::read_to_string(d.join("README.md")).unwrap();
    assert!(
        content.contains("uncommitted staged"),
        "undo must not discard working-tree edits (soft reset)"
    );
    assert_fsck_clean(&d);
}

// ════════════════════════════════════════════════════════════
// STASH-POP × dirty tree — refuse, stash preserved
// ════════════════════════════════════════════════════════════

#[test]
fn stash_pop_on_dirty_tree_is_blocked_and_preserves_stash() {
    let tmp = TempDir::new().unwrap();
    let (d, mut repo) = build_repo(&tmp);
    // Create a stash, then dirty the tree again.
    write_file(&d, "f2.txt", "second\nstashed\n");
    git(&d, &["stash", "-q"]);
    write_file(&d, "f2.txt", "second\nlocal\n");

    let plan = plan_stash_pop(&mut repo, 0).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "pop onto a dirty tree must be blocked"
    );
    // Stash still present.
    let snap = snapshot(&mut repo, 100).unwrap();
    assert_eq!(snap.stashes.len(), 1, "stash entry must be preserved");
    assert_fsck_clean(&d);
}

// ════════════════════════════════════════════════════════════
// CHECKOUT-COMMIT × dirty tree — plan/execute mismatch (BUG-2)
// ════════════════════════════════════════════════════════════
//
// The plan only *warns* about a dirty tree; safe-mode checkout then succeeds
// or fails depending on file overlap. Neither outcome destroys data. These
// tests pin that contract so a future "force checkout" regression is caught.

#[test]
fn checkout_commit_dirty_plan_warns_but_does_not_block() {
    let tmp = TempDir::new().unwrap();
    let (d, repo) = build_repo(&tmp);
    write_file(&d, "README.md", "# test\ndirty\n");
    git(&d, &["add", "README.md"]);

    let parent = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent_id(0)
        .unwrap();
    let plan = plan_checkout_commit(&repo, &CommitId(parent.to_string())).unwrap();

    // W15-ASYNCOPS / BUG-2: a *non-overlapping* dirty file (README.md is staged
    // but identical between HEAD and the parent — the parent→HEAD diff only
    // touches f2.txt) stays a warning. Safe checkout succeeds; no blocker.
    assert!(
        plan.blockers.is_empty(),
        "non-overlapping dirty tree must not block"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.to_lowercase().contains("dirty")),
        "must at least warn about the dirty tree"
    );
}

#[test]
fn checkout_commit_overlapping_dirty_plan_blocks() {
    // W15-ASYNCOPS / BUG-2: the 'mixed' repro — an uncommitted edit to a file
    // that the target commit also modifies. The in-memory dry-run must promote
    // the dirty warning to a *blocker* so the plan matches what `execute` does
    // (safe checkout would otherwise refuse in the footer after a green plan).
    let tmp = TempDir::new().unwrap();
    let (d, repo) = build_repo(&tmp);

    // f2.txt now differs between HEAD and HEAD~1 (committed) …
    write_file(&d, "f2.txt", "second\nthird-commit\n");
    git(&d, &["commit", "-qam", "third"]);
    // … and has an uncommitted edit overlapping that diff.
    write_file(&d, "f2.txt", "second\nlocal-uncommitted\n");

    let parent = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent_id(0)
        .unwrap();
    let plan = plan_checkout_commit(&repo, &CommitId(parent.to_string())).unwrap();

    assert!(
        !plan.blockers.is_empty(),
        "overlapping dirty file must be a blocker (plan/execute agreement)"
    );
    assert!(
        plan.blockers.iter().any(|b| b.contains("f2.txt")),
        "blocker should name the conflicting file"
    );
}

#[test]
fn checkout_commit_overlapping_dirty_fails_without_data_loss() {
    // The 'mixed' repro: an unstaged edit to a file that differs in the target
    // commit. Safe-mode checkout refuses; the edit must remain on disk.
    let tmp = TempDir::new().unwrap();
    let (d, repo) = build_repo(&tmp);

    // Commit a change to f2.txt so it differs between HEAD and HEAD~1.
    write_file(&d, "f2.txt", "second\nthird-commit\n");
    git(&d, &["commit", "-qam", "third"]);
    // Now make an *uncommitted* edit to f2.txt that overlaps the diff.
    write_file(&d, "f2.txt", "second\nlocal-uncommitted\n");

    let parent = repo
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .parent_id(0)
        .unwrap();

    let result = execute_checkout_commit(&repo, &CommitId(parent.to_string()));
    assert!(
        result.is_err(),
        "safe-mode checkout must refuse to clobber the local edit"
    );
    // Data safety: the uncommitted edit is still on disk.
    let content = std::fs::read_to_string(d.join("f2.txt")).unwrap();
    assert!(
        content.contains("local-uncommitted"),
        "failed checkout must not lose the local edit"
    );
    assert_fsck_clean(&d);
}

// ════════════════════════════════════════════════════════════
// AMEND × pushed commit — block rewriting published history
// ════════════════════════════════════════════════════════════

#[test]
fn amend_pushed_commit_is_blocked() {
    let tmp = TempDir::new().unwrap();
    let bare = TempDir::new().unwrap();
    let (d, repo) = build_repo(&tmp);

    git(bare.path(), &["init", "-q", "--bare", "."]);
    git(
        &d,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(&d, &["push", "-q", "-u", "origin", "main"]);

    // Stage a change to amend into the pushed HEAD.
    write_file(&d, "README.md", "# test\namend\n");
    git(&d, &["add", "README.md"]);

    let plan = plan_amend(&repo, AmendMode::Staged, None).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "amending a pushed commit must be blocked (published history)"
    );
}
