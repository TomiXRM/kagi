//! The settings store (#491): one owner for `settings.json` read-modify-write.
//!
//! Every read and write in the process goes through a single parsed document
//! guarded by [`STORE`], so a burst of setting changes (a column-divider drag
//! firing per half-pixel) no longer costs a full read+parse+write per key. The
//! file itself is only re-read when its `(mtime, len)` changed behind our back.
//!
//! Two safety properties this module exists for:
//!
//! * **A malformed file is preserved, never overwritten.** A `settings.json`
//!   that does not parse used to load as an empty default and get written
//!   straight back over the original, taking every other key with it —
//!   including `session_repos` / `session_active`, i.e. the tab set restored on
//!   the next launch. It is now moved aside to `settings.json.corrupt` before a
//!   fresh file is written, so the user's keys stay recoverable.
//! * **Saves are atomic.** A temp file in the same directory is fsync'd and
//!   renamed over the target, so an interrupted write leaves the previous file
//!   intact instead of a truncated one.

use super::settings_path;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// How long a burst of writes collapses into a single file write. The *first*
/// write after an idle period still lands synchronously, so an ordinary
/// preference change is as durable as it ever was; only the tail of a burst
/// (a divider drag firing per half-pixel) is deferred, and a short-lived
/// trailing thread writes the final value once the burst stops.
const FLUSH_WINDOW: Duration = Duration::from_millis(200);

/// The parsed `settings.json` document plus everything needed to write it back
/// safely. One instance per process, rebuilt when `KAGI_LOG_DIR` moves the file.
pub(super) struct Store {
    /// The file this document was parsed from. Pending writes go *here*, not to
    /// a freshly resolved path — tests flip `KAGI_LOG_DIR` under us.
    path: PathBuf,
    pub(super) doc: serde_json::Map<String, serde_json::Value>,
    /// `(mtime, len)` when the document was read, so an edit by another process
    /// (or by the GUI E2E runner) is picked up without re-parsing every read.
    stamp: Option<(SystemTime, u64)>,
    /// The file exists but could not be read or parsed. It must be moved aside
    /// before anything is written over it — this is the #491 data-loss guard.
    corrupt: bool,
    dirty: bool,
    last_flush: Option<Instant>,
    /// A trailing flush thread is already armed for the current burst.
    trailing: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn lock_store() -> MutexGuard<'static, Option<Store>> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// `(mtime, len)` of `path`, or `None` when it does not exist.
fn file_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((md.modified().ok()?, md.len()))
}

/// Read and parse the settings file. The returned store is flagged **corrupt**
/// when the file is present but unreadable or unparsable. A corrupt file yields
/// an empty document (settings stay best-effort) but is never written over in
/// place; [`flush_store`] moves it aside first.
fn load_doc(path: &Path) -> Store {
    let stamp = file_stamp(path);
    let (doc, corrupt) = match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
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
        stamp,
        corrupt,
        dirty: false,
        last_flush: None,
        trailing: false,
    }
}

/// Run `f` against the live settings document, (re)loading it first when the
/// resolved path moved or the file changed behind our back. `None` when no
/// settings path can be resolved at all (no `KAGI_LOG_DIR`, no `HOME`).
pub(super) fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    let path = settings_path()?;
    let mut guard = lock_store();
    let reload = match guard.as_mut() {
        Some(s) if s.path == path => {
            // A pending edit is newer than whatever is on disk, so only adopt an
            // external change when we have nothing outstanding.
            if !s.dirty && file_stamp(&path) != s.stamp {
                *s = load_doc(&path);
            }
            false
        }
        // The settings file moved (KAGI_LOG_DIR flipped): the pending document
        // belongs to the OLD file — get it there before switching.
        Some(s) => {
            flush_store(s);
            true
        }
        None => true,
    };
    if reload {
        *guard = Some(load_doc(&path));
    }
    guard.as_mut().map(f)
}

/// Mark the document dirty and get it to disk — immediately when the previous
/// write is older than [`FLUSH_WINDOW`], otherwise on one trailing thread that
/// collapses the rest of the burst into a single write.
pub(super) fn schedule_flush(s: &mut Store) {
    s.dirty = true;
    let within_burst = s.last_flush.is_some_and(|t| t.elapsed() < FLUSH_WINDOW);
    if !within_burst {
        flush_store(s);
    } else if !s.trailing {
        s.trailing = true;
        arm_trailing(FLUSH_WINDOW);
    }
}

/// Sleep out the burst window on a throwaway thread, then write whatever the
/// store holds by then. One thread per burst, not per write.
fn arm_trailing(wait: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        if let Some(s) = lock_store().as_mut() {
            s.trailing = false;
            flush_store(s);
        }
    });
}

