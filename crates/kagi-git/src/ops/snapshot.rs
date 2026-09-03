//! Working-tree snapshots — savepoints under `refs/kagi/snapshots/` (ADR-0154).
//!
//! A snapshot is a **real commit** of the working tree + index, written to the
//! ODB and pointed at by a ref under `refs/kagi/snapshots/<id>`. Because it is
//! a ref (not an unreachable blob like the discard backup, ADR-0046/0083), the
//! commit survives `git gc --prune=now` — that is the whole point (#282, #335).
//!
//! The `refs/kagi/` namespace lives outside `refs/heads` / `refs/remotes`, so
//! snapshots:
//!   - never appear in the branch list (`repo.branches(Local)` only walks
//!     `refs/heads`, see `crate::snapshot::collect_branches`), and
//!   - are never a push/fetch target (push always names the current branch
//!     explicitly — `ops::push` builds `git push -- <remote> <branch>`, never a
//!     wildcard refspec, and fetch refspecs come from the remote config).
//!
//! # Two entry points (#335 §4)
//!   1. **Explicit** — [`create_snapshot`] with a user message (the core).
//!   2. **Automatic** — taken before a destructive op runs (see
//!      `Backend::run`), gated by the `auto_snapshot` toggle (default on).
//!
//! Creating a snapshot only ADDS a ref — non-destructive, so it needs no plan.
//! **Restore** rewrites the working tree, so it goes through the full safe path
//! (`plan_restore_snapshot` / `preflight_restore_snapshot` /
//! `execute_restore_snapshot` / `verify_restore_snapshot`) and is recorded in
//! the oplog via `Backend::run`. Restore uses `checkout_tree` with force — never
//! `reset --hard` or `git clean` (AGENTS.md invariant #3).

use super::*;
use kagi_domain::plan_note::{SnapshotNote, SnapshotRecovery, SnapshotTitle};

/// The ref namespace for snapshots. Everything under `refs/kagi/` is invisible
/// to the branch list and to push/fetch.
pub const SNAPSHOT_REF_PREFIX: &str = "refs/kagi/snapshots/";

/// Default generation cap (#335 §5, PM-locked): keep at most this many
/// snapshots; creating the (cap+1)-th evicts the oldest. No day-based expiry.
pub const DEFAULT_SNAPSHOT_CAP: usize = 50;

/// One saved snapshot, listed newest-first by [`list_snapshots`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    /// The `<id>` portion of the ref (e.g. `"7"`). Aligns with the oplog
    /// sequence id where possible (#333), with a uniqueness suffix otherwise.
    pub id: String,
    /// Full ref name (`refs/kagi/snapshots/<id>`).
    pub refname: String,
    /// The snapshot commit's OID (hex).
    pub commit: String,
    /// The snapshot commit's summary line (the message passed to
    /// [`create_snapshot`]).
    pub message: String,
    /// Commit time, Unix epoch seconds — used only for display ordering.
    pub time: i64,
}

// ────────────────────────────────────────────────────────────
// create
// ────────────────────────────────────────────────────────────

/// Capture the current working tree + index as a snapshot commit under
/// `refs/kagi/snapshots/<id>` and return the created [`SnapshotEntry`].
///
/// The tree is built with `git add -A` semantics: tracked modifications and
/// **untracked-but-not-ignored** files are included, `.gitignore` is respected
/// (so `node_modules` is not swept in — #335 §5). The user's real index on disk
/// is never modified — the in-memory index mutation is discarded afterwards.
///
/// Non-destructive: it only writes objects and ADDS a ref.
pub fn create_snapshot(repo: &Repository, message: &str) -> Result<SnapshotEntry, GitError> {
    let tree_oid = write_worktree_tree(repo)?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| GitError::Other(format!("snapshot: tree lookup failed: {}", e.message())))?;

    let sig = build_signature(repo)?;

    // Parent = current HEAD commit if the branch is born; else a root commit.
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();

    let id = next_snapshot_id(repo);
    let refname = format!("{}{}", SNAPSHOT_REF_PREFIX, id);

    let commit_oid = repo
        .commit(Some(&refname), &sig, &sig, message, &tree, &parents)
        .map_err(|e| GitError::Other(format!("snapshot: commit failed: {}", e.message())))?;

    Ok(SnapshotEntry {
        id,
        refname,
        commit: commit_oid.to_string(),
        message: message.to_string(),
        time: sig.when().seconds(),
    })
}

/// Build a tree object representing the full working tree (`git add -A`) without
/// persisting the user's index. Returns the tree OID (written to the ODB).
fn write_worktree_tree(repo: &Repository) -> Result<git2::Oid, GitError> {
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("snapshot: index open failed: {}", e.message())))?;
    // `git add -A .`: update tracked entries, stage untracked-not-ignored files
    // and deletions. `IndexAddOption::DEFAULT` honours `.gitignore`.
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .map_err(|e| GitError::Other(format!("snapshot: add_all failed: {}", e.message())))?;
    let tree_oid = index
        .write_tree()
        .map_err(|e| GitError::Other(format!("snapshot: write_tree failed: {}", e.message())))?;
    // Discard the in-memory staging changes so the real index (on disk, never
    // written here) and any later reads through this Repository are unaffected.
    let _ = index.read(true);
    Ok(tree_oid)
}

