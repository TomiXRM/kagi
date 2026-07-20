//! Integration tests for amend — T-COMMIT-010 (ADR-0040, MVP = unpushed only)
//!
//! All repositories are created inside `TempDir`s (no network access).
//!
//! | # | Name | What it covers |
//! |---|------|----------------|
//! | 1 | `test_amend_message_only` | message-only amend: new SHA, tree == old HEAD tree, message replaced, author preserved |
//! | 2 | `test_amend_staged_folds_changes` | staged amend: staged change folded into new commit, message kept |
//! | 3 | `test_amend_both` | both: staged folded + message replaced |
//! | 4 | `test_amend_author_preserved` | author identity is preserved, committer updated |
//! | 5 | `test_plan_amend_pushed_blocker` | pushed HEAD → plan blocker (ADR-0040 案B) |
//! | 6 | `test_plan_amend_merge_commit_blocker` | merge commit HEAD → plan blocker |
//! | 7 | `test_plan_amend_detached_blocker` | detached HEAD → plan blocker |
//! | 8 | `test_plan_amend_root_commit_blocker` | root commit → plan blocker |
//! | 9 | `test_plan_amend_message_empty_blocker` | message-only with empty message → blocker |
//! | 10 | `test_plan_amend_staged_nothing_blocker` | staged mode with nothing staged → blocker |
//! | 11 | `test_amend_round_trip` | amend then `git reset --hard <old>` restores original |
//! | 12 | `test_amend_preflight_mismatch` | HEAD moved since plan → preflight_check fails |
//! | 13 | `test_amend_no_upstream_allowed` | local branch without upstream → amend allowed |

use std::path::{Path, PathBuf};
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi_git::{execute_amend, plan_amend, preflight_check, AmendMode};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

fn git(dir: &Path, args: &[&str]) {
    git_as(dir, "Test", "test@example.com", args);
}

/// Run a git command with an explicit author/committer identity (via env).
/// Env vars are used (not `-c user.name`) because they take precedence and let
/// us author a specific commit independently of repo config.
fn git_as(dir: &Path, name: &str, email: &str, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", name)
        .env("GIT_AUTHOR_EMAIL", email)
        .env("GIT_COMMITTER_NAME", name)
        .env("GIT_COMMITTER_EMAIL", email)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .status()
        .expect("git command failed to start");
    assert!(status.success(), "git {} failed", args.join(" "));
}

fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

fn head_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_tree_sha(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD^{tree}"])
        .current_dir(dir)
        .output()
        .expect("rev-parse tree failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_message(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%B"])
        .current_dir(dir)
        .output()
        .expect("log message failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_author(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%an <%ae>"])
        .current_dir(dir)
        .output()
        .expect("log author failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A minimal local repo: a base commit + a second commit (the amend target).
struct LocalRepo {
    _tmp: TempDir,
    path: PathBuf,
}

/// `author_name`/`author_email` set the author of the SECOND (amend-target)
/// commit so author-preservation can be asserted independently of committer.
fn setup_local_with_author(author_name: &str, author_email: &str) -> LocalRepo {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Committer"]);
    git(&path, &["config", "user.email", "committer@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "base.txt", "base content\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "initial commit"]);

    // Second commit — the amend target — authored by a distinct identity.
    write_file(&path, "feature.txt", "feature v1\n");
    git(&path, &["add", "-A"]);
    git_as(
        &path,
        author_name,
        author_email,
        &["commit", "-qm", "add feature"],
    );

    LocalRepo { _tmp: tmp, path }
}

fn setup_local() -> LocalRepo {
    setup_local_with_author("Alice", "alice@example.com")
}

/// A repo with a bare remote that has pushed `main` (HEAD == upstream).
struct RepoWithRemote {
    _tmp: TempDir,
    local: PathBuf,
    _remote: PathBuf,
}

fn setup_with_remote() -> RepoWithRemote {
    let tmp = TempDir::new().expect("tempdir");
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");

    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );

    std::fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main", "."]);
    git(&local, &["config", "user.name", "Test"]);
    git(&local, &["config", "user.email", "test@example.com"]);
    git(&local, &["config", "commit.gpgsign", "false"]);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );

    write_file(&local, "base.txt", "base\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "base"]);
    write_file(&local, "second.txt", "second\n");
    git(&local, &["add", "-A"]);
    git(&local, &["commit", "-qm", "second"]);
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    RepoWithRemote {
        _tmp: tmp,
        local,
        _remote: remote,
    }
}

