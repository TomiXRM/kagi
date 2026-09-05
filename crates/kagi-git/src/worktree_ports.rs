//! Persistence for per-worktree port blocks (issue #342, ADR-0171).
//!
//! Thin I/O wrapper around the pure allocator in
//! [`kagi_domain::worktree_ports`]. The pure core decides *which* block a
//! worktree gets; this module remembers that decision across kagi restarts by
//! storing `{ canonical worktree path → first port }` in a small JSON file
//! **beside the other local state** — the same `$KAGI_LOG_DIR` → `$HOME/.kagi`
//! resolution the oplog and the worktree-trust store already use.
//!
//! Persisting is what makes assignment idempotent in practice: a worktree keeps
//! the same block whether or not the pure allocator would recompute the same
//! answer, and regardless of which sibling worktrees exist this run.
//!
//! v1 assigns **numbers only** — no socket is bound — so a stored port can still
//! be taken by an unrelated process (documented in the ADR).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kagi_domain::worktree_ports::{allocate_block, env_map, PortRange};

const STORE_FILE: &str = "worktree_ports.json";

/// Location of the assignment store. `$KAGI_LOG_DIR` first (test isolation),
/// then `$HOME/.kagi`. Mirrors `oplog` / `worktree_steps` trust-store paths.
fn store_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("KAGI_LOG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join(STORE_FILE));
        }
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|s| !s.is_empty())?;
    Some(PathBuf::from(home).join(".kagi").join(STORE_FILE))
}

/// Canonical string key for a worktree path. Canonicalizes when the path exists
/// (collapses symlinks / `..`, e.g. macOS `/var` → `/private/var`) so the same
/// worktree keys identically across runs; falls back to the lexical path when it
/// cannot be canonicalized (e.g. not yet created).
fn canon_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Read the persisted `{ path → first port }` map. Missing or unparsable file
/// yields an empty map (best-effort, matching the settings/trust stores).
fn read_store() -> BTreeMap<String, u16> {
    let Some(path) = store_path() else {
        return BTreeMap::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Persist the whole map (pretty JSON), creating the parent dir. Best-effort;
/// a write failure is logged to stderr and otherwise ignored.
fn write_store(store: &BTreeMap<String, u16>) {
    let Some(path) = store_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, format!("{json}\n")) {
                eprintln!("worktree_ports: write failed (non-fatal): {e}");
            }
        }
        Err(e) => eprintln!("worktree_ports: serialize failed (non-fatal): {e}"),
    }
}

/// Assign (or recall) this worktree's port block and persist it.
///
/// Idempotent and stable: if the worktree already has a stored block it is
/// returned untouched; otherwise the pure allocator picks the lowest free
/// aligned block within `range`, and it is written to the store so the next kagi
/// run (or a later terminal spawn) gets the same number. Returns `None` on
/// exhaustion — every block in `range` is taken — which the caller should
/// surface rather than hand out an out-of-range port.
///
/// ponytail: numbers-only, no socket bind (v1). A stored port can still be
/// grabbed by an unrelated process; bind-to-reserve is the ADR follow-up.
pub fn assign_block(worktree_path: &Path, range: PortRange, per: u16) -> Option<u16> {
    let key = canon_key(worktree_path);
    let mut store = read_store();
    if let Some(&existing) = store.get(&key) {
        return Some(existing);
    }
    // Reclaim-on-exhaustion (#444). The store only ever grew: a
    // create→remove→create cycle kept the removed worktree's block reserved
    // forever, so the default 100-port / per-10 range exhausted after ~10
    // cycles even with one live worktree. Only reclaim when we would otherwise
    // fail — a plain allocate keeps the common path untouched and preserves the
    // "assign before the worktree is created" contract (`canon_key` falls back
    // to the lexical path for a not-yet-created target, which must not be
    // pruned as "missing").
    let first = match allocate_block(range, per, &store, &key) {
        Some(f) => f,
        None => {
            // Drop entries whose worktree path no longer exists on disk (keys
            // are canonical paths), then retry once. Still `None` → genuinely
            // full even after reclaim; surface that to the caller.
            prune_missing(&mut store, &key);
            allocate_block(range, per, &store, &key)?
        }
    };
    store.insert(key, first);
    write_store(&store);
    Some(first)
}

/// Drop entries whose worktree path no longer exists on disk, except `keep`
/// (the current target). Best-effort: a path we cannot stat is left in place
/// (treated as possibly-live) rather than risking a false reclaim.
fn prune_missing(store: &mut BTreeMap<String, u16>, keep: &str) {
    store.retain(|path, _| path == keep || Path::new(path).exists());
}

/// The port + `KAGI_*` env map handed to a worktree's terminal (issue #342).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePortEnv {
    /// First port of the assigned block (`KAGI_PORT`).
    pub port: u16,
    /// The five `KAGI_*` vars, in `kagi_domain::worktree_ports::ENV_KEYS` order.
    pub vars: Vec<(&'static str, String)>,
}

/// Assign this worktree's block (persisting it) and build its full `KAGI_*`
/// environment. This is the single entry point a later terminal-wiring PR calls
/// when spawning the embedded shell for a worktree tab. `None` on exhaustion.
pub fn worktree_env(
    worktree_path: &Path,
    worktree_name: &str,
    main_worktree_path: &Path,
    default_branch: &str,
    range: PortRange,
    per: u16,
) -> Option<WorktreePortEnv> {
    let port = assign_block(worktree_path, range, per)?;
    let vars = env_map(
        worktree_path,
        worktree_name,
        main_worktree_path,
        default_branch,
        port,
    );
    Some(WorktreePortEnv { port, vars })
}