/// Pick the next snapshot id: the oplog's next sequence id (#333) when it can
/// be read, else the current Unix timestamp. Guarantees uniqueness by bumping
/// past any ref that already exists (two manual snapshots before any op both
/// peek the same oplog id, so the collision check is required).
fn next_snapshot_id(repo: &Repository) -> String {
    let base = match crate::oplog::read_oplog_tail(1).first() {
        Some(prev) => prev.id.saturating_add(1),
        None => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    let mut n = base;
    loop {
        let refname = format!("{}{}", SNAPSHOT_REF_PREFIX, n);
        if repo.find_reference(&refname).is_err() {
            return n.to_string();
        }
        n = n.saturating_add(1);
    }
}

// ────────────────────────────────────────────────────────────
// list / prune / delete
// ────────────────────────────────────────────────────────────

/// All snapshots, newest-first (by commit time, then id descending).
pub fn list_snapshots(repo: &Repository) -> Result<Vec<SnapshotEntry>, GitError> {
    let glob = format!("{}*", SNAPSHOT_REF_PREFIX);
    let refs = repo
        .references_glob(&glob)
        .map_err(|e| GitError::Other(format!("snapshot: ref glob failed: {}", e.message())))?;

    let mut out = Vec::new();
    for r in refs.flatten() {
        let Ok(refname) = r.name() else { continue };
        let Some(id) = refname.strip_prefix(SNAPSHOT_REF_PREFIX) else {
            continue;
        };
        let Ok(commit) = r.peel_to_commit() else {
            continue;
        };
        out.push(SnapshotEntry {
            id: id.to_string(),
            refname: refname.to_string(),
            commit: commit.id().to_string(),
            message: commit.summary().ok().flatten().unwrap_or("").to_string(),
            time: commit.time().seconds(),
        });
    }
    // Newest first: by time, then numeric-ish id descending as a tiebreak.
    out.sort_by(|a, b| {
        b.time
            .cmp(&a.time)
            .then_with(|| snapshot_sort_key(&b.id).cmp(&snapshot_sort_key(&a.id)))
    });
    Ok(out)
}

/// Oldest-first ordering key (numeric ids sort numerically; the timestamp
/// fallback ids are also numeric, so a plain u128 parse orders both).
fn snapshot_sort_key(id: &str) -> u128 {
    id.parse::<u128>().unwrap_or(0)
}

/// Evict snapshots beyond the generation `cap`, deleting the oldest first.
/// Returns the ids that were removed. Deleting a ref only drops the savepoint
/// (the commit becomes unreachable and gc-able later) — it never touches the
/// working tree, so this is non-destructive to the user's files.
pub fn prune_snapshots(repo: &Repository, cap: usize) -> Result<Vec<String>, GitError> {
    let mut all = list_snapshots(repo)?; // newest-first
    if all.len() <= cap {
        return Ok(Vec::new());
    }
    // Drop the newest `cap`; the remainder (oldest) are evicted.
    let evict = all.split_off(cap);
    let mut removed = Vec::new();
    for entry in evict {
        delete_snapshot(repo, &entry.id)?;
        removed.push(entry.id);
    }
    Ok(removed)
}

/// Explicitly delete one snapshot by id. Non-destructive to the working tree.
pub fn delete_snapshot(repo: &Repository, id: &str) -> Result<(), GitError> {
    let refname = format!("{}{}", SNAPSHOT_REF_PREFIX, id);
    let mut r = repo
        .find_reference(&refname)
        .map_err(|e| GitError::Other(format!("snapshot '{}' not found: {}", id, e.message())))?;
    r.delete()
        .map_err(|e| GitError::Other(format!("snapshot: delete failed: {}", e.message())))
}

/// Whether a snapshot with `id` exists.
pub fn snapshot_exists(repo: &Repository, id: &str) -> bool {
    let refname = format!("{}{}", SNAPSHOT_REF_PREFIX, id);
    repo.find_reference(&refname).is_ok()
}

// ────────────────────────────────────────────────────────────
// restore: plan / preflight / execute / verify
// ────────────────────────────────────────────────────────────

/// Plan a restore of the working tree to snapshot `id`. `destructive: true` —
/// the UI must require an armed confirm (a savepoint is still taken first).
///
/// # Blockers
/// - the snapshot ref does not exist.
pub fn plan_restore_snapshot(repo: &Repository, id: &str) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty_display = if status.is_dirty() {
        "dirty".to_string()
    } else {
        "clean".to_string()
    };
    let current = StateSummary {
        head: head.display(),
        dirty: dirty_display.clone(),
    };

    let mut blockers: Vec<PlanNote> = Vec::new();
    if !snapshot_exists(repo, id) {
        blockers.push(PlanNote::Snapshot(SnapshotNote::SnapshotMissing {
            id: id.to_string(),
        }));
    }

    let warnings = vec![
        PlanNote::Snapshot(SnapshotNote::SavepointFirst),
        PlanNote::Snapshot(SnapshotNote::RewritesWorkingTree),
    ];

    // Restore does not move HEAD; the working tree changes to match the tree.
    let predicted = StateSummary {
        head: head.display(),
        dirty: dirty_display,
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Snapshot(SnapshotTitle::Restore { id: id.to_string() }),
        current,
        predicted,
        warnings,
        blockers,
        recovery: Some(PlanRecovery {
            kind: RecoveryKind::Snapshot(SnapshotRecovery::Restore),
            commands: Vec::new(),
        }),
        head_at_plan: head,
        stash_count_at_plan: 0,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: true,
    })
}

