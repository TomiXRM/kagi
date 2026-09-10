//! #655: `working_tree_status` writes the index stat cache back, and still
//! answers when it cannot.
//!
//! libgit2 re-hashes every file whose `stat` data disagrees with the index and
//! never writes the refreshed data back, so a repo whose files were touched
//! without changing pays a full content hash on *every* scan. On a 50,000-file
//! repo that measured 3,305 ms per scan against 135 ms warm.

use git2::Repository;
use kagi_git::{working_tree_status, working_tree_status_repairing_stat_cache};
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use tempfile::TempDir;

/// The repair slot is process-global by design: only one scan at a time may try
/// to take `index.lock`. Cargo runs these tests as threads in one process, so
/// without this every test that asserts a repair *happened* would race the
/// others into skipping it.
static SERIAL: Mutex<()> = Mutex::new(());

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
fn stale_index_repo() -> (MutexGuard<'static, ()>, TempDir, Repository) {
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
    (guard, dir, repo)
}

fn index_mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path.join(".git/index"))
        .expect("index exists")
        .modified()
        .expect("mtime")
}

#[test]
fn working_tree_status_writes_the_refreshed_stat_cache_back() {
    let (_serial, dir, repo) = stale_index_repo();
    let before = index_mtime(dir.path());

    let status = working_tree_status_repairing_stat_cache(&repo).expect("status succeeds");

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
    let (_serial, dir, repo) = stale_index_repo();
    // What a concurrent `git` process holds while it writes the index.
    std::fs::write(dir.path().join(".git/index.lock"), b"").expect("lock");

    let status = working_tree_status_repairing_stat_cache(&repo)
        .expect("status is a read and must not fail because the repair leg cannot run");

    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "locked-index fallback must report the same clean tree: {status:?}"
    );
}

#[test]
fn working_tree_status_still_answers_when_the_git_dir_is_read_only() {
    let (_serial, dir, repo) = stale_index_repo();
    let gitdir = dir.path().join(".git");
    let mut perms = std::fs::metadata(&gitdir).expect("meta").permissions();
    let restore = perms.clone();
    perms.set_readonly(true);
    std::fs::set_permissions(&gitdir, perms).expect("chmod -w");

    let status = working_tree_status_repairing_stat_cache(&repo);

    std::fs::set_permissions(&gitdir, restore).expect("restore perms");
    let status = status.expect("a read-only .git must not break reading status");
    assert!(status.staged.is_empty() && status.unstaged.is_empty());
}

/// ADR-0193 requires this test to exist for the exemption to apply: the refresh
/// must leave every index entry's `(path, OID, mode)` untouched. Without it the
/// stat-cache write is an ordinary write under invariant 4 and would have to go
/// through plan/confirm/preflight/execute/verify/oplog, which a read path cannot.
#[test]
fn the_refresh_changes_no_staged_content() {
    fn entries(repo: &Repository) -> Vec<(Vec<u8>, git2::Oid, u32)> {
        let index = repo.index().expect("index");
        index
            .iter()
            .map(|e| (e.path.clone(), e.id, e.mode))
            .collect()
    }

    let (_serial, dir, repo) = stale_index_repo();
    let before = entries(&repo);
    assert!(
        !before.is_empty(),
        "fixture must stage something to compare"
    );

    working_tree_status_repairing_stat_cache(&repo).expect("status succeeds");

    // Re-open so we read what was actually written to disk, not a cached view.
    let reopened = Repository::open(dir.path()).expect("reopen");
    let after = entries(&reopened);

    assert_eq!(
        before, after,
        "the stat-cache refresh must not change which paths are staged, their blob \
         OIDs, or their modes — if it does, it is a write under invariant 4 and the \
         ADR-0193 exemption does not apply"
    );
}

/// The default must stay a pure read.
///
/// `working_tree_status` is reached from every `plan_*` / `preflight_*` /
/// snapshot path — over a hundred call sites, including `plan_create_branch`,
/// which runs *before* the user confirms anything. A plan that wrote the index
/// would violate invariant 4 no matter how narrow the write is, so the repair
/// is opt-in on one UI refresh path and must never leak into the default.
#[test]
fn the_default_status_never_writes_the_index() {
    let (_serial, dir, repo) = stale_index_repo();
    let before = index_mtime(dir.path());

    for _ in 0..3 {
        working_tree_status(&repo).expect("status succeeds");
    }

    assert_eq!(
        before,
        index_mtime(dir.path()),
        "working_tree_status is reached from plan/preflight paths and must not \
         write .git/index (invariant 4, ADR-0193)"
    );
}

/// Overlapping scans all still answer, and answer the same thing.
///
/// The watcher has no single-flight on same-revision status reads (~1.64
/// refreshes/s under continuous saves), so scans do overlap. This pins the
/// user-visible contract: whichever scan skips the repair or loses the lock
/// still returns the same status. It does **not** prove the repair slot is
/// what prevents contention — `repair_slot_tests` in `status.rs` covers that
/// directly, because removing the slot leaves this test green.
#[test]
fn concurrent_repairing_scans_all_return_the_same_status() {
    let (_serial, dir, _repo) = stale_index_repo();
    let path = dir.path().to_path_buf();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let repo = Repository::open(&path).expect("open");
                working_tree_status_repairing_stat_cache(&repo).map(|s| s.unstaged.len())
            })
        })
        .collect();

    for handle in handles {
        let unstaged = handle
            .join()
            .expect("no panic")
            .expect("every concurrent scan still answers");
        assert_eq!(unstaged, 0, "rewriting identical bytes is not a change");
    }
}

/// The default snapshot must stay a pure read too.
///
/// `snapshot` is not UI-only: `backend/remove.rs` calls it inside an ops path.
/// Repair belongs to `snapshot_repairing_stat_cache`, which only the UI calls.
#[test]
fn the_default_snapshot_never_writes_the_index() {
    let (_serial, dir, _repo) = stale_index_repo();
    let mut repo = Repository::open(dir.path()).expect("open");
    let before = index_mtime(dir.path());

    kagi_git::snapshot(&mut repo, 100).expect("snapshot succeeds");

    assert_eq!(
        before,
        index_mtime(dir.path()),
        "snapshot is reached from ops paths and must not write .git/index \
         (invariant 4, ADR-0193)"
    );
}

/// The UI variant repairs, so opening a big repo stops costing seconds forever.
#[test]
fn the_ui_snapshot_repairs_the_stat_cache() {
    let (_serial, dir, _repo) = stale_index_repo();
    let mut repo = Repository::open(dir.path()).expect("open");
    let before = index_mtime(dir.path());

    kagi_git::snapshot_repairing_stat_cache(&mut repo, 100).expect("snapshot succeeds");

    assert_ne!(
        before,
        index_mtime(dir.path()),
        "opening a repository is the moment a stale index costs the most (#655)"
    );
}
