//! Single-instance IPC (ADR-0102).
//!
//! When `kagi <repo>` is run while a Kagi instance is already running, the new
//! process forwards the repo path to the running instance over a per-user Unix
//! domain socket and exits; the running instance opens the repo as a new tab and
//! brings its window to the front.  Running bare `kagi` (no arg) while an
//! instance is running just focuses the existing window.
//!
//! This lives at the **shell** level (`src/main.rs` + this module), outside the
//! `src/ui/` git2 invariant. The application-owned drain in `src/ui/mod.rs`
//! handles CLI requests and Finder/Dock file URLs via `open_repository`, even
//! after the last repository window has been closed.
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

/// Stash for the accept-thread receiver, handed off from `main()` (shell) to
/// the application-owned UI drain. The receiver is taken once at application
/// startup, never by a repository entity whose window may be closed.
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

/// Queue native requests without requiring a window or an initialized UI.
pub fn enqueue_open_urls(urls: Vec<String>, tx: &std::sync::mpsc::Sender<PathBuf>) {
    for url in urls {
        if let Some(path) = file_url_path(&url) {
            let _ = tx.send(path);
        }
    }
}

/// Decode a local file URL delivered by GPUI's public `on_open_urls` API.
/// Non-file schemes and remote authorities are not repository-open requests.
/// Directory validation and failure presentation remain with the existing UI
/// external-path opener, rather than discovering a repository from a file.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let (scheme, rest) = url.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("file") {
        return None;
    }
    let path = if let Some(rest) = rest.strip_prefix("//") {
        let slash = rest.find('/')?;
        let (host, path) = rest.split_at(slash);
        if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
            return None;
        }
        path
    } else {
        rest
    };
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return None;
    }
    // Reject malformed escapes instead of opening a different literal path.
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' && !(bytes.next()?.is_ascii_hexdigit() && bytes.next()?.is_ascii_hexdigit())
        {
            return None;
        }
    }
    let decoded = percent_encoding::percent_decode_str(path).collect::<Vec<_>>();
    if decoded.contains(&0) {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(std::ffi::OsString::from_vec(decoded).into())
    }
    #[cfg(not(unix))]
    {
        Some(String::from_utf8(decoded).ok()?.into())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode_local_paths_without_form_semantics() {
        assert_eq!(
            file_url_path("file:///tmp/Repo%20%23%25%3F+%E6%97%A5%E6%9C%AC"),
            Some(PathBuf::from("/tmp/Repo #%?+日本"))
        );
        assert_eq!(
            file_url_path("FILE://LOCALHOST/tmp/repo/"),
            Some(PathBuf::from("/tmp/repo/"))
        );
        assert_eq!(file_url_path("file:/tmp/repo"), Some("/tmp/repo".into()));
    }

    #[test]
    fn file_urls_reject_nonlocal_or_ambiguous_inputs() {
        for url in [
            "https://example.com/repo",
            "file://example.com/tmp/repo",
            "file://user@localhost/tmp/repo",
            "file:relative",
            "file:///tmp/repo?query",
            "file:///tmp/repo#fragment",
            "file:///tmp/%",
            "file:///tmp/%2",
            "file:///tmp/%GG",
            "file:///tmp/%00",
        ] {
            assert_eq!(file_url_path(url), None, "{url}");
        }
    }

    #[test]
    fn native_urls_wait_for_startup_and_survive_receiver_handoff() {
        let (tx, rx) = std::sync::mpsc::channel();
        enqueue_open_urls(
            vec![
                "file:///tmp/first%20repo".into(),
                "https://example.com/ignored".into(),
                "file://localhost/tmp/second".into(),
            ],
            &tx,
        );
        // Native delivery may precede application.run / window construction.
        let application_receiver = rx;
        assert_eq!(
            application_receiver.try_recv().unwrap(),
            PathBuf::from("/tmp/first repo")
        );
        assert_eq!(
            application_receiver.try_recv().unwrap(),
            PathBuf::from("/tmp/second")
        );
        enqueue_open_urls(vec!["file:///tmp/after-close".into()], &tx);
        assert_eq!(
            application_receiver.try_recv().unwrap(),
            PathBuf::from("/tmp/after-close")
        );
        assert!(application_receiver.try_recv().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_urls_preserve_non_utf8_unix_filenames() {
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            file_url_path("file:///tmp/%FF")
                .unwrap()
                .as_os_str()
                .as_bytes(),
            b"/tmp/\xff"
        );
    }
}
