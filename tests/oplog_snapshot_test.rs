//! Integration tests for working-tree snapshots (ADR-0154 / #335).
//!
//! Named `oplog_snapshot_test` (not `snapshot_test`, which already covers
//! `RepoSnapshot`) to avoid a filename clash. Mirrors #335 §6 acceptance:
//!   1. a snapshot survives `git gc --prune=now` and still restores (the
//!      discard ODB-blob backup FAILS this — a loose blob is pruned);
//!   2. `refs/kagi/snapshots/` never appears in the branch list;
//!   3. a snapshot is never a push target;
//!   4. restore goes through plan → `Backend::run` → oplog;
//!   5. exceeding the generation cap evicts the oldest.
//!
//! All writes are confined to `TempDir`s. The oplog test points `KAGI_LOG_DIR`
//! at its own tempdir and serializes on `ENV_LOCK` (the var is process-global).

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use tempfile::TempDir;

use kagi_git::{read_oplog_tail, Backend, OpOutcome, Operation, OperationOutcome};

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git failed to start");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Repo with one commit on `main` and `file.txt = "A\n"` (tracked).
fn build_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main", "."]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("file.txt"), "A\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "c1"]);
}

// ── 1. Survives gc + restores; a loose blob does NOT survive. ──────────────

#[test]
fn snapshot_survives_gc_and_restores() {
    let repo = TempDir::new().unwrap();
    let d = repo.path();
    build_repo(d);

    // Working-tree state we want to save: modify the tracked file + add an
    // untracked (not ignored) file.
    std::fs::write(d.join("file.txt"), "SAVED\n").unwrap();
    std::fs::write(d.join("new.txt"), "NEW\n").unwrap();

    let backend = Backend::open(d).expect("open");
    let snap = backend
        .create_snapshot("savepoint")
        .expect("create snapshot");

    // A loose, unreferenced blob — exactly what the discard ODB-blob backup
    // relies on (ADR-0046/0083). Its content is unique and lives in NO tree, so
    // gc --prune=now drops it; that is the weakness snapshots exist to fix
    // (#282). (The snapshot was already taken, so this file is not in it; we
    // remove it right after hashing so nothing later references the blob.)
    std::fs::write(d.join("dangle_tmp"), "UNREFERENCED-DANGLING-BLOB-42\n").unwrap();
    let dangling = git_out(d, &["hash-object", "-w", "dangle_tmp"])
        .trim()
        .to_string();
    std::fs::remove_file(d.join("dangle_tmp")).unwrap();

    // Now stomp the working tree so restore has something to undo.
    std::fs::write(d.join("file.txt"), "STOMPED\n").unwrap();

    // The whole point (#282/#335): gc must NOT drop the snapshot commit.
    git(d, &["gc", "--prune=now"]);

    // Snapshot commit still resolvable (reachable via refs/kagi/snapshots/).
    let t = git_out(d, &["cat-file", "-t", &snap.commit]);
    assert_eq!(t.trim(), "commit", "snapshot commit survived gc");

    // MUTATION EVIDENCE: the loose blob (discard-style backup) is pruned.
    let blob_type = Command::new("git")
        .args(["cat-file", "-t", &dangling])
        .current_dir(d)
        .env("HOME", d)
        .output()
        .unwrap();
    assert!(
        !blob_type.status.success(),
        "a loose/dangling blob must NOT survive gc --prune=now (this is why the \
         discard blob backup fails and snapshots exist)"
    );

    // Restore and confirm the saved content came back.
    let plan = backend
        .plan_restore_snapshot(&snap.id)
        .expect("plan restore");
    assert!(plan.blockers.is_empty(), "restore plan has no blockers");
    let mut backend = Backend::open(d).expect("reopen");
    backend
        .run(
            &Operation::RestoreSnapshot {
                id: snap.id.clone(),
            },
            &plan,
        )
        .expect("restore run");

    assert_eq!(
        std::fs::read_to_string(d.join("file.txt")).unwrap(),
        "SAVED\n",
        "tracked file restored to the snapshot content"
    );
    assert_eq!(
        std::fs::read_to_string(d.join("new.txt")).unwrap(),
        "NEW\n",
        "untracked-but-recorded file restored"
    );
}

// ── 2. refs/kagi/snapshots/ is not a branch. ───────────────────────────────

#[test]
fn snapshot_ref_not_in_branch_list() {
    let repo = TempDir::new().unwrap();
    let d = repo.path();
    build_repo(d);

    let backend = Backend::open(d).expect("open");
    let snap = backend.create_snapshot("s").expect("create");

    // git branch list.
    let branches = git_out(d, &["branch", "--list", "--all"]);
    assert!(
        !branches.contains(&snap.id) && !branches.contains("kagi/snapshots"),
        "snapshot must not appear in the branch list: {branches:?}"
    );

    // The backend's own branch snapshot (what the sidebar renders).
    let mut backend = Backend::open(d).expect("reopen");
    let repo_snap = backend.snapshot(10_000).expect("snapshot");
    assert!(
        repo_snap.branches.iter().all(|b| b.name != snap.id),
        "snapshot id must not be a local branch"
    );

    // But the ref DOES exist under refs/kagi/.
    let kagi_refs = git_out(d, &["for-each-ref", "refs/kagi/snapshots/"]);
    assert!(
        kagi_refs.contains("refs/kagi/snapshots/"),
        "the snapshot ref exists under refs/kagi/"
    );
}

// ── 3. A snapshot is never a push target. ──────────────────────────────────