// ────────────────────────────────────────────────────────────
// Test 1: message-only amend
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_message_only() {
    let r = setup_local();

    let old_sha = head_sha(&r.path);
    let old_tree = head_tree_sha(&r.path);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reworded message")).unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);
    assert!(plan.destructive, "amend plan must be destructive");
    // SHA-change must be spelled out in the predicted line (旧 <short>).
    let predicted = format!("{} {}", plan.predicted.head, plan.predicted.dirty);
    assert!(
        predicted.contains(&old_sha[..8]),
        "predicted must mention old short SHA: {}",
        predicted
    );

    let outcome = execute_amend(&repo, AmendMode::MessageOnly, Some("reworded message")).unwrap();

    assert_eq!(outcome.old.0, old_sha, "outcome.old must be old HEAD");
    assert_ne!(outcome.new.0, old_sha, "amend must produce a NEW SHA");
    assert_eq!(
        head_sha(&r.path),
        outcome.new.0,
        "branch ref must point to new commit"
    );

    // Message-only → tree must be identical to the old HEAD tree.
    assert_eq!(
        head_tree_sha(&r.path),
        old_tree,
        "message-only amend must keep the old tree"
    );
    assert_eq!(
        head_message(&r.path),
        "reworded message",
        "message replaced"
    );

    // Author preserved.
    assert_eq!(head_author(&r.path), "Alice <alice@example.com>");
}

// ────────────────────────────────────────────────────────────
// Test 2: staged amend folds changes, message kept
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_staged_folds_changes() {
    let r = setup_local();

    let old_sha = head_sha(&r.path);
    let old_tree = head_tree_sha(&r.path);
    let old_message = head_message(&r.path);

    // Stage a change to feature.txt.
    write_file(&r.path, "feature.txt", "feature v2 amended\n");
    git(&r.path, &["add", "feature.txt"]);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::Staged, None).unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_amend(&repo, AmendMode::Staged, None).unwrap();
    assert_ne!(outcome.new.0, old_sha);
    assert_eq!(head_sha(&r.path), outcome.new.0);

    // Tree must differ (staged change folded in).
    assert_ne!(
        head_tree_sha(&r.path),
        old_tree,
        "staged amend must change the tree"
    );
    // The amended content is now in the commit's tree.
    assert_eq!(read_file(&r.path, "feature.txt"), "feature v2 amended\n");
    // Message is kept.
    assert_eq!(
        head_message(&r.path),
        old_message,
        "staged amend keeps message"
    );

    // Working tree must be clean after amend (staged change is committed,
    // and execute_amend does not leave the index out of sync).
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&r.path)
        .output()
        .unwrap();
    let st = String::from_utf8_lossy(&out.stdout);
    assert!(
        st.trim().is_empty(),
        "WT should be clean after folding the only staged change, got: {}",
        st
    );
}

// ────────────────────────────────────────────────────────────
// Test 3: both — staged folded + message replaced
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_both() {
    let r = setup_local();

    let old_sha = head_sha(&r.path);
    let old_tree = head_tree_sha(&r.path);

    write_file(&r.path, "feature.txt", "feature v3 both\n");
    git(&r.path, &["add", "feature.txt"]);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::Both, Some("both: fold + reword")).unwrap();
    assert!(plan.blockers.is_empty(), "blockers: {:?}", plan.blockers);

    let outcome = execute_amend(&repo, AmendMode::Both, Some("both: fold + reword")).unwrap();
    assert_ne!(outcome.new.0, old_sha);
    assert_ne!(
        head_tree_sha(&r.path),
        old_tree,
        "both must change the tree"
    );
    assert_eq!(read_file(&r.path, "feature.txt"), "feature v3 both\n");
    assert_eq!(
        head_message(&r.path),
        "both: fold + reword",
        "message replaced"
    );
    assert_eq!(
        head_author(&r.path),
        "Alice <alice@example.com>",
        "author preserved"
    );
}

// ────────────────────────────────────────────────────────────
// Test 4: author preserved / committer updated
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_author_preserved() {
    let r = setup_local_with_author("Bob Original", "bob@orig.example");

    let repo = Repository::open(&r.path).unwrap();
    execute_amend(&repo, AmendMode::MessageOnly, Some("reworded")).unwrap();

    // Author identity carried over from the old commit.
    assert_eq!(head_author(&r.path), "Bob Original <bob@orig.example>");

    // Committer is updated to the current repo signature (user.name = Committer).
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%cn <%ce>"])
        .current_dir(&r.path)
        .output()
        .unwrap();
    let committer = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        committer, "Committer <committer@example.com>",
        "committer updated"
    );
}

// ────────────────────────────────────────────────────────────
// Test 5: pushed HEAD → blocker (ADR-0040 案B)
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_pushed_blocker() {
    let r = setup_with_remote();

    let repo = Repository::open(&r.local).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword")).unwrap();

    assert!(
        !plan.blockers.is_empty(),
        "pushed commit must block amend, got: {:?}",
        plan.blockers
    );
    let msg = plan
        .blockers
        .iter()
        .map(|n| n.message_en())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        msg.contains("pushed") || msg.contains("upstream") || msg.contains("published"),
        "blocker must mention pushed/upstream: {}",
        msg
    );
}

