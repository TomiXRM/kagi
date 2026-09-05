//! Persistence acceptance tests for per-worktree port blocks (issue #342,
//! ADR-0171). The pure allocation logic is unit-tested in
//! `kagi_domain::worktree_ports`; here we prove the JSON store makes an
//! assignment **survive a kagi restart** and that exhaustion is graceful.

use std::path::PathBuf;
use std::sync::Mutex;

use kagi_domain::worktree_ports::PortRange;
use kagi_git::worktree_ports::assign_block;

// `KAGI_LOG_DIR` is process-global; serialize the env-touching tests.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const RANGE: PortRange = PortRange {
    start: 3000,
    end: 3099,
};

/// Distinct, non-existent worktree paths (canonicalize falls back to lexical for
/// paths that do not exist, so these key stably without touching the FS).
fn wt(store: &std::path::Path, name: &str) -> PathBuf {
    store.join("worktrees").join(name)
}

/// A worktree path that actually exists on disk — a *live* worktree. Needed
/// wherever reclaim-on-exhaustion (#444) matters: a non-existent sibling would
/// be pruned as "removed" the moment the range fills, so a test that means to
/// prove "the range is genuinely full" must use live paths.
fn wt_live(store: &std::path::Path, name: &str) -> PathBuf {
    let p = wt(store, name);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn three_worktrees_get_three_different_consecutive_blocks() {
    let _g = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());

    let a = assign_block(&wt(store.path(), "a"), RANGE, 10).unwrap();
    let b = assign_block(&wt(store.path(), "b"), RANGE, 10).unwrap();
    let c = assign_block(&wt(store.path(), "c"), RANGE, 10).unwrap();

    assert_eq!((a, b, c), (3000, 3010, 3020));
    assert!(a != b && b != c && a != c);

    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn already_assigned_worktree_keeps_its_block_across_restart() {
    let _g = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());

    // First "run": three worktrees claim blocks.
    let a1 = assign_block(&wt(store.path(), "a"), RANGE, 10).unwrap();
    let _b = assign_block(&wt(store.path(), "b"), RANGE, 10).unwrap();
    let c1 = assign_block(&wt(store.path(), "c"), RANGE, 10).unwrap();

    // The store file exists on disk — nothing is held in memory between calls,
    // so each `assign_block` is a fresh read, exactly like a kagi restart.
    assert!(store.path().join("worktree_ports.json").exists());

    // "Restart": the same worktrees must get the SAME blocks, in any order.
    assert_eq!(assign_block(&wt(store.path(), "c"), RANGE, 10), Some(c1));
    assert_eq!(assign_block(&wt(store.path(), "a"), RANGE, 10), Some(a1));

    std::env::remove_var("KAGI_LOG_DIR");
}

#[test]
fn range_exhaustion_returns_none() {
    let _g = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());

    // A range holding exactly two blocks of 10.
    let small = PortRange {
        start: 3000,
        end: 3019,
    };
    // Live worktrees: both paths exist, so the range is genuinely full — reclaim
    // finds nothing to free.
    assert_eq!(
        assign_block(&wt_live(store.path(), "a"), small, 10),
        Some(3000)
    );
    assert_eq!(
        assign_block(&wt_live(store.path(), "b"), small, 10),
        Some(3010)
    );
    // Third worktree cannot fit — exhaustion is surfaced as None, not a wrapped
    // or out-of-range port.
    assert_eq!(assign_block(&wt_live(store.path(), "c"), small, 10), None);
    // ...but an already-assigned worktree still recalls its block.
    assert_eq!(
        assign_block(&wt_live(store.path(), "a"), small, 10),
        Some(3000)
    );

    std::env::remove_var("KAGI_LOG_DIR");
}

/// #444: a removed worktree's block is reclaimed on the next exhausting
/// assignment. Without the reclaim the store only grew and a full range never
/// recovered even as worktrees came and went.
#[test]
fn removed_worktree_block_is_reclaimed_on_exhaustion() {
    let _g = ENV_LOCK.lock().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::env::set_var("KAGI_LOG_DIR", store.path());

    let small = PortRange {
        start: 3000,
        end: 3019,
    };
    let a = wt_live(store.path(), "a");
    let b = wt_live(store.path(), "b");
    assert_eq!(assign_block(&a, small, 10), Some(3000));
    assert_eq!(assign_block(&b, small, 10), Some(3010));

    // Remove worktree `b` from disk (as `git worktree remove` would).
    std::fs::remove_dir_all(&b).unwrap();

    // A new worktree `c` would exhaust the range — but `b`'s block is now dead
    // and gets reclaimed, so `c` takes 3010 instead of failing.
    let c = wt_live(store.path(), "c");
    assert_eq!(assign_block(&c, small, 10), Some(3010));
    // `a` is untouched by the reclaim.
    assert_eq!(assign_block(&a, small, 10), Some(3000));
    // The dead entry is gone from the persisted store, not merely skipped.
    let json = std::fs::read_to_string(store.path().join("worktree_ports.json")).unwrap();
    assert!(
        !json.contains("worktrees/b\""),
        "reclaimed entry should be pruned from the store:\n{json}"
    );

    std::env::remove_var("KAGI_LOG_DIR");
}