/// Write the document back to its own path, atomically: a temp file in the same
/// directory, fsync'd, then renamed over the target. An interrupted write
/// therefore leaves the previous file intact rather than a truncated one.
///
/// A document parsed from a corrupt file moves the original to
/// `<name>.corrupt` first, so the user's keys survive as a recoverable file
/// instead of being overwritten by an empty default (#491). If that rename
/// fails, nothing is written — the original is worth more than the new key.
fn flush_store(s: &mut Store) {
    if !s.dirty {
        return;
    }
    s.dirty = false;
    s.last_flush = Some(Instant::now());
    if let Some(parent) = s.path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if s.corrupt {
        let aside = corrupt_path(&s.path);
        match std::fs::rename(&s.path, &aside) {
            Ok(()) => {
                klog!("settings: corrupt file kept at {}", aside.display());
                s.corrupt = false;
            }
            Err(e) => {
                klog!("settings: write failed (non-fatal): {e}");
                s.dirty = true;
                return;
            }
        }
    }
    let json = match serde_json::to_string_pretty(&s.doc) {
        Ok(json) => json,
        Err(e) => {
            klog!("settings: serialize failed (non-fatal): {e}");
            return;
        }
    };
    match write_atomic(&s.path, format!("{json}\n").as_bytes()) {
        Ok(()) => s.stamp = file_stamp(&s.path),
        Err(e) => {
            klog!("settings: write failed (non-fatal): {e}");
            s.dirty = true;
        }
    }
}

/// Where a corrupt `settings.json` is preserved: `settings.json.corrupt`.
fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".corrupt");
    path.with_file_name(name)
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

/// Write any coalesced settings change out now. Called on app quit so the tail
/// of a burst (or a preference changed just before exit) is never lost.
pub fn flush() {
    if let Some(s) = lock_store().as_mut() {
        flush_store(s);
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Tests — real files on disk (#491)
// ──────────────────────────────────────────────────────────────────────────
//
// These drive the path-taking internals (`load_doc` / `flush_store` /
// `write_atomic`) against a tempdir rather than the process-global store, so
// they exercise a genuine save+reload without touching `KAGI_LOG_DIR` (which
// sibling tests in this crate set and remove concurrently).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{scalar_to_string, Settings};

    fn set(s: &mut Store, key: &str, value: &str) {
        s.doc.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        s.dirty = true;
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

        let mut s = load_doc(&path);
        assert!(!s.corrupt, "a missing file is not corrupt");
        set(&mut s, "session_repos", "/a\u{1f}/b");
        set(&mut s, "session_active", "1");
        set(&mut s, "theme", "one-dark");
        set(&mut s, "future_only_key", "keepme");
        flush_store(&mut s);

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

        let mut s = load_doc(&path);
        assert!(s.corrupt, "unparsable settings must be flagged");
        assert!(s.doc.is_empty(), "a corrupt file reads as empty");

        // One ordinary setting change used to overwrite the file with the empty
        // default, taking session_repos (the restored tab set) with it.
        set(&mut s, "theme", "gruvbox");
        flush_store(&mut s);

        let aside = corrupt_path(&path);
        assert_eq!(
            std::fs::read_to_string(&aside).expect("original preserved"),
            original,
            "the corrupt original must be kept byte-for-byte"
        );
        assert!(!load_doc(&path).corrupt);
        assert_eq!(on_disk(&path, "theme").as_deref(), Some("gruvbox"));
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

    #[test]
    fn burst_of_writes_collapses_to_one_file_write() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("settings.json");
        let mut s = load_doc(&path);

        // The first write of a burst lands synchronously (durability unchanged).
        set(&mut s, "graph_col_w", "0");
        schedule_flush(&mut s);
        assert!(path.exists(), "the first write is not deferred");

        // The rest of the drag only updates memory. `trailing` is pre-armed so
        // this asserts the coalescing policy without spawning a thread.
        s.trailing = true;
        for w in 1..=200 {
            set(&mut s, "graph_col_w", &w.to_string());
            schedule_flush(&mut s);
        }
        assert!(s.dirty, "the burst is still pending in memory");
        assert_eq!(
            on_disk(&path, "graph_col_w").as_deref(),
            Some("0"),
            "200 drag events must not each hit the disk"
        );

        // …and the trailing flush writes the final value, once.
        flush_store(&mut s);
        assert_flat_strings(&std::fs::read_to_string(&path).expect("read"));
        assert_eq!(on_disk(&path, "graph_col_w").as_deref(), Some("200"));
    }
}
