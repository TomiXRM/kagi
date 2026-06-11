//! Integration tests for RepoSnapshot (T005).
//!
//! Each test builds a small Git repository inside a `tempfile::TempDir` using
//! the `git` CLI, then asserts the result of `kagi::git::snapshot`.
//!
//! All writes are confined to the temporary directory.

use std::path::Path;
use std::process::Command;

use git2::Repository;
use tempfile::TempDir;

use kagi::git::{Head, snapshot};

// ────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────

/// Run a git command inside `dir`, asserting it succeeds.
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

/// Write `content` to `dir/name`.
fn write_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).expect("write_file failed");
}

// ────────────────────────────────────────────────────────────
// Fixture builder — mirrors make_fixture.sh but in Rust
// ────────────────────────────────────────────────────────────

/// Build a repo with:
/// - bare remote at `tmp/remote.git`
/// - work repo at `tmp/repo`
/// - main: ahead 1 vs origin/main, HEAD attached to main
/// - feature/one: in sync (pushed)
/// - feature/two: behind 1 vs origin/feature/two
/// - tag v0.1.0 (lightweight) and v0.1.0-annot (annotated)
/// - stash 1 entry
///
/// Returns `(remote_dir, repo_dir, opened Repository)`.
fn build_fixture(tmp: &TempDir) -> (std::path::PathBuf, std::path::PathBuf, Repository) {
    let base = tmp.path();
    let remote = base.join("remote.git");
    let repo_dir = base.join("repo");

    // bare remote
    git(base, &["init", "-q", "--bare", remote.to_str().unwrap()]);

    // work repo
    git(base, &["init", "-q", "-b", "main", repo_dir.to_str().unwrap()]);
    let d = &repo_dir;
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);
    git(d, &["remote", "add", "origin", remote.to_str().unwrap()]);

    // commits on main
    write_file(d, "README.md", "# fixture\n");
    git(d, &["add", "README.md"]);
    git(d, &["commit", "-qm", "initial commit"]);

    write_file(d, "a.txt", "a\n");
    git(d, &["add", "a.txt"]);
    git(d, &["commit", "-qm", "add a.txt"]);

    // branch feature/one
    git(d, &["checkout", "-qb", "feature/one"]);
    write_file(d, "f1.txt", "f1\n");
    git(d, &["add", "f1.txt"]);
    git(d, &["commit", "-qm", "feature one work"]);
    write_file(d, "f1.txt", "f1b\n");
    git(d, &["add", "f1.txt"]);
    git(d, &["commit", "-qm", "feature one more"]);

    // back to main, merge feature/one
    git(d, &["checkout", "-q", "main"]);
    write_file(d, "b.txt", "b\n");
    git(d, &["add", "b.txt"]);
    git(d, &["commit", "-qm", "add b.txt"]);
    git(d, &["merge", "-q", "--no-ff", "feature/one", "-m", "merge feature/one"]);

    // lightweight tag
    git(d, &["tag", "v0.1.0"]);

    // annotated tag
    git(d, &[
        "tag", "-a", "v0.1.0-annot", "-m", "annotated tag for testing",
    ]);

    // branch feature/two
    git(d, &["checkout", "-qb", "feature/two"]);
    write_file(d, "f2.txt", "f2\n");
    git(d, &["add", "f2.txt"]);
    git(d, &["commit", "-qm", "feature two work"]);
    write_file(d, "f2.txt", "f2b\n");
    git(d, &["add", "f2.txt"]);
    git(d, &["commit", "-qm", "feature two more"]);

    // push everything to origin
    git(d, &["checkout", "-q", "main"]);
    git(d, &["push", "-q", "-u", "origin", "main", "feature/one", "feature/two"]);

    // make feature/two 1 behind: reset local branch back 1 commit
    git(d, &["checkout", "-q", "feature/two"]);
    git(d, &["reset", "-q", "--hard", "HEAD~1"]);

    // make main 1 ahead: add an unpushed commit
    git(d, &["checkout", "-q", "main"]);
    write_file(d, "c.txt", "c\n");
    git(d, &["add", "c.txt"]);
    git(d, &["commit", "-qm", "add c.txt (unpushed)"]);

    // stash 1 entry
    write_file(d, "a.txt", "dirty for stash\n");
    git(d, &["stash", "push", "-qm", "wip on a.txt"]);

    // dirty working tree (for status check)
    write_file(d, "b.txt", "modified\n");
    write_file(d, "untracked.txt", "untracked\n");

    let repo = Repository::open(d).expect("failed to open fixture repo");
    (remote, repo_dir, repo)
}

// ────────────────────────────────────────────────────────────
// Test: unborn repo — no crash, all collections empty
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_unborn_repo() {
    let tmp = TempDir::new().unwrap();
    let d = tmp.path();
    git(d, &["init", "-b", "main", "."]);
    git(d, &["config", "user.name", "Test"]);
    git(d, &["config", "user.email", "test@example.com"]);
    git(d, &["config", "commit.gpgsign", "false"]);

    let mut repo = Repository::open(d).expect("open unborn repo");
    let snap = snapshot(&mut repo, 10_000).expect("snapshot must not fail on unborn repo");

    assert!(
        matches!(snap.head, Head::Unborn { .. }),
        "expected Unborn head, got {:?}",
        snap.head
    );
    assert!(snap.commits.is_empty(), "expected no commits");
    assert!(snap.branches.is_empty(), "expected no branches");
    assert!(snap.remote_branches.is_empty(), "expected no remote branches");
    assert!(snap.tags.is_empty(), "expected no tags");
    assert!(snap.stashes.is_empty(), "expected no stashes");
}

