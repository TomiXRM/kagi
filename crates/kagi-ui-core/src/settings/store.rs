//! The settings store (#491): one owner for `settings.json` read-modify-write.
//!
//! Every read and write in the process goes through a parsed document guarded
//! by [`STORES`], so a burst of setting changes (a column-divider drag firing
//! per half-pixel) no longer costs a full read+parse+write per key.
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

use super::scalar_to_string;
use super::settings_path;
use std::collections::BTreeSet;
use std::io::Write as _;
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
        let Some(aside) = rescue(&s.path) else {
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

/// Move a corrupt `settings.json` aside, under a name **reserved exclusively**
/// so a second corruption never overwrites the first rescue (and two processes
/// never fight over one name). Returns where it went, or `None` if the original
/// could not be moved — in which case the caller must not write.
fn rescue(path: &Path) -> Option<PathBuf> {
    let base = path.file_name()?.to_os_string();
    for n in 0..100u32 {
        let mut name = base.clone();
        name.push(if n == 0 {
            ".corrupt".to_string()
        } else {
            format!(".corrupt.{n}")
        });
        let aside = path.with_file_name(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&aside)
        {
            // The reservation is an empty file we just created, so renaming the
            // original over it cannot destroy anyone else's rescue.
            Ok(_) => {
                return match std::fs::rename(path, &aside) {
                    Ok(()) => Some(aside),
                    Err(e) => {
                        klog!("settings: rescue rename failed: {e}");
                        let _ = std::fs::remove_file(&aside);
                        None
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                klog!("settings: rescue reservation failed: {e}");
                return None;
            }
        }
    }
    None
}

/// Replace `path` with `bytes` via a same-directory temp file + rename. The
/// rename is atomic on every platform kagi ships on; the preceding `sync_all`
/// makes the *contents* durable before the swap. This is crash-atomic, not a
/// power-loss guarantee — the containing directory is not fsync'd.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(name);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn store(path: &Path) -> Store {
        load_doc(path)
    }

    fn set(s: &mut Store, key: &str, value: &str) {
        s.doc.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        s.dirty = true;
        s.pending.insert(key.to_string());
    }

    fn on_disk(path: &Path, key: &str) -> Option<String> {
        load_doc(path).doc.get(key).and_then(scalar_to_string)
    }

    /// Every value on disk must still be a JSON **string** (ADR-0091/0092).
    fn assert_flat_strings(text: &str) {
        let v: serde_json::Value = serde_json::from_str(text).expect("on-disk settings must parse");
        for (k, v) in v.as_object().expect("flat object") {
            assert!(v.is_string(), "{k} must be stored as a string, got {v}");
        }
    }

    #[test]
    fn save_and_reload_round_trips_session_and_unknown_keys() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");

        let mut s = store(&path);
        assert!(!s.corrupt, "a missing file is not corrupt");
        set(&mut s, "session_repos", "/a\u{1f}/b");
        set(&mut s, "session_active", "1");
        set(&mut s, "theme", "one-dark");
        set(&mut s, "future_only_key", "keepme");
        assert!(flush_store(&mut s));

        assert_flat_strings(&std::fs::read_to_string(&path).expect("written"));

        // A fresh read of the real file sees every key, known and unknown.
        let back = Settings {
            raw: load_doc(&path).doc,
        };
        assert_eq!(back.get_str("session_repos").as_deref(), Some("/a\u{1f}/b"));
        assert_eq!(back.get_str("session_active").as_deref(), Some("1"));
        assert_eq!(back.theme().as_deref(), Some("one-dark"));
        assert_eq!(back.get_str("future_only_key").as_deref(), Some("keepme"));
    }

    #[test]
    fn corrupt_file_is_kept_aside_not_overwritten() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        // Truncated JSON — the exact shape a half-written file used to take.
        let original = "{\n  \"session_repos\": \"/a\u{1f}/b\",\n  \"theme\": \"one-dark\"";
        std::fs::write(&path, original).expect("seed");

        let mut s = store(&path);
        assert!(s.corrupt, "unparsable settings must be flagged");
        assert!(s.doc.is_empty(), "a corrupt file reads as empty");

        // One ordinary setting change used to overwrite the file with the empty
        // default, taking session_repos (the restored tab set) with it.
        set(&mut s, "theme", "gruvbox");
        assert!(flush_store(&mut s));

        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.corrupt")).expect("rescued"),
            original,
            "the corrupt original must be kept byte-for-byte"
        );
        assert!(!load_doc(&path).corrupt);
        assert_eq!(on_disk(&path, "theme").as_deref(), Some("gruvbox"));
    }

    /// #617 review 1: a fixed `.corrupt` name would let a second corruption
    /// replace the only surviving copy of the first one's session data.
    #[test]
    fn second_corruption_does_not_overwrite_the_first_rescue() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");

        let first = "{ \"session_repos\": \"/first\"";
        std::fs::write(&path, first).expect("seed");
        let mut s = store(&path);
        set(&mut s, "theme", "a");
        assert!(flush_store(&mut s));

        // The freshly written file is corrupted again by something else.
        let second = "{ \"session_repos\": \"/second\"";
        std::fs::write(&path, second).expect("re-corrupt");
        let mut s = store(&path);
        set(&mut s, "theme", "b");
        assert!(flush_store(&mut s));

        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.corrupt")).expect("first rescue"),
            first,
            "the first rescue must survive a second corruption"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("settings.json.corrupt.1"))
                .expect("second rescue"),
            second
        );
        assert_eq!(on_disk(&path, "theme").as_deref(), Some("b"));
    }

    /// Two rescues racing for a name must not collide: the reservation is
    /// exclusive, so each caller gets its own destination.
    #[test]
    fn rescue_reserves_a_name_exclusively() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut taken = Vec::new();
        for i in 0..3 {
            let path = tmp.path().join("settings.json");
            std::fs::write(&path, format!("garbage {i}")).expect("seed");
            let aside = rescue(&path).expect("rescue");
            assert!(!taken.contains(&aside), "{aside:?} handed out twice");
            assert_eq!(
                std::fs::read_to_string(&aside).expect("rescued"),
                format!("garbage {i}")
            );
            taken.push(aside);
        }
    }

    /// When the original cannot be moved, the write is refused outright rather
    /// than replacing it. Here the rescue name is occupied by a non-empty
    /// *directory*, which `create_new` rejects and `rename` cannot replace.
    #[test]
    fn rescue_failure_refuses_the_write_and_keeps_the_original() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        let original = "{ \"session_repos\": \"/keepme\"";
        std::fs::write(&path, original).expect("seed");
        // Block every candidate name with directories.
        std::fs::create_dir(tmp.path().join("settings.json.corrupt")).expect("mkdir");
        for n in 1..100 {
            std::fs::create_dir(tmp.path().join(format!("settings.json.corrupt.{n}")))
                .expect("mkdir");
        }

        let mut s = store(&path);
        set(&mut s, "theme", "gruvbox");
        assert!(!flush_store(&mut s), "a failed rescue must report failure");
        assert!(s.dirty, "the change stays pending");
        assert_eq!(
            std::fs::read_to_string(&path).expect("original"),
            original,
            "the original must be untouched when it cannot be rescued"
        );
    }

    /// #617 review 2: the store is dirty (a burst is pending) when something
    /// else corrupts the file. The flush must still rescue it — "the original
    /// is always preserved" has no dirty-path exception.
    #[test]
    fn external_corruption_during_a_burst_is_still_rescued() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, "{ \"theme\": \"one-dark\" }\n").expect("seed");

        let mut s = store(&path);
        assert!(!s.corrupt);
        set(&mut s, "graph_col_w", "120"); // pending, not yet flushed

        let garbage = "{ \"session_repos\": \"/gone\"";
        std::fs::write(&path, garbage).expect("corrupt behind our back");

        assert!(flush_store(&mut s));
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.corrupt")).expect("rescued"),
            garbage,
            "a file corrupted after load must still be rescued"
        );
        assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
    }

    /// …and a *valid* external edit during a burst is merged, not clobbered:
    /// only the keys this process wrote are replayed on top of theirs.
    #[test]
    fn external_edit_during_a_burst_is_merged_not_clobbered() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, "{ \"theme\": \"one-dark\" }\n").expect("seed");

        let mut s = store(&path);
        set(&mut s, "graph_col_w", "120"); // ours, pending

        // Another writer replaces the file, changing a key we never touched and
        // adding one this build does not know.
        std::fs::write(
            &path,
            "{ \"theme\": \"gruvbox\", \"future_only_key\": \"keepme\" }\n",
        )
        .expect("external write");

        assert!(flush_store(&mut s));
        assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
        assert_eq!(
            on_disk(&path, "theme").as_deref(),
            Some("gruvbox"),
            "a key we did not write must keep the other writer's value"
        );
        assert_eq!(on_disk(&path, "future_only_key").as_deref(), Some("keepme"));
    }

    /// #617 review 3: `(mtime, len)` cannot see a same-size in-place edit with
    /// the timestamp pinned. The write path therefore compares bytes, so such
    /// an edit is still merged instead of overwritten.
    #[test]
    fn same_stamp_external_edit_is_detected_by_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, "{ \"theme\": \"aaaaaaaa\" }\n").expect("seed");

        let s_before = store(&path);
        let stamp_before = s_before.stamp;
        let mut s = s_before;
        set(&mut s, "graph_col_w", "120");

        // Same length, and the timestamps restored to exactly what we read.
        let times = std::fs::File::open(&path)
            .and_then(|f| f.metadata())
            .map(|m| {
                std::fs::FileTimes::new()
                    .set_accessed(m.accessed().unwrap_or_else(|_| SystemTime::now()))
                    .set_modified(m.modified().expect("mtime"))
            })
            .expect("times");
        std::fs::write(&path, "{ \"theme\": \"bbbbbbbb\" }\n").expect("in-place edit");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_times(times))
            .expect("pin mtime");
        assert_eq!(
            file_stamp(&path),
            stamp_before,
            "the edit must be invisible to stat for this test to mean anything"
        );

        assert!(flush_store(&mut s));
        assert_eq!(
            on_disk(&path, "theme").as_deref(),
            Some("bbbbbbbb"),
            "a stat-invisible edit must still be merged, not overwritten"
        );
        assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("120"));
    }

    #[test]
    fn write_atomic_replaces_via_temp_and_leaves_none_behind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        std::fs::write(&path, "{\"theme\":\"old\"}\n").expect("seed");

        write_atomic(&path, b"{\"theme\":\"new\"}\n").expect("atomic write");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "{\"theme\":\"new\"}\n"
        );
        assert_eq!(
            std::fs::read_dir(tmp.path()).expect("read_dir").count(),
            1,
            "the temp file must be renamed away, not left next to settings.json"
        );

        // A failing replace (the target is a directory) leaves the previous
        // content readable and no partial temp file behind.
        let blocked = tmp.path().join("blocked");
        std::fs::create_dir(&blocked).expect("mkdir");
        let occupied = blocked.join("settings.json");
        std::fs::create_dir(&occupied).expect("mkdir target");
        assert!(write_atomic(&occupied, b"nope").is_err());
        assert_eq!(
            std::fs::read_dir(&blocked).expect("read_dir").count(),
            1,
            "a failed atomic write must clean up its temp file"
        );
    }

    /// #617 review 4: only a *repeated* key coalesces. Two different keys in a
    /// row are two user actions and must both be on disk immediately.
    #[test]
    fn distinct_keys_are_never_deferred() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        let mut s = store(&path);

        s.doc.insert("session_repos".into(), "/a".into());
        schedule_flush(&mut s, "session_repos");
        s.doc.insert("session_active".into(), "1".into());
        schedule_flush(&mut s, "session_active");

        assert!(!s.dirty, "a session save must not sit in memory");
        assert_eq!(on_disk(&path, "session_repos").as_deref(), Some("/a"));
        assert_eq!(on_disk(&path, "session_active").as_deref(), Some("1"));
    }

    #[test]
    fn burst_of_writes_collapses_to_one_file_write() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        let mut s = store(&path);

        // The first write of a burst lands synchronously (durability unchanged).
        s.doc.insert("graph_col_w".into(), "0".into());
        schedule_flush(&mut s, "graph_col_w");
        assert!(path.exists(), "the first write is not deferred");

        // The rest of the drag only updates memory. `trailing` is pre-armed so
        // this asserts the coalescing policy without spawning a thread.
        s.trailing = true;
        for w in 1..=200 {
            s.doc.insert("graph_col_w".into(), w.to_string().into());
            schedule_flush(&mut s, "graph_col_w");
        }
        assert!(s.dirty, "the burst is still pending in memory");
        assert_eq!(
            on_disk(&path, "graph_col_w").as_deref(),
            Some("0"),
            "200 drag events must not each hit the disk"
        );

        // …and the trailing flush writes the final value, once.
        assert!(flush_store(&mut s));
        assert_flat_strings(&std::fs::read_to_string(&path).expect("read"));
        assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("200"));
    }
}
