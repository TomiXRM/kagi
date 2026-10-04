//! The settings store (#491): one owner for `settings.json` read-modify-write.
//!
//! Every read and write goes through the document guarded by [`STORES`]; a second
//! owner can overwrite corrupt-file rescues, concurrent unknown keys, and session
//! tabs restored at launch. Repeated divider updates still avoid per-key disk I/O.
//!
//! What this module exists to guarantee:
//!
//! * **A malformed file is preserved, never overwritten.** A `settings.json`
//!   that does not parse used to load as an empty default and get written
//!   straight back over the original, taking every other key with it —
//!   including `session_repos` / `session_active`, i.e. the tab set restored on
//!   the next launch. It is now moved aside under an **exclusively reserved**
//!   name (`settings.json.corrupt`, `…​.corrupt.1`, …) before a fresh file is
//!   written, so a second corruption cannot overwrite the first rescue. If the
//!   name cannot be reserved, or the rename fails, nothing is written at all.
//! * **The rescue has no exception.** The file is re-read immediately before
//!   every write, so a file corrupted *after* we loaded it — while a burst sat
//!   in memory — is still rescued rather than renamed over.
//! * **A concurrent writer's keys survive.** When the file changed underneath a
//!   pending burst, only the keys *this* process wrote are replayed on top of
//!   whatever is on disk now; the other writer's keys (known and unknown) stay.
//! * **Saves are atomic.** A temp file in the same directory is fsync'd and
//!   renamed over the target, so an interrupted write leaves the previous file
//!   intact instead of a truncated one.
//!
//! Drift is detected by **content**, not by `(mtime, len)`: a same-size
//! in-place edit with the timestamp pinned is invisible to `stat`, which is the
//! stat-cache trap #458 root-caused in libgit2. `stat` is only the cheap first
//! filter on the read path, backed by a content re-read every
//! [`VERIFY_INTERVAL`]; the *write* path always compares bytes.

use super::{parse_ui_zoom, scalar_to_string, settings_path};
use crate::atomic_file::{rescue, write_atomic};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// How long a *repeated* write of the same key collapses into one file write.
/// The first write of a burst still lands synchronously, and so does any write
/// of a **different** key — `session_repos` followed by `session_active` are
/// two distinct user actions, not a burst, and both go to disk immediately.
/// Only a key rewritten over and over (a divider drag) is deferred.
const FLUSH_WINDOW: Duration = Duration::from_millis(200);

/// How stale a cached document may get when `stat` reports no change. Guards
/// against an external edit that keeps both mtime and length.
const VERIFY_INTERVAL: Duration = Duration::from_millis(250);

/// How many settings files keep their own pending state. One in the app; more
/// only when `KAGI_LOG_DIR` moves (tests, a relocated profile).
// ponytail: a fixed cap, not an LRU cache — raise it if a real workflow ever
// juggles more than a handful of settings files at once.
const MAX_STORES: usize = 4;

/// The parsed `settings.json` document plus everything needed to write it back
/// safely. One instance **per settings file**: a store is never repurposed for
/// a different path, so a write that failed for one file cannot be dropped by
/// switching to another.
pub(super) struct Store {
    /// The file this document was parsed from. Pending writes go *here*, not to
    /// a freshly resolved path — tests flip `KAGI_LOG_DIR` under us.
    path: PathBuf,
    doc: serde_json::Map<String, serde_json::Value>,
    /// The exact bytes last read from (or written to) `path`. The authority for
    /// "did someone else change this file", since it cannot be spoofed by a
    /// same-size, same-mtime replacement.
    snapshot: Option<String>,
    /// `(mtime, len)` alongside `snapshot` — the cheap filter that avoids a
    /// read on the common case where nothing changed.
    stamp: Option<(SystemTime, u64)>,
    /// When `snapshot` was last checked against the file's real bytes.
    verified: Instant,
    /// The file exists but could not be read or parsed. It must be moved aside
    /// before anything is written over it — this is the #491 data-loss guard.
    corrupt: bool,
    dirty: bool,
    /// Keys written since the last successful flush. Replayed on top of an
    /// external edit so a concurrent writer's keys are not clobbered.
    pending: BTreeSet<String>,
    last_flush: Option<Instant>,
    /// The key written last, so "same key again" can be told from "next action".
    last_key: Option<String>,
    /// A trailing flush thread is already armed for the current burst.
    trailing: bool,
}

/// Every live settings document, most recently used first.
static STORES: Mutex<Vec<Store>> = Mutex::new(Vec::new());