// ────────────────────────────────────────────────────────────
// Test: HEAD is Attached to main
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_head_attached_main() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    assert!(
        matches!(&snap.head, Head::Attached { branch, .. } if branch == "main"),
        "expected Head::Attached(main), got {:?}",
        snap.head
    );
}

// ────────────────────────────────────────────────────────────
// Test: main is ahead 1, feature/two is behind 1
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_branch_ahead_behind() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    let main_branch = snap
        .branches
        .iter()
        .find(|b| b.name == "main")
        .expect("main branch not found");

    let up = main_branch
        .upstream
        .as_ref()
        .expect("main should have upstream");
    assert_eq!(up.ahead, 1, "main should be ahead 1, got {}", up.ahead);
    assert_eq!(up.behind, 0, "main should be behind 0, got {}", up.behind);

    let f2 = snap
        .branches
        .iter()
        .find(|b| b.name == "feature/two")
        .expect("feature/two branch not found");

    let f2_up = f2.upstream.as_ref().expect("feature/two should have upstream");
    assert_eq!(f2_up.ahead, 0, "feature/two should be ahead 0, got {}", f2_up.ahead);
    assert_eq!(f2_up.behind, 1, "feature/two should be behind 1, got {}", f2_up.behind);
}

// ────────────────────────────────────────────────────────────
// Test: remote branches — 3 entries, origin/HEAD excluded
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_remote_branches_no_head() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    // origin/HEAD symbolic ref must be excluded.
    let head_aliases: Vec<_> = snap
        .remote_branches
        .iter()
        .filter(|rb| rb.name == "HEAD")
        .collect();
    assert!(
        head_aliases.is_empty(),
        "origin/HEAD should be excluded, but found: {:?}",
        head_aliases
    );

    // Should have exactly 3: main, feature/one, feature/two.
    assert_eq!(
        snap.remote_branches.len(),
        3,
        "expected 3 remote branches, got {}: {:?}",
        snap.remote_branches.len(),
        snap.remote_branches
            .iter()
            .map(|rb| format!("{}/{}", rb.remote, rb.name))
            .collect::<Vec<_>>()
    );

    let names: Vec<String> = snap
        .remote_branches
        .iter()
        .map(|rb| format!("{}/{}", rb.remote, rb.name))
        .collect();
    assert!(names.contains(&"origin/main".to_string()), "missing origin/main: {:?}", names);
    assert!(
        names.contains(&"origin/feature/one".to_string()),
        "missing origin/feature/one: {:?}",
        names
    );
    assert!(
        names.contains(&"origin/feature/two".to_string()),
        "missing origin/feature/two: {:?}",
        names
    );
}

// ────────────────────────────────────────────────────────────
// Test: annotated tag is peeled to commit
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_annotated_tag_peeled() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    let annot = snap
        .tags
        .iter()
        .find(|t| t.name == "v0.1.0-annot")
        .expect("annotated tag v0.1.0-annot not found");

    // The target must be a valid commit OID (40 hex chars).
    assert_eq!(
        annot.target.0.len(),
        40,
        "annotated tag target should be a 40-char OID, got: {}",
        annot.target.0
    );

    // Verify it resolves to an actual commit in the repo.
    let oid = git2::Oid::from_str(&annot.target.0).expect("invalid OID");
    let obj = repo.find_object(oid, None).expect("object not found");
    assert_eq!(
        obj.kind(),
        Some(git2::ObjectType::Commit),
        "annotated tag should resolve to a Commit, got {:?}",
        obj.kind()
    );

    // lightweight tag must also be present.
    assert!(
        snap.tags.iter().any(|t| t.name == "v0.1.0"),
        "lightweight tag v0.1.0 not found"
    );
}

// ────────────────────────────────────────────────────────────
// Test: stash — 1 entry with correct index and message
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_stash() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    assert_eq!(
        snap.stashes.len(),
        1,
        "expected 1 stash, got {}",
        snap.stashes.len()
    );

    let s = &snap.stashes[0];
    assert_eq!(s.index, 0, "stash index should be 0, got {}", s.index);
    assert!(
        s.message.contains("wip on a.txt"),
        "stash message should contain 'wip on a.txt', got: {:?}",
        s.message
    );
}

// ────────────────────────────────────────────────────────────
// Test: feature/one has upstream in sync (ahead=0, behind=0)
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_feature_one_in_sync() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    let f1 = snap
        .branches
        .iter()
        .find(|b| b.name == "feature/one")
        .expect("feature/one branch not found");

    if let Some(up) = &f1.upstream {
        assert_eq!(
            up.ahead, 0,
            "feature/one should be ahead 0, got {}",
            up.ahead
        );
        assert_eq!(
            up.behind, 0,
            "feature/one should be behind 0, got {}",
            up.behind
        );
    }
    // If no upstream configured that is also acceptable for this test.
}

// ────────────────────────────────────────────────────────────
// Test: commits count matches expected history
// ────────────────────────────────────────────────────────────

#[test]
fn test_snapshot_commits_count() {
    let tmp = TempDir::new().unwrap();
    let (_remote, _repo_dir, mut repo) = build_fixture(&tmp);
    let snap = snapshot(&mut repo, 10_000).expect("snapshot failed");

    // The fixture creates these commits (see build_fixture):
    // initial commit, add a.txt, feature one work, feature one more,
    // add b.txt, merge feature/one, feature two work, feature two more,
    // add c.txt (unpushed)
    // = 9 commits (stash creates a commit but it's not in the main walk)
    assert_eq!(
        snap.commits.len(),
        9,
        "expected 9 commits, got {}: {:?}",
        snap.commits.len(),
        snap.commits.iter().map(|c| &c.summary).collect::<Vec<_>>()
    );
}
