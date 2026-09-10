//! #655: `working_tree_status` writes the index stat cache back, and still
//! answers when it cannot.
//!
//! libgit2 re-hashes every file whose `stat` data disagrees with the index and
//! never writes the refreshed data back, so a repo whose files were touched
//! without changing pays a full content hash on *every* scan. On a 50,000-file
//! repo that measured 3,305 ms per scan against 135 ms warm.

use git2::Repository;
use kagi_git::working_tree_status;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// A committed repo whose index stat cache is stale: the files carry a newer
/// mtime than the index recorded, but identical content.
fn stale_index_repo() -> (TempDir, Repository) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path();
    git(path, &["init", "-q"]);
    git(path, &["config", "user.email", "t@example.com"]);
    git(path, &["config", "user.name", "t"]);
    for i in 0..8 {
        std::fs::write(path.join(format!("f{i}.txt")), format!("content {i}\n")).expect("write");
    }
    git(path, &["add", "-A"]);
    git(path, &["commit", "-qm", "seed"]);

    // Rewrite each file with the exact same bytes. Content still hashes equal,
    // but mtime moves, so the index stat cache no longer matches.
    for i in 0..8 {
        let file = path.join(format!("f{i}.txt"));
        let bytes = std::fs::read(&file).expect("read");
        std::fs::write(&file, bytes).expect("rewrite");
    }

    let repo = Repository::open(path).expect("open");
    (dir, repo)
}

fn index_mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path.join(".git/index"))
        .expect("index exists")
        .modified()
        .expect("mtime")
}

#[test]
fn working_tree_status_writes_the_refreshed_stat_cache_back() {
    let (dir, repo) = stale_index_repo();
    let before = index_mtime(dir.path());

    let status = working_tree_status(&repo).expect("status succeeds");

    // Touch-without-change is not a modification; the scan must stay honest.
    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "rewriting identical bytes is not a change: {status:?}"
    );
    assert_ne!(
        before,
        index_mtime(dir.path()),
        "the index stat cache was not written back, so every later scan re-hashes \
         the whole worktree (#655)"
    );
}

#[test]
fn working_tree_status_still_answers_while_the_index_is_locked() {
    let (dir, repo) = stale_index_repo();
    // What a concurrent `git` process holds while it writes the index.
    std::fs::write(dir.path().join(".git/index.lock"), b"").expect("lock");

    let status = working_tree_status(&repo)
        .expect("status is a read and must not fail because the repair leg cannot run");

    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "locked-index fallback must report the same clean tree: {status:?}"
    );
}

#[test]
fn working_tree_status_still_answers_when_the_git_dir_is_read_only() {
    let (dir, repo) = stale_index_repo();
    let gitdir = dir.path().join(".git");
    let mut perms = std::fs::metadata(&gitdir).expect("meta").permissions();
    let restore = perms.clone();
    perms.set_readonly(true);
    std::fs::set_permissions(&gitdir, perms).expect("chmod -w");

    let status = working_tree_status(&repo);

    std::fs::set_permissions(&gitdir, restore).expect("restore perms");
    let status = status.expect("a read-only .git must not break reading status");
    assert!(status.staged.is_empty() && status.unstaged.is_empty());
}