fn lock_stores() -> MutexGuard<'static, Vec<Store>> {
    STORES.lock().unwrap_or_else(|e| e.into_inner())
}

/// `(mtime, len)` of `path`, or `None` when it does not exist.
fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

/// Read and parse the settings file. The returned store is flagged **corrupt**
/// when the file is present but unreadable or unparsable. A corrupt file yields
/// an empty document (settings stay best-effort) but is never written over in
/// place; [`flush_store`] rescues it first.
fn load_doc(path: &Path) -> Store {
    let stamp = file_stamp(path);
    let text = std::fs::read_to_string(path);
    let (doc, corrupt) = match &text {
        Ok(text) => match serde_json::from_str(text) {
            Ok(doc) => (doc, false),
            Err(e) => {
                klog!("settings: parse failed, original preserved: {e}");
                (serde_json::Map::new(), true)
            }
        },
        // Absent is normal (first run); present-but-unreadable is not, and must
        // not be silently replaced either.
        Err(e) => {
            let present = stamp.is_some();
            if present {
                klog!("settings: read failed, original preserved: {e}");
            }
            (serde_json::Map::new(), present)
        }
    };
    Store {
        path: path.to_path_buf(),
        doc,
        snapshot: text.ok(),
        stamp,
        verified: Instant::now(),
        corrupt,
        dirty: false,
        pending: BTreeSet::new(),
        last_flush: None,
        last_key: None,
        trailing: false,
    }
}

/// Adopt an external edit when there is nothing pending. `stat` is the cheap
/// filter; every [`VERIFY_INTERVAL`] the file's bytes are compared for real, so
/// an edit that preserves both mtime and length is still picked up.
fn refresh(s: &mut Store) {
    // A pending edit is newer than whatever is on disk. The drift is resolved
    // at flush time instead, where the file is re-read unconditionally.
    if s.dirty {
        return;
    }
    let stamp = file_stamp(&s.path);
    if stamp == s.stamp && s.verified.elapsed() < VERIFY_INTERVAL {
        return;
    }
    let disk = load_doc(&s.path);
    if disk.snapshot != s.snapshot || disk.corrupt != s.corrupt {
        *s = disk;
    } else {
        s.stamp = stamp;
        s.verified = Instant::now();
    }
}

/// Run `f` against the live document for the current settings path, loading it
/// first if this process has not touched that file yet. `None` when no settings
/// path can be resolved at all (no `KAGI_LOG_DIR`, no `HOME`).
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    let path = settings_path()?;
    let mut stores = lock_stores();
    let idx = match stores.iter().position(|s| s.path == path) {
        Some(i) => {
            refresh(&mut stores[i]);
            i
        }
        None => {
            stores.insert(0, load_doc(&path));
            // Retire the least recently used file, flushing anything it still
            // holds. Its own store kept that pending state until now — a path
            // switch never silently discarded it.
            while stores.len() > MAX_STORES {
                if let Some(mut old) = stores.pop() {
                    flush_store(&mut old);
                }
            }
            0
        }
    };
    Some(f(&mut stores[idx]))
}

/// Read one setting from the live document.
pub(super) fn read(key: &str) -> Option<String> {
    with_store(|s| s.doc.get(key).and_then(scalar_to_string)).flatten()
}

/// The whole live document (the backing map of [`super::Settings`]).
pub(super) fn snapshot() -> serde_json::Map<String, serde_json::Value> {
    with_store(|s| s.doc.clone()).unwrap_or_default()
}

/// Upsert (or remove, with `value = None`) one setting and get it to disk.
pub(super) fn write(key: &str, value: Option<&str>) {
    with_store(|s| {
        match value {
            Some(v) => {
                s.doc
                    .insert(key.to_string(), serde_json::Value::String(v.to_string()));
            }
            None => {
                s.doc.remove(key);
            }
        }
        schedule_flush(s, key);
    });
}

/// Atomically record the new scale base and the equivalent saved zoom. Unlike
/// two `write_setting` calls, one flush cannot expose a converted zoom without
/// its marker. The store lock also serializes this with ordinary setting writes.
pub(super) fn migrate_ui_scale_base() -> Option<u32> {
    with_store(migrate_ui_scale_base_store).flatten()
}

