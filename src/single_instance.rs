//! Single-instance IPC (ADR-0102).
//!
//! When `kagi <repo>` is run while a Kagi instance is already running, the new
//! process forwards the repo path to the running instance over a per-user Unix
//! domain socket and exits; the running instance opens the repo as a new tab and
//! brings its window to the front.  Running bare `kagi` (no arg) while an
//! instance is running just focuses the existing window.
//!
//! This lives at the **shell** level (`src/main.rs` + this module), outside the
//! `src/ui/` git2 invariant.  The only UI-side code is the small drain loop
//! (`KagiApp::arm_single_instance_listener`), which calls `open_repository`
//! (Backend-backed, no git2 in UI) and `cx.activate(true)`.
//!
//! Unix (macOS/Linux) only.  On other platforms the functions are no-ops so the
//! crate still compiles; Kagi simply launches a second window there as before.
//!
//! Failure is always non-fatal: if the socket cannot be bound (permissions,
//! exotic temp dir, …) the primary runs normally **without** single-instance.
//! If `try_forward` cannot connect, the caller falls through to a normal launch.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, OnceLock};

/// Stash for the accept-thread receiver, handed off from `main()` (shell) to the
/// UI-side drain loop (`KagiApp::arm_single_instance_listener`).  The drain loop
/// `take()`s it exactly once; threading it through every `KagiApp` constructor
/// would touch dozens of call sites for no benefit (ADR-0102).
static PENDING_RX: OnceLock<Mutex<Option<Receiver<Option<PathBuf>>>>> = OnceLock::new();

/// Store the accept-thread receiver for the UI drain loop to pick up.
pub fn store_receiver(rx: Receiver<Option<PathBuf>>) {
    let _ = PENDING_RX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map(|mut slot| *slot = Some(rx));
}

/// Take the stashed accept-thread receiver (consumed once by the drain loop).
pub fn take_receiver() -> Option<Receiver<Option<PathBuf>>> {
    PENDING_RX
        .get()
        .and_then(|m| m.lock().ok())
        .and_then(|mut slot| slot.take())
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc::Sender;

    /// Per-user socket path, e.g. `/tmp/kagi-instance-<user>.sock`.
    ///
    /// Keyed by the `USER` env var (falling back to `uid` via `geteuid`-free
    /// means is awkward without new deps, so `USER` is used; an empty/missing
    /// `USER` falls back to a fixed name).  Lives in the system temp dir so it is
    /// writable and cleaned by the OS across reboots.
    pub fn socket_path() -> PathBuf {
        let user = std::env::var("USER")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default".to_string());
        // Sanitise: keep the filename single-segment even if USER is weird.
        let user = user.replace(['/', '\\', '.'], "_");
        std::env::temp_dir().join(format!("kagi-instance-{user}.sock"))
    }

    /// Try to forward `path` (or a focus-only request when `None`) to a running
    /// instance.  Returns `true` if a running instance accepted the connection
    /// (this process should then exit); `false` on any error (no instance, or a
    /// stale socket file with no listener — the caller launches normally).
    pub fn try_forward(path: Option<PathBuf>) -> bool {
        let mut stream = match UnixStream::connect(socket_path()) {
            Ok(s) => s,
            Err(_) => return false,
        };
        // One line: the canonical absolute repo path, or empty for focus-only.
        let line = match path {
            Some(p) => p.to_string_lossy().into_owned(),
            None => String::new(),
        };
        if stream.write_all(line.as_bytes()).is_err() {
            return false;
        }
        if stream.write_all(b"\n").is_err() {
            return false;
        }
        let _ = stream.flush();
        true
    }

    /// Remove any stale socket file, then bind a fresh listener.  Returns `None`
    /// on failure (the caller then runs normally without single-instance — never
    /// crashing).  Unlinking up-front means a previous crash can't permanently
    /// block (a leftover file with no listener would otherwise make `bind` fail).
    pub fn bind_listener() -> Option<UnixListener> {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        UnixListener::bind(&path).ok()
    }

    /// Spawn a background thread that accepts connections, reads the first line,
    /// and forwards `Some(path)` (non-empty) or `None` (focus-only) to `tx`.
    /// Individual connection errors are ignored so the loop never dies.
    pub fn spawn_accept_thread(listener: UnixListener, tx: Sender<Option<PathBuf>>) {
        std::thread::Builder::new()
            .name("kagi-single-instance".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    let stream = match conn {
                        Ok(s) => s,
                        Err(_) => continue, // transient accept error: keep going.
                    };
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        continue;
                    }
                    let trimmed = line.trim();
                    let msg = if trimmed.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(trimmed))
                    };
                    // If the UI side is gone the send fails; nothing else to do.
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            })
            .ok();
    }
}

// ── Non-unix fallbacks (compile-only; single-instance is a no-op) ───────────
#[cfg(not(unix))]
mod imp {
    use super::*;

    pub fn try_forward(_path: Option<PathBuf>) -> bool {
        false
    }
    pub fn bind_listener() -> Option<()> {
        None
    }
}

#[cfg(unix)]
pub use imp::{bind_listener, spawn_accept_thread, try_forward};

#[cfg(not(unix))]
#[allow(unused_imports)]
pub use imp::{bind_listener, try_forward};