// ────────────────────────────────────────────────────────────
// Test 6: merge commit → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_merge_commit_blocker() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "base.txt", "base\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "base"]);

    git(&path, &["checkout", "-q", "-b", "side"]);
    write_file(&path, "side.txt", "side\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "side commit"]);

    git(&path, &["checkout", "-q", "main"]);
    git(&path, &["merge", "--no-ff", "-m", "merge side", "side"]);

    let repo = Repository::open(&path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword")).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "merge commit must block amend, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.blockers
            .iter()
            .map(|n| n.message_en())
            .collect::<Vec<_>>()
            .join(" ")
            .contains("merge"),
        "blocker must mention merge: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 7: detached HEAD → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_detached_blocker() {
    let r = setup_local();
    let sha = head_sha(&r.path);
    git(&r.path, &["checkout", "--detach", &sha]);

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword")).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "detached HEAD must block amend, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.blockers
            .iter()
            .map(|n| n.message_en())
            .collect::<Vec<_>>()
            .join(" ")
            .contains("detached")
            || plan
                .blockers
                .iter()
                .map(|n| n.message_en())
                .collect::<Vec<_>>()
                .join(" ")
                .contains("branch"),
        "blocker must mention detached/branch: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 8: root commit → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_root_commit_blocker() {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_path_buf();

    git(&path, &["init", "-q", "-b", "main", "."]);
    git(&path, &["config", "user.name", "Test"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "commit.gpgsign", "false"]);

    write_file(&path, "only.txt", "only commit\n");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-qm", "root commit"]);

    let repo = Repository::open(&path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword")).unwrap();
    assert!(
        !plan.blockers.is_empty(),
        "root commit must block amend, got: {:?}",
        plan.blockers
    );
    assert!(
        plan.blockers
            .iter()
            .map(|n| n.message_en())
            .collect::<Vec<_>>()
            .join(" ")
            .contains("root")
            || plan
                .blockers
                .iter()
                .map(|n| n.message_en())
                .collect::<Vec<_>>()
                .join(" ")
                .contains("parent"),
        "blocker must mention root/parent: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 9: message-only with empty message → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_message_empty_blocker() {
    let r = setup_local();
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("   ")).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().contains("message")),
        "empty message must block, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 10: staged mode with nothing staged → blocker
// ────────────────────────────────────────────────────────────

#[test]
fn test_plan_amend_staged_nothing_blocker() {
    let r = setup_local();
    // Nothing staged (clean working tree).
    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::Staged, None).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.message_en().to_lowercase().contains("staged")
                || b.message_en().contains("Nothing")),
        "nothing-staged must block staged amend, got: {:?}",
        plan.blockers
    );
}

// ────────────────────────────────────────────────────────────
// Test 11: round-trip — amend then reset --hard <old> restores original
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_round_trip() {
    let r = setup_local();

    let old_sha = head_sha(&r.path);
    let old_tree = head_tree_sha(&r.path);
    let old_message = head_message(&r.path);

    let repo = Repository::open(&r.path).unwrap();
    let outcome = execute_amend(&repo, AmendMode::MessageOnly, Some("temporary reword")).unwrap();
    assert_ne!(head_sha(&r.path), old_sha);

    // Restore the original commit via reflog SHA.
    git(&r.path, &["reset", "--hard", &outcome.old.0]);

    assert_eq!(
        head_sha(&r.path),
        old_sha,
        "reset --hard <old> restores original SHA"
    );
    assert_eq!(head_tree_sha(&r.path), old_tree);
    assert_eq!(head_message(&r.path), old_message);
}

// ────────────────────────────────────────────────────────────
// Test 12: preflight mismatch — HEAD moved since plan
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_preflight_mismatch() {
    let r = setup_local();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword")).unwrap();
    assert!(plan.blockers.is_empty());

    // Move HEAD out from under the plan with a new commit.
    write_file(&r.path, "concurrent.txt", "concurrent\n");
    git(&r.path, &["add", "-A"]);
    git(&r.path, &["commit", "-qm", "concurrent commit"]);

    let repo2 = Repository::open(&r.path).unwrap();
    let result = preflight_check(&repo2, &plan);
    assert!(result.is_err(), "preflight must fail when HEAD has moved");
}

// ────────────────────────────────────────────────────────────
// Test 13: local branch without upstream → amend allowed
// ────────────────────────────────────────────────────────────

#[test]
fn test_amend_no_upstream_allowed() {
    let r = setup_local();

    let repo = Repository::open(&r.path).unwrap();
    let plan = plan_amend(&repo, AmendMode::MessageOnly, Some("reword on local")).unwrap();
    assert!(
        plan.blockers.is_empty(),
        "local-only branch must allow amend, got: {:?}",
        plan.blockers
    );

    let repo2 = Repository::open(&r.path).unwrap();
    let outcome = execute_amend(&repo2, AmendMode::MessageOnly, Some("reword on local")).unwrap();
    assert_eq!(head_sha(&r.path), outcome.new.0);
    assert_eq!(head_message(&r.path), "reword on local");
}