fn migrate_ui_scale_base_store(s: &mut Store) -> Option<u32> {
    let zoom = s.doc.get("ui_zoom").and_then(parse_ui_zoom);
    if s.doc
        .get("ui_scale_base")
        .and_then(scalar_to_string)
        .as_deref()
        == Some("v2")
    {
        return zoom;
    }

    let zoom = zoom.map(|old| {
        // round(old / 0.9), without multiplying a u32 in its own width.
        let converted = (u64::from(old) * 10 + 4) / 9;
        u32::try_from(converted).unwrap_or(u32::MAX)
    });
    if let Some(zoom) = zoom {
        s.doc.insert(
            "ui_zoom".into(),
            serde_json::Value::String(zoom.to_string()),
        );
        s.pending.insert("ui_zoom".into());
    }
    s.doc.insert(
        "ui_scale_base".into(),
        serde_json::Value::String("v2".into()),
    );
    s.pending.insert("ui_scale_base".into());
    s.dirty = true;
    s.last_key = None;
    flush_store(s);
    zoom
}

/// Mark the document dirty and get it to disk — immediately, unless this is the
/// *same key* rewritten inside [`FLUSH_WINDOW`], which is what a divider drag
/// looks like. A burst then costs one write per window plus one trailing write.
fn schedule_flush(s: &mut Store, key: &str) {
    s.dirty = true;
    s.pending.insert(key.to_string());
    let repeat = s.last_key.as_deref() == Some(key)
        && s.last_flush.is_some_and(|t| t.elapsed() < FLUSH_WINDOW);
    s.last_key = Some(key.to_string());
    if !repeat {
        flush_store(s);
    } else if !s.trailing {
        s.trailing = true;
        arm_trailing(s.path.clone(), FLUSH_WINDOW);
    }
}

/// Sleep out the burst window on a throwaway thread, then write whatever *that
/// file's* store holds by then. One thread per burst, and it can only ever
/// touch the store it was armed for.
fn arm_trailing(path: PathBuf, wait: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        let mut stores = lock_stores();
        if let Some(s) = stores.iter_mut().find(|s| s.path == path) {
            s.trailing = false;
            flush_store(s);
        }
    });
}

/// Write the document back to its own path, atomically: a temp file in the same
/// directory, fsync'd, then renamed over the target. An interrupted write
/// therefore leaves the previous file intact rather than a truncated one.
///
/// The file is re-read first, because it may have been replaced or corrupted
/// since we parsed it:
///
/// * corrupt now → rescue it under a reserved name and start a fresh file. If
///   the rescue fails, **nothing is written** — the original is worth more than
///   the new key, and the change stays pending.
/// * changed but valid → replay only the keys this process wrote on top of the
///   other writer's document, so their keys survive.
///
/// Returns `false` when the change is still pending afterwards.
fn flush_store(s: &mut Store) -> bool {
    if !s.dirty {
        return true;
    }
    if let Some(parent) = s.path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let disk = load_doc(&s.path);
    if disk.corrupt {
        let Some(aside) = rescue(&s.path, "settings") else {
            klog!("settings: rescue failed, write refused to protect the original");
            return false;
        };
        klog!("settings: corrupt file kept at {}", aside.display());
        s.snapshot = None;
        s.stamp = None;
    } else if disk.snapshot != s.snapshot {
        let mut base = disk.doc;
        for key in &s.pending {
            match s.doc.get(key) {
                Some(v) => base.insert(key.clone(), v.clone()),
                None => base.remove(key),
            };
        }
        s.doc = base;
    }
    let json = match serde_json::to_string_pretty(&s.doc) {
        Ok(json) => json,
        Err(e) => {
            klog!("settings: serialize failed (non-fatal): {e}");
            s.dirty = false; // Unwritable content; retrying cannot help.
            return true;
        }
    };
    let text = format!("{json}\n");
    match write_atomic(&s.path, text.as_bytes()) {
        Ok(()) => {
            s.dirty = false;
            s.corrupt = false;
            s.pending.clear();
            s.stamp = file_stamp(&s.path);
            s.snapshot = Some(text);
            s.verified = Instant::now();
            s.last_flush = Some(Instant::now());
            true
        }
        Err(e) => {
            klog!("settings: write failed (non-fatal): {e}");
            false
        }
    }
}

/// Write any coalesced settings change out now, for every file this process has
/// touched. Called on app quit so the tail of a burst is never lost.
pub fn flush() {
    for s in lock_stores().iter_mut() {
        flush_store(s);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Tests — real files on disk (#491)
// ──────────────────────────────────────────────────────────────────────────
//
// These drive the path-taking internals (`load_doc` / `flush_store` /
// `write_atomic` / `rescue`) against a tempdir rather than the process-global
// stores, so they exercise a genuine save+reload without touching
// `KAGI_LOG_DIR` (which sibling tests in this crate set and remove
// concurrently). The public global path is covered by `env_tests`, which runs
// each case in a child process.

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