#[test]
fn snapshot_not_a_push_target() {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    let local = tmp.path().join("local");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir_all(&local).unwrap();

    git(&remote, &["init", "-q", "--bare", "-b", "main", "."]);
    build_repo(&local);
    git(
        &local,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&local, &["push", "-q", "-u", "origin", "main"]);

    // Create a snapshot, then push everything the default refspec would send.
    let backend = Backend::open(&local).expect("open");
    let _snap = backend.create_snapshot("s").expect("create");

    // A normal `git push` (and even `--all`) must not carry refs/kagi/.
    git(&local, &["push", "-q", "origin", "main"]);
    let _ = Command::new("git")
        .args(["push", "-q", "--all", "origin"])
        .current_dir(&local)
        .env("HOME", &local)
        .status();

    // Only the ref *name* is meaningful here — a substring match on `snap.id`
    // (a short numeric id) can coincidentally hit an unrelated commit SHA in
    // `for-each-ref` output (seen on git 2.55 CI), so match the ref path itself.
    let remote_refs = git_out(&remote, &["for-each-ref", "--format=%(refname)"]);
    assert!(
        !remote_refs.contains("refs/kagi/"),
        "snapshot ref must never reach the remote: {remote_refs:?}"
    );
}

// ── 4. Restore goes through plan → run → oplog. ────────────────────────────

#[test]
fn restore_goes_through_plan_and_oplog() {
    let _guard = ENV_LOCK.lock().unwrap();
    let repo = TempDir::new().unwrap();
    let logdir = TempDir::new().unwrap();
    let prev = std::env::var("KAGI_LOG_DIR").ok();
    std::env::set_var("KAGI_LOG_DIR", logdir.path());

    let d = repo.path();
    build_repo(d);
    std::fs::write(d.join("file.txt"), "SAVED\n").unwrap();

    let backend = Backend::open(d).expect("open");
    let snap = backend.create_snapshot("s").expect("create");

    std::fs::write(d.join("file.txt"), "STOMPED\n").unwrap();

    let plan = backend.plan_restore_snapshot(&snap.id).expect("plan");
    let mut backend = Backend::open(d).expect("reopen");
    let outcome = backend
        .run(
            &Operation::RestoreSnapshot {
                id: snap.id.clone(),
            },
            &plan,
        )
        .expect("run");
    // #418: the savepoint id (recovery handle) must reach the caller, not be
    // dropped as `OperationOutcome::Unit`.
    let savepoint = match &outcome {
        OperationOutcome::RestoreSnapshot { savepoint } => savepoint.clone(),
        other => panic!("expected RestoreSnapshot outcome carrying a savepoint, got {other:?}"),
    };
    assert!(
        !savepoint.is_empty(),
        "savepoint id (recovery handle) must be present"
    );

    let tail = read_oplog_tail(10);
    let entry = tail
        .iter()
        .find(|e| e.op == "restore-snapshot")
        .expect("oplog has a restore-snapshot entry (proves run→oplog path)");
    // #418: the recovery handle must be persisted in the oplog `after`, mirroring
    // how discard records its backup blob SHA.
    match &entry.outcome {
        OpOutcome::Success { after } => assert!(
            after.dirty.contains(&savepoint),
            "oplog `after` must record the savepoint id {savepoint}, got dirty={:?}",
            after.dirty
        ),
        other => panic!("restore must be recorded as Success, got {other:?}"),
    }

    match prev {
        Some(v) => std::env::set_var("KAGI_LOG_DIR", v),
        None => std::env::remove_var("KAGI_LOG_DIR"),
    }
}

// ── 5. Generation cap evicts the oldest. ───────────────────────────────────

#[test]
fn cap_eviction_removes_oldest() {
    let repo = TempDir::new().unwrap();
    let d = repo.path();
    build_repo(d);

    let backend = Backend::open(d).expect("open");

    // Create 5 distinct snapshots (distinct trees so commit oids differ).
    for i in 0..5 {
        std::fs::write(d.join("file.txt"), format!("v{i}\n")).unwrap();
        backend.create_snapshot(&format!("s{i}")).expect("create");
    }

    // `list_snapshots` is newest-first and defines eviction order: prune keeps
    // the first `cap` and evicts the rest (the oldest). Assert prune agrees with
    // that list rather than assuming an id scheme — robust to the oplog/id base.
    let cap = 3;
    let before = backend.list_snapshots().unwrap();
    assert_eq!(before.len(), 5);
    let keep: Vec<String> = before[..cap].iter().map(|e| e.id.clone()).collect();
    let evict: Vec<String> = before[cap..].iter().map(|e| e.id.clone()).collect();

    let mut removed = backend.prune_snapshots(cap).expect("prune");
    removed.sort();
    let mut evict_sorted = evict.clone();
    evict_sorted.sort();
    assert_eq!(
        removed, evict_sorted,
        "prune evicts exactly the oldest beyond cap"
    );

    let remaining: Vec<String> = backend
        .list_snapshots()
        .unwrap()
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(remaining.len(), cap, "cap enforced");
    for k in &keep {
        assert!(remaining.contains(k), "the newest `cap` snapshots are kept");
    }
    for e in &evict {
        assert!(!remaining.contains(e), "evicted snapshots are gone");
    }

    // MUTATION EVIDENCE: with cap=5 (>= count) nothing is evicted.
    let removed_none = backend.prune_snapshots(5).expect("prune");
    assert!(removed_none.is_empty(), "no eviction when under the cap");
}