/// Re-check, immediately before execute, that the snapshot still exists (the
/// standard HEAD/stash preflight runs separately in `Backend::run`).
pub fn preflight_restore_snapshot(repo: &Repository, id: &str) -> Result<(), GitError> {
    if !snapshot_exists(repo, id) {
        return Err(GitError::Other(format!(
            "restore refused: snapshot '{}' no longer exists",
            id
        )));
    }
    Ok(())
}

/// Rewrite the working tree (and index) to match snapshot `id`.
///
/// Order (safe write path): **savepoint → checkout → verify**.
///   1. Take a fresh savepoint of the CURRENT working tree so the restore is
///      itself reversible.
///   2. `checkout_tree` with force — overwrites modified tracked files and
///      re-creates recorded files. No `reset --hard`, no `git clean`.
///   3. Verify (see [`verify_restore_snapshot`]).
///
/// Returns the savepoint's id (the recovery handle).
pub fn execute_restore_snapshot(repo: &Repository, id: &str) -> Result<String, GitError> {
    preflight_restore_snapshot(repo, id)?;

    // ── 1. Savepoint the current state first. ──
    let savepoint = create_snapshot(repo, &format!("savepoint before restoring snapshot {}", id))?;

    // ── 2. Checkout the target snapshot's tree (force). ──
    let refname = format!("{}{}", SNAPSHOT_REF_PREFIX, id);
    let commit = repo
        .find_reference(&refname)
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| {
            GitError::Other(format!("restore: snapshot lookup failed: {}", e.message()))
        })?;
    let tree = commit
        .tree()
        .map_err(|e| GitError::Other(format!("restore: tree lookup failed: {}", e.message())))?;

    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force();
    repo.checkout_tree(tree.as_object(), Some(&mut cb))
        .map_err(|e| GitError::Other(format!("restore: checkout_tree failed: {}", e.message())))?;

    // ── 3. Verify every file the snapshot recorded now matches on disk. ──
    // #418: the working tree is already overwritten here, so a verify failure
    // must NOT drop the savepoint id — it is the only handle back to the
    // pre-restore state. Keep it in the error so the caller (and the oplog
    // Failed entry) can surface the recovery handle.
    if let Err(e) = verify_restore_snapshot(repo, &tree) {
        return Err(GitError::Other(format!(
            "restore verify failed after overwriting the working tree; \
             recover the previous state from savepoint {}: {}",
            savepoint.id, e
        )));
    }

    Ok(savepoint.id)
}

/// Confirm that every blob the snapshot `tree` recorded is present in the
/// working tree with matching content. This is a **subset** check on purpose:
/// restore is additive (it never deletes — no `git clean`), so files created
/// after the snapshot legitimately remain and must not fail verification.
pub fn verify_restore_snapshot(repo: &Repository, tree: &git2::Tree) -> Result<(), GitError> {
    // Re-stage the current working tree into a throwaway tree, then check each
    // snapshot path resolves to the same OID there.
    let after_oid = write_worktree_tree(repo)?;
    let after = repo.find_tree(after_oid).map_err(|e| {
        GitError::Other(format!(
            "restore verify: tree lookup failed: {}",
            e.message()
        ))
    })?;

    let mut mismatch: Option<String> = None;
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return git2::TreeWalkResult::Ok;
        }
        let Ok(name) = entry.name() else {
            return git2::TreeWalkResult::Ok;
        };
        let path = format!("{}{}", dir, name);
        let on_disk = after
            .get_path(std::path::Path::new(&path))
            .ok()
            .map(|e| e.id());
        if on_disk != Some(entry.id()) {
            mismatch = Some(path);
            return git2::TreeWalkResult::Abort;
        }
        git2::TreeWalkResult::Ok
    })
    .map_err(|e| GitError::Other(format!("restore verify: walk failed: {}", e.message())))?;

    if let Some(path) = mismatch {
        return Err(GitError::Other(format!(
            "restore verify failed: '{}' does not match the snapshot",
            path
        )));
    }
    Ok(())
}
